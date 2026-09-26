//! Totally ordered scalars.

/// A finite `f32` with total equality, so input payloads that are
/// inherently fractional (pen pressure, pinch scale, stick deflection) can
/// live inside `rustnative_core::Event`, which is `Eq`.
///
/// `NaN` is not representable: [`Scalar::new`] maps it to `0.0`, and `-0.0`
/// is normalized to `0.0`, which is what makes bitwise equality a lawful
/// `Eq`. Platform input APIs never legitimately produce `NaN`; one arriving
/// anyway is a driver bug this type absorbs rather than propagates.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scalar(f32);

impl Scalar {
    /// `0.0`.
    pub const ZERO: Self = Self(0.0);
    /// `1.0`.
    pub const ONE: Self = Self(1.0);

    /// Wraps `value`, mapping `NaN` and `-0.0` to `0.0`.
    #[must_use]
    pub fn new(value: f32) -> Self {
        if value.is_nan() || value == 0.0 { Self(0.0) } else { Self(value) }
    }

    /// The wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl PartialEq for Scalar {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Scalar {}

impl core::hash::Hash for Scalar {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl From<f32> for Scalar {
    fn from(value: f32) -> Self {
        Self::new(value)
    }
}
