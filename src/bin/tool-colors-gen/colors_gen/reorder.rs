#![forbid(unsafe_code)]

use super::generator::Swatch;
use palette::Oklab;

#[inline]
fn dist_oklab(a: Oklab, b: Oklab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    db.mul_add(db, dl.mul_add(dl, da * da)).sqrt()
}

/// Maximin-to-set ordering in `OKLab` (deterministic seed = most chromatic).
#[must_use]
pub fn reorder_maximin(swatches: &[Swatch]) -> Vec<Swatch> {
    let n = swatches.len();
    let mut remaining: Vec<usize> = (0..n).collect();

    // Seed: most chromatic (largest a^2 + b^2)
    let seed = *remaining
        .iter()
        .max_by(|&&i, &&j| {
            let ci = swatches[i].lab.a.hypot(swatches[i].lab.b);
            let cj = swatches[j].lab.a.hypot(swatches[j].lab.b);
            ci.partial_cmp(&cj).unwrap()
        })
        .unwrap();

    let mut order = vec![seed];
    remaining.retain(|&k| k != seed);

    let mut dmin = vec![f32::INFINITY; n];
    for &idx in &remaining {
        dmin[idx] = dist_oklab(swatches[idx].lab, swatches[seed].lab);
    }

    while !remaining.is_empty() {
        let (pos, _) = remaining
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| dmin[**a].partial_cmp(&dmin[**b]).unwrap())
            .unwrap();
        let chosen = remaining.swap_remove(pos);
        order.push(chosen);
        for &idx in &remaining {
            let d = dist_oklab(swatches[idx].lab, swatches[chosen].lab);
            if d < dmin[idx] {
                dmin[idx] = d;
            }
        }
    }

    order.into_iter().map(|i| swatches[i]).collect()
}
