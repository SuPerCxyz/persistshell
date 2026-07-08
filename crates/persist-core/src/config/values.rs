use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::de::{self, Visitor};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct ByteSize(u64);

impl ByteSize {
    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> u64 {
        self.0
    }
}

impl FromStr for ByteSize {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("size value cannot be empty".to_string());
        }

        let split_at = value
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(value.len());
        let (number, unit) = value.split_at(split_at);
        if number.is_empty() {
            return Err(format!("size value must start with a number: {value}"));
        }

        let number = number
            .parse::<u64>()
            .map_err(|error| format!("invalid size number {number}: {error}"))?;
        let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
            "" | "b" => 1,
            "k" | "kb" | "kib" => 1024,
            "m" | "mb" | "mib" => 1024 * 1024,
            "g" | "gb" | "gib" => 1024 * 1024 * 1024,
            "t" | "tb" | "tib" => 1024_u64.pow(4),
            other => return Err(format!("unsupported size unit: {other}")),
        };

        number
            .checked_mul(multiplier)
            .map(Self)
            .ok_or_else(|| format!("size value is too large: {value}"))
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0;
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;

        if bytes >= GB && bytes % GB == 0 {
            write!(formatter, "{}GB", bytes / GB)
        } else if bytes >= MB && bytes % MB == 0 {
            write!(formatter, "{}MB", bytes / MB)
        } else if bytes >= KB && bytes % KB == 0 {
            write!(formatter, "{}KB", bytes / KB)
        } else {
            write!(formatter, "{bytes}B")
        }
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(ByteSizeVisitor)
    }
}

struct ByteSizeVisitor;

impl Visitor<'_> for ByteSizeVisitor {
    type Value = ByteSize;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a byte size such as 512KB, 8MB, or an integer byte count")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(ByteSize::from_bytes(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        u64::try_from(value)
            .map(ByteSize::from_bytes)
            .map_err(|_| E::custom("byte size cannot be negative"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value.parse::<ByteSize>().map_err(E::custom)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct DurationValue(Duration);

impl DurationValue {
    pub const fn from_secs(seconds: u64) -> Self {
        Self(Duration::from_secs(seconds))
    }

    pub const fn from_millis(milliseconds: u64) -> Self {
        Self(Duration::from_millis(milliseconds))
    }

    pub const fn duration(self) -> Duration {
        self.0
    }
}

impl FromStr for DurationValue {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("duration value cannot be empty".to_string());
        }

        let split_at = value
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(value.len());
        let (number, unit) = value.split_at(split_at);
        if number.is_empty() {
            return Err(format!("duration value must start with a number: {value}"));
        }

        let number = number
            .parse::<u64>()
            .map_err(|error| format!("invalid duration number {number}: {error}"))?;
        match unit.trim().to_ascii_lowercase().as_str() {
            "" | "s" | "sec" | "secs" | "second" | "seconds" => {
                Ok(Self(Duration::from_secs(number)))
            }
            "ms" | "millisecond" | "milliseconds" => Ok(Self(Duration::from_millis(number))),
            "m" | "min" | "mins" | "minute" | "minutes" => number
                .checked_mul(60)
                .map(Duration::from_secs)
                .map(Self)
                .ok_or_else(|| format!("duration value is too large: {value}")),
            "h" | "hr" | "hrs" | "hour" | "hours" => number
                .checked_mul(60 * 60)
                .map(Duration::from_secs)
                .map(Self)
                .ok_or_else(|| format!("duration value is too large: {value}")),
            other => Err(format!("unsupported duration unit: {other}")),
        }
    }
}

impl fmt::Display for DurationValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let duration = self.0;
        let seconds = duration.as_secs();

        if duration.subsec_millis() != 0 {
            write!(formatter, "{}ms", duration.as_millis())
        } else if seconds >= 3600 && seconds % 3600 == 0 {
            write!(formatter, "{}h", seconds / 3600)
        } else if seconds >= 60 && seconds % 60 == 0 {
            write!(formatter, "{}m", seconds / 60)
        } else {
            write!(formatter, "{seconds}s")
        }
    }
}

impl<'de> Deserialize<'de> for DurationValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(DurationValueVisitor)
    }
}

struct DurationValueVisitor;

impl Visitor<'_> for DurationValueVisitor {
    type Value = DurationValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a duration such as 500ms, 3s, 10m, or an integer second count")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(DurationValue::from_secs(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        u64::try_from(value)
            .map(DurationValue::from_secs)
            .map_err(|_| E::custom("duration cannot be negative"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value.parse::<DurationValue>().map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_byte_size_units() {
        assert_eq!("512KB".parse::<ByteSize>().expect("parse").bytes(), 524288);
        assert_eq!("8MB".parse::<ByteSize>().expect("parse").bytes(), 8388608);
    }

    #[test]
    fn parses_duration_units() {
        assert_eq!(
            "10m"
                .parse::<DurationValue>()
                .expect("parse")
                .duration()
                .as_secs(),
            600
        );
        assert_eq!(
            "500ms"
                .parse::<DurationValue>()
                .expect("parse")
                .duration()
                .as_millis(),
            500
        );
    }
}
