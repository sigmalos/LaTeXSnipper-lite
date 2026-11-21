//! Polygon helpers for DB text detection (replaces OpenCV + pyclipper).

pub type Pt = (f64, f64);

/// Andrew's monotone chain convex hull (counter-clockwise, no duplicates).
pub fn convex_hull(points: &[Pt]) -> Vec<Pt> {
    let mut p: Vec<Pt> = points.to_vec();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Pt, a: Pt, b: Pt| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let half = |iter: &mut dyn Iterator<Item = Pt>| {
        let mut h: Vec<Pt> = Vec::new();
        for pt in iter {
            while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], pt) <= 0.0 {
                h.pop();
            }
            h.push(pt);
        }
        h.pop();
        h
    };
    let mut hull = half(&mut p.iter().copied());
    hull.extend(half(&mut p.iter().rev().copied()));
    hull
}

/// Minimum-area enclosing rectangle via rotating calipers.
/// Returns the four corners and the length of the shorter side.
pub fn min_area_rect(points: &[Pt]) -> ([Pt; 4], f64) {
    let hull = convex_hull(points);
    if hull.is_empty() {
        return ([(0.0, 0.0); 4], 0.0);
    }
    if hull.len() < 3 {
        let (a, b) = (hull[0], *hull.last().unwrap());
        return ([a, b, b, a], 0.0);
    }
    let mut best = (f64::INFINITY, [(0.0, 0.0); 4], 0.0);
    for i in 0..hull.len() {
        let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if len == 0.0 {
            continue;
        }
        let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
        let (mut min_u, mut max_u, mut min_v, mut max_v) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for &(x, y) in &hull {
            let u = x * ux + y * uy;
            let v = -x * uy + y * ux;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
        let area = (max_u - min_u) * (max_v - min_v);
        if area < best.0 {
            let corner = |u: f64, v: f64| (u * ux - v * uy, u * uy + v * ux);
            best = (
                area,
                [
                    corner(min_u, min_v),
                    corner(max_u, min_v),
                    corner(max_u, max_v),
                    corner(min_u, max_v),
                ],
                (max_u - min_u).min(max_v - min_v),
            );
        }
    }
    (best.1, best.2)
}

/// Order like rapidocr's `get_mini_boxes`: sort by x, then pick TL, TR, BR, BL.
pub fn mini_box(rect: [Pt; 4]) -> [Pt; 4] {
    let mut p = rect.to_vec();
    p.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let (i1, i4) = if p[1].1 > p[0].1 { (0, 1) } else { (1, 0) };
    let (i2, i3) = if p[3].1 > p[2].1 { (2, 3) } else { (3, 2) };
    [p[i1], p[i2], p[i3], p[i4]]
}

pub fn polygon_area(p: &[Pt]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

pub fn polygon_perimeter(p: &[Pt]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| ((p[i].0 - p[(i + 1) % n].0).powi(2) + (p[i].1 - p[(i + 1) % n].1).powi(2)).sqrt())
        .sum()
}

/// DB "unclip": expand a rectangle outward by `area * ratio / perimeter`.
/// For a convex rectangle the round-joined offset's min-area rect is exactly
/// the rectangle grown by `d` on every side.
pub fn unclip_rect(rect: [Pt; 4], ratio: f64) -> [Pt; 4] {
    let perim = polygon_perimeter(&rect);
    if perim == 0.0 {
        return rect;
    }
    let d = polygon_area(&rect) * ratio / perim;
    let cx = rect.iter().map(|p| p.0).sum::<f64>() / 4.0;
    let cy = rect.iter().map(|p| p.1).sum::<f64>() / 4.0;
    let axis = |a: Pt, b: Pt| {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(1e-9);
        ((b.0 - a.0) / len, (b.1 - a.1) / len)
    };
    let (ux, uy) = axis(rect[0], rect[1]);
    let (vx, vy) = axis(rect[1], rect[2]);
    rect.map(|(x, y)| {
        let su = ((x - cx) * ux + (y - cy) * uy).signum();
        let sv = ((x - cx) * vx + (y - cy) * vy).signum();
        (x + d * (su * ux + sv * vx), y + d * (su * uy + sv * vy))
    })
}

pub fn point_in_polygon(x: f64, y: f64, poly: &[Pt]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Connected components (8-connectivity) of a binary bitmap; each component's
/// pixel coordinates. Equivalent input to OpenCV's contour + minAreaRect.
pub fn components(bitmap: &[bool], w: usize, h: usize) -> Vec<Vec<Pt>> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !bitmap[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut pts = Vec::new();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            // Pixel corners make the rect enclose the full pixel area like OpenCV contours.
            pts.push((x as f64, y as f64));
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let j = ny * w + nx;
                    if bitmap[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        out.push(pts);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_of_square_with_interior() {
        let h = convex_hull(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (1.0, 1.0)]);
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn axis_aligned_rect() {
        let pts: Vec<Pt> = (0..=10)
            .flat_map(|x| (0..=4).map(move |y| (x as f64, y as f64)))
            .collect();
        let (rect, short) = min_area_rect(&pts);
        assert!((short - 4.0).abs() < 1e-9);
        assert!((polygon_area(&rect) - 40.0).abs() < 1e-6);
    }

    #[test]
    fn rotated_rect_is_tight() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let pts: Vec<Pt> = [(0.0, 0.0), (10.0, 0.0), (10.0, 2.0), (0.0, 2.0)]
            .iter()
            .map(|&(x, y)| (x * s - y * s, x * s + y * s))
            .collect();
        let (rect, short) = min_area_rect(&pts);
        assert!((polygon_area(&rect) - 20.0).abs() < 1e-6);
        assert!((short - 2.0).abs() < 1e-6);
    }

    #[test]
    fn unclip_grows_each_side() {
        let r = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)];
        let g = unclip_rect(r, 1.6); // d = 40*1.6/28
        let d = 40.0 * 1.6 / 28.0;
        assert!((g[0].0 + d).abs() < 1e-9 && (g[0].1 + d).abs() < 1e-9);
        assert!((g[2].0 - 10.0 - d).abs() < 1e-9 && (g[2].1 - 4.0 - d).abs() < 1e-9);
    }

    #[test]
    fn mini_box_order() {
        let b = mini_box([(10.0, 4.0), (0.0, 0.0), (10.0, 0.0), (0.0, 4.0)]);
        assert_eq!(b, [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)]);
    }

    #[test]
    fn components_split_by_gap() {
        let w = 6;
        let mut bm = vec![false; w * 2];
        bm[0] = true;
        bm[1] = true;
        bm[4] = true;
        bm[w + 5] = true; // diagonal neighbour of 4 → same component
        let c = components(&bm, w, 2);
        assert_eq!(c.len(), 2);
        assert_eq!(c.iter().map(Vec::len).max(), Some(2));
    }

    #[test]
    fn pip() {
        let sq = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        assert!(point_in_polygon(2.0, 2.0, &sq));
        assert!(!point_in_polygon(5.0, 2.0, &sq));
    }
}
