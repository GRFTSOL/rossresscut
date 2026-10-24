//! Integer-based media time types.
//!
//! Media times are stored as whole microseconds. This keeps the domain model
//! free of floating point and still permits exact conversion to and from
//! FFmpeg's rational time bases. The canonical internal time base is
//! [`TimeBase::MICROSECONDS`]; UI formatting truncates to milliseconds.

use std::fmt;

use serde::Serialize;

const MICROS_PER_SECOND: i64 = 1_000_000;

/// Rational time base: one tick equals `numerator / denominator` seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TimeBase {
    numerator: u32,
    denominator: u32,
}

impl TimeBase {
    /// The canonical time base of the domain model.
    pub const MICROSECONDS: Self = Self {
        numerator: 1,
        denominator: 1_000_000,
    };

    /// Creates a time base from a numerator and denominator.
    ///
    /// Returns `None` if either part is zero, because such a time base cannot
    /// convert ticks to time.
    pub const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            None
        } else {
            Some(Self {
                numerator,
                denominator,
            })
        }
    }

    /// Returns the numerator of the time base.
    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    /// Returns the denominator of the time base.
    pub const fn denominator(self) -> u32 {
        self.denominator
    }

    /// Parses an FFmpeg-style `numerator/denominator` string.
    pub fn parse(value: &str) -> Option<Self> {
        let (numerator, denominator) = value.split_once('/')?;
        Self::new(
            numerator.trim().parse().ok()?,
            denominator.trim().parse().ok()?,
        )
    }

    /// Converts `ticks` in this time base to microseconds.
    ///
    /// The result is rounded to the nearest microsecond, with ties away from
    /// zero. Returns `None` on overflow.
    pub fn ticks_to_micros(self, ticks: i64) -> Option<i64> {
        let numerator = i128::from(ticks)
            .checked_mul(i128::from(self.numerator))?
            .checked_mul(i128::from(MICROS_PER_SECOND))?;
        let rounded = div_round_half_away_from_zero(numerator, i128::from(self.denominator))?;
        i64::try_from(rounded).ok()
    }

    /// Converts microseconds to ticks in this time base.
    ///
    /// The result is rounded to the nearest tick, with ties away from zero.
    /// Returns `None` on overflow.
    pub fn micros_to_ticks(self, micros: i64) -> Option<i64> {
        let numerator = i128::from(micros).checked_mul(i128::from(self.denominator))?;
        let denominator = i128::from(self.numerator).checked_mul(i128::from(MICROS_PER_SECOND))?;
        let rounded = div_round_half_away_from_zero(numerator, denominator)?;
        i64::try_from(rounded).ok()
    }
}

impl fmt::Display for TimeBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.numerator, self.denominator)
    }
}

/// A point on a media timeline, stored as microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Timestamp {
    micros: i64,
}

impl Timestamp {
    /// The zero timestamp.
    pub const ZERO: Self = Self { micros: 0 };

    /// Creates a timestamp from microseconds.
    pub const fn from_micros(micros: i64) -> Self {
        Self { micros }
    }

    /// Returns the timestamp in microseconds.
    pub const fn micros(self) -> i64 {
        self.micros
    }

    /// Parses a decimal-seconds string as produced by FFprobe.
    ///
    /// Returns `None` for `N/A`, malformed input, or overflow.
    pub fn from_seconds(value: &str) -> Option<Self> {
        parse_decimal_seconds(value).map(Self::from_micros)
    }

    /// Creates a timestamp from ticks in `time_base`.
    pub fn from_ticks(ticks: i64, time_base: TimeBase) -> Option<Self> {
        time_base.ticks_to_micros(ticks).map(Self::from_micros)
    }

    /// Converts this timestamp to ticks in `time_base`.
    pub fn to_ticks(self, time_base: TimeBase) -> Option<i64> {
        time_base.micros_to_ticks(self.micros)
    }

    /// Adds a duration, returning `None` on overflow.
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let duration_micros = i64::try_from(duration.micros).ok()?;
        self.micros
            .checked_add(duration_micros)
            .map(Self::from_micros)
    }

    /// Subtracts a duration, returning `None` on overflow.
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        let duration_micros = i64::try_from(duration.micros).ok()?;
        self.micros
            .checked_sub(duration_micros)
            .map(Self::from_micros)
    }

    /// Returns the non-negative duration from `earlier` to this timestamp.
    ///
    /// Returns `None` when `earlier` is after this timestamp.
    pub fn duration_since(self, earlier: Self) -> Option<Duration> {
        let difference = self.micros.checked_sub(earlier.micros)?;
        u64::try_from(difference).ok().map(Duration::from_micros)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.micros.is_negative() {
            formatter.write_str("-")?;
        }
        formatter.write_str(&format_clock(self.micros.unsigned_abs()))
    }
}

/// A non-negative length of media time, stored as microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Duration {
    micros: u64,
}

impl Duration {
    /// The zero duration.
    pub const ZERO: Self = Self { micros: 0 };

    /// Creates a duration from microseconds.
    pub const fn from_micros(micros: u64) -> Self {
        Self { micros }
    }

    /// Returns the duration in microseconds.
    pub const fn micros(self) -> u64 {
        self.micros
    }

    /// Parses a non-negative decimal-seconds string as produced by FFprobe.
    ///
    /// Returns `None` for `N/A`, malformed input, negative values, or overflow.
    pub fn from_seconds(value: &str) -> Option<Self> {
        let micros = parse_decimal_seconds(value)?;
        u64::try_from(micros).ok().map(Self::from_micros)
    }

    /// Creates a duration from ticks in `time_base`.
    pub fn from_ticks(ticks: i64, time_base: TimeBase) -> Option<Self> {
        let micros = time_base.ticks_to_micros(ticks)?;
        u64::try_from(micros).ok().map(Self::from_micros)
    }

    /// Converts this duration to ticks in `time_base`.
    pub fn to_ticks(self, time_base: TimeBase) -> Option<i64> {
        let micros = i64::try_from(self.micros).ok()?;
        time_base.micros_to_ticks(micros)
    }

    /// Adds another duration, returning `None` on overflow.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.micros.checked_add(other.micros).map(Self::from_micros)
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&format_clock(self.micros))
    }
}

/// Frame rate expressed as a rational number of frames per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct FrameRate {
    numerator: u32,
    denominator: u32,
}

impl FrameRate {
    /// Creates a frame rate from a numerator and denominator.
    ///
    /// Returns `None` if either part is zero.
    pub const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            None
        } else {
            Some(Self {
                numerator,
                denominator,
            })
        }
    }

    /// Parses an FFmpeg-style `numerator/denominator` frame rate.
    ///
    /// `0/0` and other zero-valued rates yield `None`.
    pub fn parse(value: &str) -> Option<Self> {
        let (numerator, denominator) = value.split_once('/')?;
        Self::new(
            numerator.trim().parse().ok()?,
            denominator.trim().parse().ok()?,
        )
    }

    /// Returns the numerator of the frame rate.
    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    /// Returns the denominator of the frame rate.
    pub const fn denominator(self) -> u32 {
        self.denominator
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.numerator, self.denominator)
    }
}

fn div_round_half_away_from_zero(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator <= 0 {
        return None;
    }
    let denominator = denominator.unsigned_abs();
    let magnitude = numerator.unsigned_abs();
    let quotient = magnitude.checked_add(denominator / 2)? / denominator;
    let quotient = i128::try_from(quotient).ok()?;
    Some(if numerator.is_negative() {
        -quotient
    } else {
        quotient
    })
}

fn parse_decimal_seconds(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("N/A") {
        return None;
    }

    let (negative, unsigned) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };

    let (whole_text, fraction_text) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (unsigned, ""),
    };

    if whole_text.is_empty() && fraction_text.is_empty() {
        return None;
    }

    let whole: i64 = if whole_text.is_empty() {
        0
    } else {
        whole_text.parse().ok()?
    };

    let mut fraction_micros = 0_i64;
    let mut place = 100_000_i64;
    let mut rounding_digit = 0_i64;
    for (index, character) in fraction_text.chars().enumerate() {
        let digit = i64::from(character.to_digit(10)?);
        match index {
            0..=5 => {
                fraction_micros += digit * place;
                place /= 10;
            }
            6 => rounding_digit = digit,
            _ => {}
        }
    }
    if rounding_digit >= 5 {
        fraction_micros = fraction_micros.checked_add(1)?;
    }

    let magnitude = whole
        .checked_mul(MICROS_PER_SECOND)?
        .checked_add(fraction_micros)?;
    Some(if negative { -magnitude } else { magnitude })
}

fn format_clock(micros: u64) -> String {
    let milliseconds = micros / 1_000;
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    let milliseconds = milliseconds % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{milliseconds:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_seconds_exactly() {
        assert_eq!(
            Timestamp::from_seconds("1.500000"),
            Some(Timestamp::from_micros(1_500_000))
        );
        assert_eq!(
            Timestamp::from_seconds("-0.250000"),
            Some(Timestamp::from_micros(-250_000))
        );
        assert_eq!(
            Timestamp::from_seconds("5"),
            Some(Timestamp::from_micros(5_000_000))
        );
        assert_eq!(
            Timestamp::from_seconds(".5"),
            Some(Timestamp::from_micros(500_000))
        );
        assert_eq!(
            Timestamp::from_seconds("-1.5"),
            Some(Timestamp::from_micros(-1_500_000))
        );
    }

    #[test]
    fn rejects_unavailable_or_malformed_seconds() {
        assert_eq!(Timestamp::from_seconds("N/A"), None);
        assert_eq!(Timestamp::from_seconds("not-a-number"), None);
        assert_eq!(Timestamp::from_seconds("1.2.3"), None);
        assert_eq!(Timestamp::from_seconds("-"), None);
        assert_eq!(Duration::from_seconds("-1.0"), None);
    }

    #[test]
    fn rounds_sub_microsecond_digits() {
        assert_eq!(Timestamp::from_seconds("0.0000004"), Some(Timestamp::ZERO));
        assert_eq!(
            Timestamp::from_seconds("0.0000005"),
            Some(Timestamp::from_micros(1))
        );
    }

    #[test]
    fn converts_between_ticks_and_microseconds() {
        let time_base = TimeBase::parse("1/90000").expect("test time base is valid");

        assert_eq!(time_base.ticks_to_micros(90_000), Some(1_000_000));
        assert_eq!(time_base.micros_to_ticks(1_000_000), Some(90_000));
        assert_eq!(
            TimeBase::MICROSECONDS.ticks_to_micros(1_234_567),
            Some(1_234_567)
        );
        assert_eq!(
            TimeBase::MICROSECONDS.micros_to_ticks(1_234_567),
            Some(1_234_567)
        );
    }

    #[test]
    fn rejects_zero_time_bases_and_frame_rates() {
        assert_eq!(TimeBase::new(1, 0), None);
        assert_eq!(TimeBase::new(0, 1), None);
        assert_eq!(FrameRate::parse("0/0"), None);
        assert_eq!(FrameRate::parse("30000/1001"), FrameRate::new(30000, 1001));
    }

    #[test]
    fn formats_timestamps_and_durations_as_clock_time() {
        assert_eq!(Timestamp::ZERO.to_string(), "00:00:00.000");
        assert_eq!(
            Timestamp::from_micros(3_661_001_000).to_string(),
            "01:01:01.001"
        );
        assert_eq!(
            Timestamp::from_micros(-500_000).to_string(),
            "-00:00:00.500"
        );
        assert_eq!(Duration::from_micros(1_500_000).to_string(), "00:00:01.500");
    }

    #[test]
    fn timestamp_arithmetic_checks_bounds() {
        let timestamp = Timestamp::from_micros(2_000_000);
        let duration = Duration::from_micros(500_000);

        assert_eq!(
            timestamp.checked_add(duration),
            Some(Timestamp::from_micros(2_500_000))
        );
        assert_eq!(
            timestamp.checked_sub(duration),
            Some(Timestamp::from_micros(1_500_000))
        );
        assert_eq!(
            timestamp.duration_since(Timestamp::from_micros(500_000)),
            Some(Duration::from_micros(1_500_000))
        );
        assert_eq!(
            timestamp.duration_since(Timestamp::from_micros(3_000_000)),
            None
        );
    }
}
