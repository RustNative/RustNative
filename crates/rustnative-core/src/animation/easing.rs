//! Easing curves, including springs.
//!
//! Everything here maps elapsed time to *progress* — `0.0` at the start,
//! `1.0` at the end — which [`super::AnimatedValue::interpolate`] then
//! applies to the values being animated. Progress is allowed to leave that
//! range: a spring overshoots and settles back, and so do some hand-authored
//! bezier curves.

use std::time::Duration;

use crate::input::Scalar;

/// How an animation moves between its endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Constant speed.
    Linear,
    /// Starts slowly.
    EaseIn,
    /// Ends slowly.
    EaseOut,
    /// Starts and ends slowly — the default, and what most interface
    /// motion wants.
    EaseInOut,
    /// An arbitrary cubic bezier through `(0,0)` and `(1,1)`, given by its
    /// two control points, exactly as CSS' `cubic-bezier()`.
    CubicBezier {
        /// First control point's x, within `0.0..=1.0`.
        x1: Scalar,
        /// First control point's y.
        y1: Scalar,
        /// Second control point's x, within `0.0..=1.0`.
        x2: Scalar,
        /// Second control point's y.
        y2: Scalar,
    },
    /// A damped spring, which has no duration: it runs until it settles.
    Spring {
        /// How hard it pulls toward the target.
        stiffness: Scalar,
        /// How strongly motion is damped. At the critical value
        /// (`2 * sqrt(stiffness * mass)`) it settles without overshooting.
        damping: Scalar,
        /// The mass being moved.
        mass: Scalar,
    },
}

impl Easing {
    /// A spring, with values clamped to physically meaningful ones
    /// (positive stiffness and mass, non-negative damping).
    #[must_use]
    pub fn spring(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self::Spring {
            stiffness: Scalar::new(stiffness.max(f32::EPSILON)),
            damping: Scalar::new(damping.max(0.0)),
            mass: Scalar::new(mass.max(f32::EPSILON)),
        }
    }

    /// A cubic bezier, with its control-point x values clamped to
    /// `0.0..=1.0` as the curve requires to stay a function of time.
    #[must_use]
    pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self::CubicBezier {
            x1: Scalar::new(x1.clamp(0.0, 1.0)),
            y1: Scalar::new(y1),
            x2: Scalar::new(x2.clamp(0.0, 1.0)),
            y2: Scalar::new(y2),
        }
    }

    /// Whether this curve settles on its own rather than over a duration.
    #[must_use]
    pub const fn is_spring(&self) -> bool {
        matches!(self, Self::Spring { .. })
    }

    /// Progress at `fraction` of the way through a timed animation.
    ///
    /// `fraction` is clamped to `0.0..=1.0`; a spring ignores it (see
    /// [`Self::spring_progress`]) and is treated as linear here so a caller
    /// that mixes the two still gets sensible endpoints.
    #[must_use]
    pub fn progress(&self, fraction: f32) -> f32 {
        let t = fraction.clamp(0.0, 1.0);
        match *self {
            Self::Linear | Self::Spring { .. } => t,
            // The classic pair: quadratic in, quadratic out.
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let shifted = t - 1.0;
                    1.0 - 2.0 * shifted * shifted
                }
            }
            Self::CubicBezier { x1, y1, x2, y2 } => {
                bezier(t, x1.get(), y1.get(), x2.get(), y2.get())
            }
        }
    }

    /// A spring's progress after `elapsed`, and whether it has settled.
    ///
    /// Returns `(1.0, true)` for a non-spring curve, which has no physics
    /// to simulate.
    #[must_use]
    pub fn spring_progress(&self, elapsed: Duration) -> (f32, bool) {
        // Close enough to the target, and slow enough, to stop simulating:
        // a tenth of a percent of the distance, which is sub-pixel for any
        // real animated value.
        const SETTLED: f32 = 0.001;

        let Self::Spring { stiffness, damping, mass } = *self else {
            return (1.0, true);
        };
        let (stiffness, damping, mass) = (stiffness.get(), damping.get(), mass.get());
        let time = elapsed.as_secs_f32();
        // The standard damped-oscillator parameters: undamped angular
        // frequency, and the damping ratio that decides which regime the
        // spring is in.
        let omega0 = (stiffness / mass).sqrt();
        let zeta = damping / (2.0 * (stiffness * mass).sqrt());
        // Displacement from the target, starting at -1 (fully away) with no
        // initial velocity; progress is 1 + that displacement.
        let (displacement, velocity) = if zeta < 1.0 {
            // Underdamped: oscillates toward the target.
            let damped_frequency = omega0 * (1.0 - zeta * zeta).sqrt();
            let decay = (-zeta * omega0 * time).exp();
            let (sin, cos) = (damped_frequency * time).sin_cos();
            (
                -decay * (cos + (zeta * omega0 / damped_frequency) * sin),
                decay * (omega0 * omega0 / damped_frequency) * sin,
            )
        } else if (zeta - 1.0).abs() < f32::EPSILON {
            // Critically damped: the fastest approach without overshoot.
            let decay = (-omega0 * time).exp();
            (-decay * (1.0 + omega0 * time), decay * omega0 * omega0 * time)
        } else {
            // Overdamped: two real roots, no oscillation.
            let root = omega0 * (zeta * zeta - 1.0).sqrt();
            let (fast, slow) = (-zeta * omega0 + root, -zeta * omega0 - root);
            let (fast_decay, slow_decay) = ((fast * time).exp(), (slow * time).exp());
            // Solved from x(0) = -1 (fully away from the target) and
            // v(0) = 0, so the pair already sums to -1 and needs no
            // further negation.
            let fast_weight = -slow / (slow - fast);
            let slow_weight = fast / (slow - fast);
            (
                fast_weight * fast_decay + slow_weight * slow_decay,
                fast_weight * fast * fast_decay + slow_weight * slow * slow_decay,
            )
        };
        let progress = 1.0 + displacement;
        let settled = displacement.abs() < SETTLED && velocity.abs() < SETTLED;
        (progress, settled)
    }
}

/// Evaluates a cubic bezier's `y` at the `x` given by `t`, the way CSS
/// timing functions do: the curve is parameterized by its own variable, so
/// finding `y` means first solving for the parameter that produces this
/// `x`.
fn bezier(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    /// Newton-Raphson converges in a few steps for well-behaved curves.
    const NEWTON_ITERATIONS: usize = 8;
    /// Close enough that the remaining error is far below one pixel.
    const EPSILON: f32 = 1e-6;
    /// Bisection is the fallback where the derivative is too flat for
    /// Newton to make progress; 30 halvings resolve far past `f32`.
    const BISECTION_ITERATIONS: usize = 30;

    let curve = |t: f32, a: f32, b: f32| {
        // The cubic with endpoints fixed at 0 and 1, expanded.
        let inverse = 1.0 - t;
        3.0 * inverse * inverse * t * a + 3.0 * inverse * t * t * b + t * t * t
    };
    let slope = |t: f32, a: f32, b: f32| {
        let inverse = 1.0 - t;
        3.0 * inverse * inverse * a + 6.0 * inverse * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };

    let mut t = x;
    for _ in 0..NEWTON_ITERATIONS {
        let error = curve(t, x1, x2) - x;
        if error.abs() < EPSILON {
            return curve(t, y1, y2);
        }
        let derivative = slope(t, x1, x2);
        if derivative.abs() < EPSILON {
            break;
        }
        t -= error / derivative;
    }

    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    let mut t = x.clamp(0.0, 1.0);
    for _ in 0..BISECTION_ITERATIONS {
        let value = curve(t, x1, x2);
        if (value - x).abs() < EPSILON {
            break;
        }
        if value < x {
            low = t;
        } else {
            high = t;
        }
        t = f32::midpoint(low, high);
    }
    curve(t, y1, y2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVES: [Easing; 5] = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::CubicBezier {
            x1: Scalar::ZERO,
            y1: Scalar::ZERO,
            x2: Scalar::ZERO,
            y2: Scalar::ONE,
        },
    ];

    #[test]
    fn every_timed_curve_starts_at_zero_and_ends_at_one() {
        for curve in CURVES {
            assert!(curve.progress(0.0).abs() < 1e-5, "{curve:?} must start at 0");
            assert!((curve.progress(1.0) - 1.0).abs() < 1e-5, "{curve:?} must end at 1");
            // Outside the range, time is clamped rather than extrapolated.
            assert!(curve.progress(-1.0).abs() < 1e-5);
            assert!((curve.progress(2.0) - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn the_standard_curves_never_move_backwards() {
        for curve in CURVES {
            let mut previous = f32::NEG_INFINITY;
            for step in 0..=100 {
                #[allow(clippy::cast_precision_loss, reason = "0..=100 is exact in f32")]
                let value = curve.progress(step as f32 / 100.0);
                assert!(value >= previous - 1e-5, "{curve:?} went backwards at {step}");
                previous = value;
            }
        }
    }

    #[test]
    fn ease_in_starts_slower_than_linear_and_ease_out_faster() {
        assert!(Easing::EaseIn.progress(0.25) < 0.25);
        assert!(Easing::EaseOut.progress(0.25) > 0.25);
        assert!((Easing::EaseInOut.progress(0.5) - 0.5).abs() < 1e-5, "symmetric at the midpoint");
    }

    #[test]
    fn a_cubic_bezier_matches_the_linear_curve_when_its_controls_are_linear() {
        let linear = Easing::cubic_bezier(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
        for step in 0..=20 {
            #[allow(clippy::cast_precision_loss, reason = "0..=20 is exact in f32")]
            let t = step as f32 / 20.0;
            assert!((linear.progress(t) - t).abs() < 1e-3, "at {t}");
        }
    }

    #[test]
    fn an_underdamped_spring_overshoots_and_then_settles_at_the_target() {
        let spring = Easing::spring(180.0, 12.0, 1.0);
        let mut overshot = false;
        let mut settled_at = None;
        for step in 0..400 {
            let (progress, settled) = spring.spring_progress(Duration::from_millis(step * 5));
            if progress > 1.001 {
                overshot = true;
            }
            if settled {
                settled_at = Some(step);
                assert!((progress - 1.0).abs() < 0.01, "settling means being at the target");
                break;
            }
        }
        assert!(overshot, "a lightly damped spring overshoots");
        assert!(settled_at.is_some(), "and then settles");
    }

    #[test]
    fn a_critically_damped_spring_never_overshoots() {
        // damping == 2 * sqrt(k * m) is the critical value.
        let spring = Easing::spring(100.0, 20.0, 1.0);
        for step in 0..400 {
            let (progress, settled) = spring.spring_progress(Duration::from_millis(step * 5));
            assert!(progress <= 1.0 + 1e-4, "overshot at step {step}: {progress}");
            if settled {
                return;
            }
        }
        panic!("a critically damped spring must settle");
    }

    #[test]
    fn an_overdamped_spring_approaches_slowly_without_oscillating() {
        let spring = Easing::spring(100.0, 60.0, 1.0);
        let mut previous = 0.0;
        // Heavily overdamped: this one really does take a few seconds to
        // creep the last thousandth of the way.
        for step in 0..2_000 {
            let (progress, settled) = spring.spring_progress(Duration::from_millis(step * 5));
            assert!(progress <= 1.0 + 1e-4);
            assert!(progress >= previous - 1e-4, "overdamped motion is monotone");
            previous = progress;
            if settled {
                return;
            }
        }
        panic!("an overdamped spring must settle");
    }

    #[test]
    fn a_spring_starts_at_rest() {
        let spring = Easing::spring(180.0, 12.0, 1.0);
        let (progress, settled) = spring.spring_progress(Duration::ZERO);
        assert!(progress.abs() < 1e-5);
        assert!(!settled);
    }
}
