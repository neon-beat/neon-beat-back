#![forbid(unsafe_code)]

// Hue warp: peak-normalized von Mises notch
// vm(h) = exp(kappa * (cos(h - mu) - 1))  ∈ (0, 1], vm(mu) = 1
// weight(h) = 1 - strength * vm(h)
#[derive(Clone, Copy, Debug)]
pub struct WarpParams {
    pub mu_deg: f32,
    pub strength: f32,
    pub kappa: f32,
}

impl WarpParams {
    #[must_use]
    pub fn new(mu_deg: f32, strength: f32, kappa: f32) -> Self {
        debug_assert!(strength.is_finite() && (0.0..=1.0).contains(&strength));
        debug_assert!(kappa.is_finite() && kappa > 0.0);
        Self {
            mu_deg: (mu_deg % 360.0 + 360.0) % 360.0,
            strength: strength.clamp(0.0, 1.0),
            kappa,
        }
    }
}

#[must_use = "HueCdf should be kept and used for inversion"]
pub struct HueCdf {
    step_deg: f32,
    pub cdf: Vec<f32>, // normalized [0,1]
}

impl HueCdf {
    pub fn new<F>(samples: usize, mut weight: F) -> Self
    where
        F: FnMut(f32) -> f32,
    {
        #[allow(clippy::cast_precision_loss)]
        let step = 360.0 / samples as f32;
        let mut cdf = vec![0.0; samples + 1];
        let mut acc = 0.0_f64;
        for i in 0..samples {
            #[allow(clippy::cast_precision_loss)]
            let h = i as f32 * step; // left Riemann
            let w = f64::from(weight(h).max(1.0e-6));
            acc += w * f64::from(step);
            #[allow(clippy::cast_possible_truncation)]
            {
                cdf[i + 1] = acc as f32;
            }
        }
        let total = cdf[samples].max(1e-9);
        for x in &mut cdf {
            *x /= total;
        }
        Self {
            step_deg: step,
            cdf,
        }
    }

    #[inline]
    pub fn invert(&self, t: f32) -> f32 {
        let n = self.cdf.len() - 1;
        let (mut lo, mut hi) = (0usize, n);
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.cdf[mid] <= t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (t0, t1) = (self.cdf[lo], self.cdf[hi]);
        let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
        #[allow(clippy::cast_precision_loss)]
        let lo_f32 = lo as f32;
        ((lo_f32 + u) * self.step_deg) % 360.0
    }
}

#[inline]
pub fn weight(h_deg: f32, params: WarpParams) -> f32 {
    let h = h_deg.to_radians();
    let mu = params.mu_deg.to_radians();
    let vm = (params.kappa * ((h - mu).cos() - 1.0)).exp(); // peak == 1 at mu
    let w = params.strength.mul_add(-vm, 1.0);
    w.max(1.0e-6)
}
