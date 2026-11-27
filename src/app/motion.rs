//! Easing curves: how an animation moves from 0 to 1.

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// Slow, fast, slow.
pub(super) fn in_out_cubic(x: f32) -> f32 {
    let x = clamp01(x);
    if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 }
}

/// The next number of `rng` (xorshift64), seeded from the clock the first time (`rng` at 0).
pub(super) fn random(rng: &mut u64) -> u64 {
    if *rng == 0 {
        *rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9e37_79b9) | 1;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_go_from_0_to_1() {
        assert!(in_out_cubic(0.0).abs() < 1e-6);
        assert!((in_out_cubic(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(in_out_cubic(-3.0), in_out_cubic(0.0));
    }
}
