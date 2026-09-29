//! Easing curves: how an animation moves from 0 to 1.

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// Fast, then slowing down.
pub(super) fn out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - clamp01(x)).powi(3)
}

/// Slow, fast, slow.
pub(super) fn in_out_cubic(x: f32) -> f32 {
    let x = clamp01(x);
    if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 }
}

/// A little past the end, then back: a landing.
pub(super) fn out_back(x: f32) -> f32 {
    let x = clamp01(x);
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

/// How far an animation starting at `start` and lasting `length` seconds is at `now`, eased.
pub(super) fn phase(now: f64, start: f64, length: f64, ease: fn(f32) -> f32) -> f32 {
    ease(((now - start) / length) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_go_from_0_to_1() {
        for ease in [out_cubic, in_out_cubic, out_back] {
            assert!(ease(0.0).abs() < 1e-6);
            assert!((ease(1.0) - 1.0).abs() < 1e-6);
            assert_eq!(ease(-3.0), ease(0.0));
        }
        // The landing overshoots.
        assert!((0..100).any(|k| out_back(k as f32 / 100.0) > 1.0));
    }
}
