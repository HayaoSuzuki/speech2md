use arbitrary::Arbitrary;
use yasumaro_core::{TimeSpan, Timestamp};

/// Mix densely overlapping short spans with full-width and near-overflow times.
#[derive(Arbitrary, Debug)]
pub struct SpanInput {
    pub start: u64,
    pub duration: u64,
    pub mode: u8,
}

impl SpanInput {
    pub fn span(&self) -> TimeSpan {
        let (start, duration) = match self.mode % 3 {
            0 => (self.start % 10_001, self.duration % 2_001),
            1 => (u64::MAX - self.start % 10_001, self.duration % 2_001),
            _ => (self.start, self.duration),
        };
        TimeSpan::new(
            Timestamp::from_millis(start),
            Timestamp::from_millis(start.saturating_add(duration)),
        )
        .expect("generated ordered span")
    }
}
