//! LaTeX post-processing: quality flags, multi-line alignment and
//! render-safety repairs. Ported from `mathcraft_ocr/{latex_quality,
//! latex_alignment,formula_lines}.py`.

use std::collections::{BTreeSet, HashMap};

pub const SEVERE_FLAGS: [&str; 5] = [
    "duplicate_relation",
    "repeated_token_run",
    "excessive_repeated_token",
    "unbalanced_group",
    "mismatched_environment",
];

fn is_escaped(chars: &[char], index: usize) -> bool {
    let mut n = 0;
    let mut i = index;
    while i > 0 && chars[i - 1] == '\\' {
        n += 1;
        i -= 1;
    }
    n % 2 == 1
}

/// Tokens matching `\\[A-Za-z]+|[A-Za-z]+|\d+|[^\s]`.
fn tokens(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let start = i;
        if c[i] == '\\' && i + 1 < c.len() && c[i + 1].is_ascii_alphabetic() {
            i += 1;
            while i < c.len() && c[i].is_ascii_alphabetic() {
                i += 1;
            }
        } else if c[i].is_ascii_alphabetic() {
            while i < c.len() && c[i].is_ascii_alphabetic() {
                i += 1;
            }
        } else if c[i].is_ascii_digit() {
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
        } else if c[i].is_whitespace() {
            i += 1;
            continue;
        } else {
            i += 1;
        }
        out.push(c[start..i].iter().collect());
    }
    out
}

/// Every `\left` / `\right` not followed by a letter, as (index, is_left).
fn left_right_tokens(c: &[char]) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for i in 0..c.len() {
        if c[i] != '\\' {
            continue;
        }
        for (word, left) in [("left", true), ("right", false)] {
            let n = word.len();
            if i + 1 + n <= c.len()
                && c[i + 1..i + 1 + n].iter().copied().eq(word.chars())
                && !c.get(i + 1 + n).is_some_and(|ch| ch.is_ascii_alphabetic())
            {
                out.push((i, left));
            }
        }
    }
    out
}

/// `\begin{env}` / `\end{env}` occurrences as (is_begin, env, start, end).
fn environments(text: &str) -> Vec<(bool, String, usize, usize)> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '\\' {
            i += 1;
            continue;
        }
        let kind = if c[i..].starts_with(&['\\', 'b', 'e', 'g', 'i', 'n']) {
            Some((true, 6))
        } else if c[i..].starts_with(&['\\', 'e', 'n', 'd']) {
            Some((false, 4))
        } else {
            None
        };
        let Some((begin, len)) = kind else {
            i += 1;
            continue;
        };
        let mut j = i + len;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if j >= c.len() || c[j] != '{' {
            i += 1;
            continue;
        }
        let close = c[j + 1..].iter().position(|&ch| ch == '}' || ch == '{');
        match close {
            Some(off) if c[j + 1 + off] == '}' => {
                let env: String = c[j + 1..j + 1 + off].iter().collect();
                if !env.trim().is_empty() {
                    out.push((begin, env.trim().to_string(), i, j + 2 + off));
                }
                i = j + 2 + off;
            }
            _ => i += 1,
        }
    }
    out
}

pub fn quality_flags(text: &str) -> Vec<&'static str> {
    let mut flags = BTreeSet::new();
    let c: Vec<char> = text.chars().collect();
    // (?<!\\)=\s*=
    for i in 0..c.len() {
        if c[i] == '=' && !(i > 0 && c[i - 1] == '\\') {
            let mut j = i + 1;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if j < c.len() && c[j] == '=' {
                flags.insert("duplicate_relation");
                break;
            }
        }
    }
    let toks = tokens(text);
    let mut run = 0;
    for (i, t) in toks.iter().enumerate() {
        run = if i > 0 && toks[i - 1] == *t {
            run + 1
        } else {
            1
        };
        if run >= 8 {
            flags.insert("repeated_token_run");
            break;
        }
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in &toks {
        if matches!(t.as_str(), "\\quad" | "\\qquad" | "\\cdots" | "\\ldots") {
            *counts.entry(t.as_str()).or_default() += 1;
        }
    }
    if counts.values().any(|&n| n >= 18) {
        flags.insert("excessive_repeated_token");
    }
    let mut depth = 0i64;
    for i in 0..c.len() {
        if c[i] == '{' && !is_escaped(&c, i) {
            depth += 1;
        } else if c[i] == '}' && !is_escaped(&c, i) {
            depth -= 1;
        }
    }
    if depth != 0 {
        flags.insert("unbalanced_group");
    }
    let mut stack: Vec<String> = Vec::new();
    let mut balanced = true;
    for (begin, env, _, _) in environments(text) {
        if begin {
            stack.push(env);
        } else if stack.pop().as_deref() != Some(env.as_str()) {
            balanced = false;
            break;
        }
    }
    if !balanced || !stack.is_empty() {
        flags.insert("mismatched_environment");
    }
    let lr = left_right_tokens(&c);
    if lr.iter().filter(|t| t.1).count() != lr.iter().filter(|t| !t.1).count() {
        flags.insert("unbalanced_left_right");
    }
    flags.into_iter().collect()
}

pub fn severe_count(text: &str) -> usize {
    quality_flags(text)
        .iter()
        .filter(|f| SEVERE_FLAGS.contains(f))
        .count()
}

pub fn has_severe_issue(text: &str) -> bool {
    severe_count(text) > 0
}

// ---------------------------------------------------------------- alignment

const RELATION_COMMANDS: [&str; 7] = [
    "\\leqslant",
    "\\geqslant",
    "\\leq",
    "\\geq",
    "\\approx",
    "\\equiv",
    "\\sim",
];

fn read_command(c: &[char], i: usize) -> String {
    let mut end = i + 1;
    while end < c.len() && c[end].is_alphabetic() {
        end += 1;
    }
    c[i..end].iter().collect()
}

pub fn find_alignment_relation(text: &str) -> Option<(String, String, String)> {
    let c: Vec<char> = text.chars().collect();
    let split = |i: usize, op: &str| {
        let left: String = c[..i].iter().collect();
        let right: String = c[i + op.chars().count()..].iter().collect();
        (
            left.trim().to_string(),
            op.to_string(),
            right.trim().to_string(),
        )
    };
    let mut depth = 0usize;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' {
            let cmd = read_command(&c, i);
            if depth == 0 && RELATION_COMMANDS.contains(&cmd.as_str()) {
                return Some(split(i, &cmd));
            }
            i += cmd.chars().count().max(1);
            continue;
        }
        if ch == '{' && !is_escaped(&c, i) {
            depth += 1;
        } else if ch == '}' && !is_escaped(&c, i) {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && matches!(ch, '=' | '<' | '>') && !is_escaped(&c, i) {
            let dup = ch == '='
                && (c[..i].iter().rev().find(|x| !x.is_whitespace()) == Some(&'=')
                    || c[i + 1..].iter().find(|x| !x.is_whitespace()) == Some(&'='));
            if !dup {
                return Some(split(i, &ch.to_string()));
            }
        }
        i += 1;
    }
    None
}

pub fn align_relation_lines(lines: &[String]) -> Vec<String> {
    let cleaned: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if cleaned.len() <= 1 {
        return cleaned.into_iter().map(String::from).collect();
    }
    let mut out = Vec::new();
    let mut saw = false;
    for (i, line) in cleaned.iter().enumerate() {
        match find_alignment_relation(line) {
            None if i > 0 && saw => out.push(format!("&\\quad {line}")),
            None => out.push(line.to_string()),
            Some((left, op, right)) => {
                saw = true;
                let s = if left.is_empty() {
                    format!("&{op} {right}")
                } else {
                    format!("{left} &{op} {right}")
                };
                out.push(s.trim_end().to_string());
            }
        }
    }
    out
}

// ---------------------------------------------------------- render safety

fn strip_begin_end(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut last = 0;
    for (_, _, start, end) in environments(text) {
        if start < last {
            continue;
        }
        let mut stop = end;
        // optional trailing `{...}` argument (e.g. \begin{array}{cc})
        let mut j = end;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if j < c.len() && c[j] == '{' {
            if let Some(off) = c[j + 1..].iter().position(|&ch| ch == '}' || ch == '{') {
                if c[j + 1 + off] == '}' {
                    stop = j + 2 + off;
                }
            }
        }
        out.extend(&c[last..start]);
        last = stop;
    }
    out.extend(&c[last..]);
    out
}

fn strip_outer_group(text: &str) -> String {
    let s = text.trim();
    let c: Vec<char> = s.chars().collect();
    if c.len() >= 2 && c[0] == '{' && c[c.len() - 1] == '}' && !is_escaped(&c, c.len() - 1) {
        return c[1..c.len() - 1]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
    }
    s.to_string()
}

fn strip_wrappers(line: &str) -> String {
    strip_outer_group(strip_begin_end(line.trim()).trim())
}

/// Regex `(?<!\\)(?:\\\\)+`: a `\\` pair not preceded by another backslash.
fn contains_row_break(c: &[char]) -> bool {
    (0..c.len().saturating_sub(1))
        .any(|i| c[i] == '\\' && c[i + 1] == '\\' && (i == 0 || c[i - 1] != '\\'))
}

fn drop_left_right(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let lr = left_right_tokens(&c);
    let mut out = String::new();
    let mut i = 0;
    let mut k = 0;
    while i < c.len() {
        if k < lr.len() && lr[k].0 == i {
            let len = if lr[k].1 { 5 } else { 6 };
            let mut j = i + len;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if j < c.len() && c[j] == '.' {
                j += 1; // invisible delimiter `\left.` disappears entirely
            }
            i = j;
            k += 1;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out.trim().to_string()
}

fn left_right_render_safe(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let lr = left_right_tokens(&c);
    if contains_row_break(&c) && !lr.is_empty() {
        return drop_left_right(text);
    }
    let mut depth = 0i64;
    let mut ok = true;
    for (_, left) in &lr {
        depth += if *left { 1 } else { -1 };
        if depth < 0 {
            ok = false;
            break;
        }
    }
    if ok && depth == 0 {
        return text.to_string();
    }
    drop_left_right(text)
}

fn alignment_tabs_render_safe(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    for i in 0..c.len() {
        if c[i] == '&' && !is_escaped(&c, i) {
            out.push_str("\\quad ");
        } else {
            out.push(c[i]);
        }
    }
    out.trim().to_string()
}

fn tex_groups_render_safe(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut depth = 0;
    let mut out = String::new();
    for i in 0..c.len() {
        let ch = c[i];
        if !matches!(ch, '{' | '}') || is_escaped(&c, i) {
            out.push(ch);
        } else if ch == '{' {
            depth += 1;
            out.push(ch);
        } else if depth > 0 {
            depth -= 1;
            out.push(ch);
        }
    }
    let mut s = out.trim().to_string();
    s.push_str(&"}".repeat(depth));
    s
}

fn skip_space_left(c: &[char], mut pos: isize) -> isize {
    while pos >= 0 && c[pos as usize].is_whitespace() {
        pos -= 1;
    }
    pos
}

fn matching_open_brace(c: &[char], close: usize) -> Option<usize> {
    let mut depth = 0;
    for i in (0..=close).rev() {
        if !matches!(c[i], '{' | '}') || is_escaped(c, i) {
            continue;
        }
        if c[i] == '}' {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn skip_script_arg_left(c: &[char], pos: isize) -> isize {
    let mut pos = skip_space_left(c, pos);
    if pos < 0 {
        return pos;
    }
    let p = pos as usize;
    if c[p] == '}' && !is_escaped(c, p) {
        return matching_open_brace(c, p).map_or(pos - 1, |s| s as isize - 1);
    }
    if c[p].is_alphabetic() {
        while pos >= 0 && c[pos as usize].is_alphabetic() {
            pos -= 1;
        }
        if pos >= 0 && c[pos as usize] == '\\' {
            pos -= 1;
        }
        return pos;
    }
    pos - 1
}

fn base_start_left(c: &[char], pos: isize) -> Option<usize> {
    let mut pos = skip_space_left(c, pos);
    if pos < 0 {
        return None;
    }
    let p = pos as usize;
    if c[p] == '}' && !is_escaped(c, p) {
        return Some(matching_open_brace(c, p).unwrap_or(p));
    }
    if c[p].is_alphabetic() {
        while pos >= 0 && c[pos as usize].is_alphabetic() {
            pos -= 1;
        }
        if pos >= 0 && c[pos as usize] == '\\' {
            return Some(pos as usize);
        }
        return Some((pos + 1) as usize);
    }
    Some(p)
}

fn scripted_atom_start(c: &[char], script: usize) -> Option<usize> {
    let mut pos = skip_space_left(c, script as isize - 1);
    if pos < 0 {
        return None;
    }
    loop {
        let before = skip_space_left(c, skip_script_arg_left(c, pos));
        if before >= 0 && matches!(c[before as usize], '^' | '_') && !is_escaped(c, before as usize)
        {
            pos = skip_space_left(c, before - 1);
            if pos < 0 {
                return None;
            }
            continue;
        }
        break;
    }
    base_start_left(c, pos)
}

fn atom_has_top_level_script(atom: &[char], script: char) -> bool {
    let mut depth = 0i64;
    for i in 0..atom.len() {
        if matches!(atom[i], '{' | '}') && !is_escaped(atom, i) {
            depth = (depth + if atom[i] == '{' { 1 } else { -1 }).max(0);
            continue;
        }
        if depth == 0 && atom[i] == script && !is_escaped(atom, i) {
            return true;
        }
    }
    false
}

/// Wrap `x^a^b` style double scripts as `{x^a}^b` so MathJax can render them.
fn double_scripts_render_safe(text: &str) -> String {
    let mut c: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if !matches!(ch, '^' | '_') || is_escaped(&c, i) {
            i += 1;
            continue;
        }
        let Some(start) = scripted_atom_start(&c, i) else {
            i += 1;
            continue;
        };
        if start >= i || !atom_has_top_level_script(&c[start..i], ch) {
            i += 1;
            continue;
        }
        let atom: String = c[start..i].iter().collect();
        let mut next: Vec<char> = c[..start].to_vec();
        next.push('{');
        next.extend(atom.trim_end().chars());
        next.push('}');
        let new_i = next.len();
        next.extend(&c[i..]);
        c = next;
        i = new_i + 1;
    }
    c.into_iter().collect()
}

pub fn clean_formula_line(line: &str) -> String {
    tex_groups_render_safe(&alignment_tabs_render_safe(&double_scripts_render_safe(
        &left_right_render_safe(&strip_wrappers(line)),
    )))
}

pub fn compose_formula_line(segments: &[String]) -> String {
    let joined = segments
        .iter()
        .map(|s| strip_wrappers(s))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    tex_groups_render_safe(&alignment_tabs_render_safe(&double_scripts_render_safe(
        &left_right_render_safe(joined.trim()),
    )))
}

pub fn compose_aligned_formula(lines: &[String]) -> String {
    let cleaned: Vec<String> = lines
        .iter()
        .map(|l| clean_formula_line(l))
        .filter(|l| !l.is_empty())
        .collect();
    match cleaned.len() {
        0 => String::new(),
        1 => cleaned[0].clone(),
        _ => format!(
            "\\begin{{aligned}}\n{}\n\\end{{aligned}}",
            align_relation_lines(&cleaned).join(" \\\\\n")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn flags_detect_common_failures() {
        assert!(quality_flags("a = = b").contains(&"duplicate_relation"));
        assert!(quality_flags("\\frac{a}{b").contains(&"unbalanced_group"));
        assert!(quality_flags("\\begin{cases} x \\end{array}").contains(&"mismatched_environment"));
        assert!(quality_flags("\\left( x").contains(&"unbalanced_left_right"));
        assert!(quality_flags(&"x ".repeat(9)).contains(&"repeated_token_run"));
        assert!(quality_flags(&"\\quad a ".repeat(18)).contains(&"excessive_repeated_token"));
        assert!(quality_flags("\\frac{a}{b} = \\left( c \\right)").is_empty());
        assert!(quality_flags("\\{ x \\}").is_empty());
    }

    #[test]
    fn severity_ignores_left_right_only() {
        assert!(!has_severe_issue("\\left( x"));
        assert!(has_severe_issue("{x"));
    }

    #[test]
    fn finds_relations_outside_groups() {
        assert_eq!(
            find_alignment_relation("{a=b} \\leq c"),
            Some(("{a=b}".into(), "\\leq".into(), "c".into()))
        );
        assert_eq!(find_alignment_relation("x == y"), None);
        assert_eq!(find_alignment_relation("abc"), None);
    }

    #[test]
    fn aligns_lines_on_relations() {
        let out = align_relation_lines(&s(&["f(x) = x^2", "= y", "+ 1"]));
        assert_eq!(out, s(&["f(x) &= x^2", "&= y", "&\\quad + 1"]));
    }

    #[test]
    fn composes_aligned_block() {
        let out = compose_aligned_formula(&s(&["a = b", "", "{c = d}"]));
        assert_eq!(out, "\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}");
        assert_eq!(compose_aligned_formula(&s(&["x"])), "x");
        assert_eq!(compose_aligned_formula(&[]), "");
    }

    #[test]
    fn render_safety_repairs() {
        assert_eq!(clean_formula_line("\\left( x"), "( x");
        assert_eq!(
            clean_formula_line("\\left. x \\right|"),
            "\\left. x \\right|"
        );
        assert_eq!(
            clean_formula_line("\\left( a \\\\ b \\right)"),
            "( a \\\\ b )"
        );
        assert_eq!(clean_formula_line("a & b"), "a \\quad  b");
        assert_eq!(clean_formula_line("\\frac{a}{b"), "\\frac{a}{b}");
        assert_eq!(clean_formula_line("a}}"), "a");
        assert_eq!(clean_formula_line("x^a^b"), "{x^a}^b");
        assert_eq!(clean_formula_line("\\begin{array}{cc} a \\end{array}"), "a");
        assert_eq!(
            clean_formula_line("\\left( a \\right)"),
            "\\left( a \\right)"
        );
    }

    #[test]
    fn composes_segments_on_one_line() {
        assert_eq!(compose_formula_line(&s(&["{a+b}", "", "= c"])), "a+b = c");
    }
}
