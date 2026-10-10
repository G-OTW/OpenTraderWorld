//! Special functions and distribution tails the statistical tests need.
//!
//! Hand-rolled on purpose, like the rest of the quant engine: the textbook algorithms below
//! (Lanczos gamma, Numerical Recipes incomplete gamma/beta) are accurate to ~1e-12, far below
//! the precision of any statistic estimated from market data, and each one is checked against
//! scipy on known points in the tests at the bottom.

use std::f64::consts::PI;

/// ln Γ(x) for x > 0 (Lanczos, g = 7, n = 9).
pub fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection keeps the series in its accurate range.
        return (PI / (PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + G + 0.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// Regularized lower incomplete gamma P(a, x).
pub fn gamma_p(a: f64, x: f64) -> f64 {
    if x <= 0.0 || a <= 0.0 {
        return 0.0;
    }
    if x < a + 1.0 {
        // Series.
        let mut ap = a;
        let mut sum = 1.0 / a;
        let mut del = sum;
        for _ in 0..1000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-16 {
                break;
            }
        }
        (sum.ln() - x + a * x.ln() - ln_gamma(a)).exp()
    } else {
        1.0 - gamma_q_cf(a, x)
    }
}

/// Regularized upper incomplete gamma Q(a, x) = 1 - P(a, x).
pub fn gamma_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 || a <= 0.0 {
        return 1.0;
    }
    if x < a + 1.0 {
        1.0 - gamma_p(a, x)
    } else {
        gamma_q_cf(a, x)
    }
}

/// Continued fraction for Q(a, x) (modified Lentz), valid for x >= a + 1.
fn gamma_q_cf(a: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / TINY;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..1000 {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < TINY {
            d = TINY;
        }
        c = b + an / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            break;
        }
    }
    ((-x + a * x.ln() - ln_gamma(a)).exp() * h).clamp(0.0, 1.0)
}

/// Regularized incomplete beta I_x(a, b).
pub fn beta_inc(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_front = ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    let front = ln_front.exp();
    // The continued fraction converges fast on this side of the mean; use symmetry otherwise.
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(a, b, x) / a
    } else {
        1.0 - front * beta_cf(b, a, 1.0 - x) / b
    }
}

fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..3000 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            break;
        }
    }
    h
}

/// erfc(x) without the cancellation 1 - erf(x) suffers in the far tail.
pub fn erfc(x: f64) -> f64 {
    if x < 0.0 {
        2.0 - gamma_q(0.5, x * x)
    } else {
        gamma_q(0.5, x * x)
    }
}

/// Standard normal CDF.
pub fn norm_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

/// Standard normal survival 1 - Φ(x), accurate in the upper tail.
#[cfg(test)]
pub fn norm_sf(x: f64) -> f64 {
    0.5 * erfc(x / std::f64::consts::SQRT_2)
}

pub fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * PI).sqrt()
}

/// Standard normal quantile: Acklam's rational approximation polished by one Halley step
/// against the exact CDF, which takes it from ~1e-9 to machine precision.
pub fn norm_ppf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    let x = super::norm_ppf(p);
    let e = norm_cdf(x) - p;
    let u = e * (2.0 * PI).sqrt() * (x * x / 2.0).exp();
    x - u / (1.0 + x * u / 2.0)
}

/// Student t CDF with `df` degrees of freedom.
pub fn t_cdf(t: f64, df: f64) -> f64 {
    if !t.is_finite() {
        return if t > 0.0 { 1.0 } else { 0.0 };
    }
    let x = df / (df + t * t);
    let tail = 0.5 * beta_inc(df / 2.0, 0.5, x);
    if t > 0.0 {
        1.0 - tail
    } else {
        tail
    }
}

/// Two-sided p-value of a t statistic.
pub fn t_two_sided(t: f64, df: f64) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    beta_inc(df / 2.0, 0.5, df / (df + t * t)).clamp(0.0, 1.0)
}

/// Student t quantile by bisection on the CDF (only used for confidence intervals).
pub fn t_ppf(p: f64, df: f64) -> f64 {
    let (mut lo, mut hi) = (-1e3, 1e3);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if t_cdf(mid, df) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Upper tail of the F distribution, P(F > f).
pub fn f_sf(f: f64, d1: f64, d2: f64) -> f64 {
    if f <= 0.0 {
        return 1.0;
    }
    beta_inc(d2 / 2.0, d1 / 2.0, d2 / (d2 + d1 * f)).clamp(0.0, 1.0)
}

/// Upper tail of the chi-square distribution, P(X > x).
pub fn chi2_sf(x: f64, k: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    gamma_q(k / 2.0, x / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    // Reference values from scipy.special / scipy.stats (1.16).
    #[test]
    fn gamma_family_matches_scipy() {
        assert!(close(ln_gamma(0.5), 0.572_364_942_924_700_1, 1e-13));
        assert!(close(ln_gamma(10.0), 12.801_827_480_081_469, 1e-13));
        assert!(close(gamma_p(2.5, 1.7), 0.361_430_076_896_204_8, 1e-12));
        assert!(close(gamma_q(3.0, 7.5), 0.020_256_715_056_664_397, 1e-12));
    }

    #[test]
    fn normal_matches_scipy() {
        assert!(close(norm_cdf(1.96), 0.975_002_104_851_780_1, 1e-14));
        assert!(close(norm_sf(6.0), 9.865_876_450_376_946e-10, 1e-10));
        assert!(close(norm_ppf(0.975), 1.959_963_984_540_054, 1e-13));
        assert!(close(norm_ppf(1e-6), -4.753_424_308_822_899, 1e-12));
    }

    #[test]
    fn t_f_chi2_match_scipy() {
        assert!(close(t_cdf(2.0, 5.0), 0.949_030_260_585_070_6, 1e-12));
        assert!(close(t_two_sided(-2.5, 30.0), 0.018_115_649_068_066_69, 1e-11));
        assert!(close(t_ppf(0.975, 10.0), 2.228_138_851_986_274, 1e-9));
        assert!(close(f_sf(3.2, 3.0, 40.0), 0.033_421_180_722_892_93, 1e-11));
        assert!(close(chi2_sf(5.991_464_547_107_979, 2.0), 0.05, 1e-12));
        assert!(close(beta_inc(2.0, 3.0, 0.4), 0.5248, 1e-13));
    }
}
