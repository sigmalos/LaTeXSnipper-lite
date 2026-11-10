//! Reading-order layout for mixed text + formula pages (condensed port of
//! `mathcraft_ocr/layout.py`).

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Block {
    /// "text", "embedding" (inline formula), "isolated" (display formula)
    pub kind: &'static str,
    pub xyxy: [f64; 4],
    pub text: String,
    pub score: f64,
}

impl Block {
    pub fn is_formula(&self) -> bool {
        self.kind != "text"
    }
    fn is_display(&self) -> bool {
        self.kind == "isolated"
    }
}

fn y_overlap(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let overlap = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    overlap / (a[3] - a[1]).min(b[3] - b[1]).max(1.0)
}

pub fn overlap_ratio(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let area = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    if area <= 0.0 {
        return 0.0;
    }
    let iw = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let ih = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    iw * ih / area
}

/// Split a text line box into the horizontal pieces not covered by formulas.
pub fn split_text_around_formulas(text: &[f64; 4], formulas: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let mut relevant: Vec<&[f64; 4]> = formulas
        .iter()
        .filter(|f| overlap_ratio(text, f) >= 0.1 || y_overlap(text, f) >= 0.6)
        .collect();
    relevant.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    let mut intervals = vec![(text[0], text[2])];
    for f in relevant {
        let mut next = Vec::new();
        for (s, e) in intervals {
            if f[2] <= s || f[0] >= e {
                next.push((s, e));
                continue;
            }
            let left_end = s.max(e.min(f[0]));
            let right_start = e.min(s.max(f[2]));
            if left_end - s >= 8.0 {
                next.push((s, left_end));
            }
            if e - right_start >= 8.0 {
                next.push((right_start, e));
            }
        }
        intervals = next;
    }
    intervals
        .into_iter()
        .filter(|(s, e)| e - s >= 8.0)
        .map(|(s, e)| [s, text[1], e, text[3]])
        .collect()
}

fn is_page_number(text: &str) -> bool {
    let t = text.trim();
    (1..=4).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit())
        || (1..=8).contains(&t.len()) && t.chars().all(|c| "ivxlcdmIVXLCDM".contains(c))
}

fn is_formula_label(text: &str) -> bool {
    let t = text.trim().trim_start_matches('(').trim_end_matches(')');
    !t.is_empty()
        && t.len() <= 10
        && t.chars().next().is_some_and(|c| c.is_ascii_digit())
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
}

/// Drop OCR text that a formula box swallows (keep equation labels).
pub fn resolve_conflicts(blocks: Vec<Block>) -> Vec<Block> {
    let formulas: Vec<[f64; 4]> = blocks
        .iter()
        .filter(|b| b.is_formula())
        .map(|b| b.xyxy)
        .collect();
    blocks
        .into_iter()
        .filter(|b| {
            if b.is_formula() {
                return true;
            }
            let t = b.text.trim();
            !t.is_empty()
                && (is_formula_label(t)
                    || !formulas.iter().any(|f| overlap_ratio(&b.xyxy, f) >= 0.82))
        })
        .collect()
}

fn column_of(b: &[f64; 4], width: f64) -> usize {
    let mid = width / 2.0;
    if b[0] >= mid {
        1
    } else if b[2] <= mid
        || b[2] - b[0] >= width * 0.7
        || (b[0] <= width * 0.45 && b[2] >= width * 0.55)
    {
        0
    } else {
        ((b[0] + b[2]) / 2.0 >= mid) as usize
    }
}

pub fn is_two_column(blocks: &[Block], width: f64) -> bool {
    if width <= 0.0 {
        return false;
    }
    let text: Vec<&Block> = blocks
        .iter()
        .filter(|b| b.kind == "text" && !b.text.trim().is_empty())
        .collect();
    if text.len() < 3 {
        return false;
    }
    let (mut wide, mut left, mut right) = (0, 0, 0);
    for b in &text {
        let (x1, x2) = (b.xyxy[0], b.xyxy[2]);
        let center = (x1 + x2) / 2.0;
        if x2 - x1 >= width * 0.62 || (x1 <= width * 0.45 && x2 >= width * 0.55) {
            wide += 1;
        } else if center < width * 0.46 {
            left += 1;
        } else if center > width * 0.54 {
            right += 1;
        }
    }
    if wide >= 2.max(text.len() * 22 / 100) {
        return false;
    }
    if text.len() < 6 {
        left >= 2 && right >= 2 && wide == 0
    } else {
        left >= 3 && right >= 3
    }
}

/// Group blocks into visual lines in reading order.
pub fn group_lines(blocks: &[Block], width: f64) -> Vec<Vec<Block>> {
    let two_col = is_two_column(blocks, width);
    let col = |b: &Block| {
        if two_col {
            column_of(&b.xyxy, width)
        } else {
            0
        }
    };
    let mut sorted = blocks.to_vec();
    sorted.sort_by(|a, b| {
        (col(a), a.xyxy[1], a.xyxy[0])
            .partial_cmp(&(col(b), b.xyxy[1], b.xyxy[0]))
            .unwrap()
    });
    let mut lines: Vec<Vec<Block>> = Vec::new();
    for block in sorted {
        let mut best: Option<(usize, f64)> = None;
        for (i, line) in lines.iter().enumerate() {
            let first = &line[0];
            if first.is_display() || block.is_display() || col(first) != col(&block) {
                continue;
            }
            let ov = y_overlap(&union(line), &block.xyxy);
            if best.is_none_or(|(_, b)| ov > b) {
                best = Some((i, ov));
            }
        }
        match best {
            Some((i, ov)) if ov >= 0.45 => lines[i].push(block),
            _ => lines.push(vec![block]),
        }
    }
    for line in &mut lines {
        line.sort_by(|a, b| a.xyxy[0].partial_cmp(&b.xyxy[0]).unwrap());
    }
    lines.sort_by(|a, b| {
        let (ua, ub) = (union(a), union(b));
        (col(&a[0]), ua[1], ua[0])
            .partial_cmp(&(col(&b[0]), ub[1], ub[0]))
            .unwrap()
    });
    lines
}

fn union(line: &[Block]) -> [f64; 4] {
    line.iter()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |u, b| {
            [
                u[0].min(b.xyxy[0]),
                u[1].min(b.xyxy[1]),
                u[2].max(b.xyxy[2]),
                u[3].max(b.xyxy[3]),
            ]
        })
}

/// Merge blocks into Markdown-style text with `$…$` / `$$…$$` math.
pub fn merge_text(blocks: &[Block], size: (f64, f64)) -> String {
    let (width, height) = size;
    let mut out: Vec<String> = Vec::new();
    for line in group_lines(blocks, width) {
        let u = union(&line);
        let in_margin = u[1] < height * 0.06 || u[3] > height * 0.94;
        if line.len() == 1
            && line[0].kind == "text"
            && in_margin
            && is_page_number(&line[0].text)
            && height > 400.0
        {
            continue;
        }
        let mut merged = String::new();
        let mut display = false;
        for b in &line {
            let t = b.text.trim();
            if t.is_empty() {
                continue;
            }
            let part = match b.kind {
                "isolated" => {
                    display = true;
                    format!("$$\n{t}\n$$")
                }
                "embedding" => format!(" ${t}$ "),
                _ => t.to_string(),
            };
            if merged.is_empty()
                || merged.ends_with([' ', '\n'])
                || part.starts_with([' ', '\n', '$'])
            {
                merged.push_str(&part);
            } else {
                merged.push(' ');
                merged.push_str(&part);
            }
        }
        let merged = merged.trim().to_string();
        if !merged.is_empty() {
            out.push(if display {
                format!("\n{merged}\n")
            } else {
                merged
            });
        }
    }
    let mut text = out.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(kind: &'static str, xyxy: [f64; 4], text: &str) -> Block {
        Block {
            kind,
            xyxy,
            text: text.into(),
            score: 0.9,
        }
    }

    #[test]
    fn splits_text_around_inline_formula() {
        let parts =
            split_text_around_formulas(&[0.0, 0.0, 100.0, 10.0], &[[40.0, 0.0, 60.0, 10.0]]);
        assert_eq!(
            parts,
            vec![[0.0, 0.0, 40.0, 10.0], [60.0, 0.0, 100.0, 10.0]]
        );
        assert_eq!(
            split_text_around_formulas(&[0.0, 0.0, 10.0, 10.0], &[[0.0, 0.0, 10.0, 10.0]]),
            Vec::<[f64; 4]>::new()
        );
    }

    #[test]
    fn merges_inline_and_display_math() {
        let blocks = vec![
            b("text", [0.0, 0.0, 40.0, 10.0], "Let"),
            b("embedding", [45.0, 0.0, 60.0, 10.0], "x"),
            b("text", [65.0, 0.0, 120.0, 10.0], "be real."),
            b("isolated", [20.0, 30.0, 100.0, 60.0], "x^2 = 1"),
            b("text", [0.0, 70.0, 60.0, 80.0], "Done."),
        ];
        assert_eq!(
            merge_text(&blocks, (200.0, 100.0)),
            "Let $x$ be real.\n\n$$\nx^2 = 1\n$$\n\nDone."
        );
    }

    #[test]
    fn drops_swallowed_text_but_keeps_labels() {
        let blocks = vec![
            b("isolated", [0.0, 0.0, 100.0, 20.0], "a=b"),
            b("text", [10.0, 2.0, 50.0, 18.0], "a=b"),
            b("text", [80.0, 2.0, 98.0, 18.0], "(1)"),
        ];
        let kept = resolve_conflicts(blocks);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].text, "(1)");
    }

    #[test]
    fn detects_two_columns_and_orders_reading() {
        let mut blocks = Vec::new();
        for i in 0..4 {
            let y = i as f64 * 20.0;
            blocks.push(b("text", [10.0, y, 90.0, y + 10.0], &format!("L{i}")));
            blocks.push(b("text", [110.0, y, 190.0, y + 10.0], &format!("R{i}")));
        }
        assert!(is_two_column(&blocks, 200.0));
        assert_eq!(
            merge_text(&blocks, (200.0, 100.0)),
            "L0\nL1\nL2\nL3\nR0\nR1\nR2\nR3"
        );
    }

    #[test]
    fn page_number_in_margin_is_dropped() {
        let blocks = vec![
            b("text", [0.0, 100.0, 300.0, 120.0], "Body text"),
            b("text", [140.0, 980.0, 160.0, 995.0], "12"),
        ];
        assert_eq!(merge_text(&blocks, (300.0, 1000.0)), "Body text");
    }
}
