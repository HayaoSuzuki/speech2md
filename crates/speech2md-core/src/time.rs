use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(u64);

impl Timestamp {
    #[must_use]
    pub const fn from_millis(milliseconds: u64) -> Self {
        Self(milliseconds)
    }

    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeSpan {
    start: Timestamp,
    end: Timestamp,
}

impl TimeSpan {
    /// Creates an ordered half-open time span.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTimeSpan`] when `start` is after `end`.
    pub fn new(start: Timestamp, end: Timestamp) -> Result<Self, InvalidTimeSpan> {
        (start <= end)
            .then_some(Self { start, end })
            .ok_or(InvalidTimeSpan)
    }

    #[must_use]
    pub const fn start(self) -> Timestamp {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> Timestamp {
        self.end
    }

    #[must_use]
    pub const fn duration_ms(self) -> u64 {
        self.end.as_millis() - self.start.as_millis()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("time span start must not be after its end")]
pub struct InvalidTimeSpan;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Confidence(f32);

impl Confidence {
    /// Creates a probability-like confidence value.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidConfidence`] unless `value` is finite and in the
    /// inclusive range from zero to one.
    pub fn new(value: f32) -> Result<Self, InvalidConfidence> {
        (value.is_finite() && (0.0..=1.0).contains(&value))
            .then_some(Self(value))
            .ok_or(InvalidConfidence)
    }

    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("confidence must be a finite value between zero and one")]
pub struct InvalidConfidence;

#[cfg(test)]
mod tests {
    use super::{Confidence, TimeSpan, Timestamp};

    #[test]
    fn rejects_reversed_span() {
        assert!(TimeSpan::new(Timestamp::from_millis(20), Timestamp::from_millis(10)).is_err());
    }

    #[test]
    fn timestamp_preserves_milliseconds() {
        assert_eq!(Timestamp::from_millis(42).as_millis(), 42);
    }

    #[test]
    fn span_reports_its_duration() {
        let span = TimeSpan::new(Timestamp::from_millis(10), Timestamp::from_millis(42))
            .expect("ordered timestamps form a valid span");

        assert_eq!(span.duration_ms(), 32);
        assert_eq!(span.start().as_millis(), 10);
        assert_eq!(span.end().as_millis(), 42);
    }

    #[test]
    fn confidence_is_a_probability() {
        assert_eq!(
            Confidence::new(0.0).expect("lower bound").value().to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            Confidence::new(1.0).expect("upper bound").value().to_bits(),
            1.0_f32.to_bits()
        );
        assert!(Confidence::new(-0.1).is_err());
        assert!(Confidence::new(1.1).is_err());
        assert!(Confidence::new(f32::NAN).is_err());
        assert!(Confidence::new(f32::INFINITY).is_err());
    }
}
