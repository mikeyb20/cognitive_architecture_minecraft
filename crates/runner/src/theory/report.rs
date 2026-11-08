//! Theory test tables (spec §29) and the lines computed across cells.

use cognitive_core::modes::Mode;

use super::{Cell, Claim};

/// Two decimals, or a dash when a column had no data.
pub fn num(x: f64) -> String {
    if x.is_nan() {
        "–".into()
    } else {
        format!("{x:.2}")
    }
}

fn header(c: &Claim) -> Vec<&str> {
    let mut h = vec!["variant", "scenario", "survival"];
    h.extend(c.columns);
    h
}

fn cells(cell: &Cell) -> Vec<String> {
    let mut out = vec![cell.variant.clone(), cell.scenario.clone()];
    out.extend(cell.means().into_iter().map(num));
    out
}

/// The claim as a Markdown table, with the cross-cell line if it has one.
pub fn markdown(c: &Claim, rows: &[Cell], seeds: u64) -> String {
    let h = header(c);
    let mut t = format!(
        "## {}: {}\n\n{} {} seeds per cell; survival = share alive at the end.\n\n",
        c.id, c.claim, c.reading, seeds
    );
    t += &format!("| {} |\n|{}\n", h.join(" | "), "---|".repeat(h.len()));
    for r in rows {
        t += &format!("| {} |\n", cells(r).join(" | "));
    }
    if let Some(post) = c.post {
        t += &format!("\n{}\n", post(rows));
    }
    t
}

/// The claim as CSV, one line per cell.
pub fn csv(c: &Claim, rows: &[Cell]) -> String {
    let mut t = format!("claim,{}\n", header(c).join(","));
    for r in rows {
        let c2: Vec<String> = cells(r)
            .into_iter()
            .map(|x| {
                if x.contains(',') {
                    format!("\"{x}\"")
                } else {
                    x
                }
            })
            .collect();
        t += &format!("{},{}\n", c.id, c2.join(","));
    }
    t
}

/// Scenarios in first-seen order.
fn scenarios(rows: &[Cell]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for r in rows {
        if !out.contains(&r.scenario.as_str()) {
            out.push(&r.scenario);
        }
    }
    out
}

/// T-IND: for each "only X learns" row, the largest relative change in the
/// other lenses' success against the all-frozen row (columns 1..=4).
pub fn independence(rows: &[Cell]) -> String {
    let mut parts = Vec::new();
    for sc in scenarios(rows) {
        let find = |label: &str| rows.iter().find(|r| r.scenario == sc && r.variant == label);
        let Some(base) = find("all frozen").map(Cell::means) else {
            continue;
        };
        let mut moved = Vec::new();
        for m in Mode::ALL {
            let Some(row) = find(&format!("only {m:?} learns")).map(Cell::means) else {
                continue;
            };
            let worst = (0..4)
                .filter(|&i| i != m.index())
                .map(|i| ((row[i + 1] - base[i + 1]) / base[i + 1]).abs())
                .filter(|x| x.is_finite())
                .fold(f64::NAN, f64::max);
            let pct = if worst.is_nan() {
                "–".to_string()
            } else {
                format!("{:.1}%", 100.0 * worst)
            };
            moved.push(format!("{m:?} {pct}"));
        }
        parts.push(format!("{sc}: {}", moved.join(", ")));
    }
    format!(
        "Largest change in the frozen lenses' success when one lens learns (claim: ≤ 5%): {}",
        parts.join("; ")
    )
}

/// T-MODE: leave-one-out nearest-centroid accuracy at telling the forced
/// mode from a run's numbers (survival left out), per scenario.
pub fn mode_accuracy(rows: &[Cell]) -> String {
    let mut parts = Vec::new();
    for sc in scenarios(rows) {
        let mut runs: Vec<(usize, Vec<f64>)> = Vec::new();
        for (class, r) in rows.iter().filter(|r| r.scenario == sc).enumerate() {
            for x in &r.runs {
                let f = x[1..].iter().map(|v| if v.is_nan() { 0.0 } else { *v });
                runs.push((class, f.collect()));
            }
        }
        parts.push(format!("{sc} {}", num(loo_accuracy(&runs))));
    }
    format!(
        "Nearest-centroid accuracy (leave one out; chance = 1 / modes): {}",
        parts.join(", ")
    )
}

/// Share of points whose nearest class centroid, computed without the
/// point, is their own class. NaN with fewer than two points.
pub fn loo_accuracy(points: &[(usize, Vec<f64>)]) -> f64 {
    if points.len() < 2 {
        return f64::NAN;
    }
    let classes = points.iter().map(|p| p.0).max().unwrap_or(0) + 1;
    let mut right = 0;
    for (k, (class, x)) in points.iter().enumerate() {
        let mut best = (f64::INFINITY, usize::MAX);
        for c in 0..classes {
            let others: Vec<&Vec<f64>> = points
                .iter()
                .enumerate()
                .filter(|(j, p)| *j != k && p.0 == c)
                .map(|(_, p)| &p.1)
                .collect();
            if others.is_empty() {
                continue;
            }
            let n = others.len() as f64;
            let d: f64 = (0..x.len())
                .map(|i| x[i] - others.iter().map(|o| o[i]).sum::<f64>() / n)
                .map(|e| e * e)
                .sum();
            if d < best.0 {
                best = (d, c);
            }
        }
        right += usize::from(best.1 == *class);
    }
    right as f64 / points.len() as f64
}

/// Scale each column to mean 0 and sd 1 across points, so no column wins
/// the distance by its units. Constant columns become 0.
pub fn standardise(points: &mut [(usize, Vec<f64>)]) {
    let n = points.len() as f64;
    let cols = points.first().map_or(0, |p| p.1.len());
    for i in 0..cols {
        let mean = points.iter().map(|p| p.1[i]).sum::<f64>() / n;
        let var = points.iter().map(|p| (p.1[i] - mean).powi(2)).sum::<f64>() / n;
        let sd = var.sqrt();
        for p in points.iter_mut() {
            p.1[i] = if sd > 0.0 { (p.1[i] - mean) / sd } else { 0.0 };
        }
    }
}

fn centroid(xs: &[&Vec<f64>]) -> Vec<f64> {
    let n = xs.len() as f64;
    let cols = xs.first().map_or(0, |x| x.len());
    (0..cols)
        .map(|i| xs.iter().map(|x| x[i]).sum::<f64>() / n)
        .collect()
}

fn dist2(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}

/// P2 (plan §5.3): (within, between). Within = mean squared distance of a
/// point to its class centroid; between = mean squared distance of a
/// point's class centroid to the overall centroid.
pub fn spread(points: &[(usize, Vec<f64>)]) -> (f64, f64) {
    let n = points.len() as f64;
    let all: Vec<&Vec<f64>> = points.iter().map(|p| &p.1).collect();
    let grand = centroid(&all);
    let (mut within, mut between) = (0.0, 0.0);
    for (class, x) in points {
        let mine: Vec<&Vec<f64>> = points
            .iter()
            .filter(|p| p.0 == *class)
            .map(|p| &p.1)
            .collect();
        let c = centroid(&mine);
        within += dist2(x, &c);
        between += dist2(&c, &grand);
    }
    (within / n, between / n)
}

/// P1 and P2 per scenario (plan §5.3): leave-one-out nearest-centroid
/// accuracy on standardised behaviour, and within/between spread.
pub fn personality_lines(rows: &[Cell]) -> String {
    let (mut p1, mut p2) = (Vec::new(), Vec::new());
    for sc in scenarios(rows) {
        let mut runs: Vec<(usize, Vec<f64>)> = Vec::new();
        for (class, r) in rows.iter().filter(|r| r.scenario == sc).enumerate() {
            for x in &r.runs {
                let f = x[1..].iter().map(|v| if v.is_nan() { 0.0 } else { *v });
                runs.push((class, f.collect()));
            }
        }
        standardise(&mut runs);
        p1.push(format!("{sc} {}", num(loo_accuracy(&runs))));
        let (w, b) = spread(&runs);
        p2.push(format!("{sc} {}", num(w / b)));
    }
    format!(
        "P1 nearest-centroid accuracy (leave one out, standardised; chance 0.25): {}\n\nP2 within/between spread (individual if above 0, distinct if below 1): {}",
        p1.join(", "),
        p2.join(", ")
    )
}
