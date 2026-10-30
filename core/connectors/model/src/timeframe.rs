//! Bar resolution. Spelled the way OTW already stores it (`1m`, `4h`, `1d`, `1w`), plus
//! seconds and calendar months (`1M`). A month has no fixed length, so it never divides
//! anything and nothing divides it except another month count.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TfUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timeframe {
    pub n: u32,
    pub unit: TfUnit,
}

impl Timeframe {
    pub const M1: Timeframe = Timeframe { n: 1, unit: TfUnit::Minute };
    pub const M5: Timeframe = Timeframe { n: 5, unit: TfUnit::Minute };
    pub const M15: Timeframe = Timeframe { n: 15, unit: TfUnit::Minute };
    pub const M30: Timeframe = Timeframe { n: 30, unit: TfUnit::Minute };
    pub const H1: Timeframe = Timeframe { n: 1, unit: TfUnit::Hour };
    pub const H4: Timeframe = Timeframe { n: 4, unit: TfUnit::Hour };
    pub const D1: Timeframe = Timeframe { n: 1, unit: TfUnit::Day };
    pub const W1: Timeframe = Timeframe { n: 1, unit: TfUnit::Week };
    pub const MO1: Timeframe = Timeframe { n: 1, unit: TfUnit::Month };

    /// Length in seconds, `None` for months.
    pub fn fixed_secs(self) -> Option<i64> {
        let unit: i64 = match self.unit {
            TfUnit::Second => 1,
            TfUnit::Minute => 60,
            TfUnit::Hour => 3_600,
            TfUnit::Day => 86_400,
            TfUnit::Week => 604_800,
            TfUnit::Month => return None,
        };
        Some(unit * i64::from(self.n))
    }

    /// Whether whole bars of `self` tile one bar of `larger` exactly (`5m` into `1h`).
    pub fn divides(self, larger: Timeframe) -> bool {
        match (self.unit, larger.unit) {
            (TfUnit::Month, TfUnit::Month) => self.n > 0 && larger.n.is_multiple_of(self.n),
            // A day tiles a month; a week does not.
            (TfUnit::Day, TfUnit::Month) => self.n == 1,
            (_, TfUnit::Month) | (TfUnit::Month, _) => false,
            _ => match (self.fixed_secs(), larger.fixed_secs()) {
                (Some(a), Some(b)) => a > 0 && b >= a && b % a == 0,
                _ => false,
            },
        }
    }

    /// Session-shaped: a period the venue delimits (a day, a week, a month), not an epoch
    /// bucket. Mirrors OTW's `hub::anchored` rule (`>= 1d`).
    pub fn is_session_shaped(self) -> bool {
        matches!(self.unit, TfUnit::Day | TfUnit::Week | TfUnit::Month)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown timeframe `{0}`")]
pub struct BadTimeframe(pub String);

impl FromStr for Timeframe {
    type Err = BadTimeframe;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let split = s.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| BadTimeframe(s.into()))?;
        let (num, unit) = s.split_at(split);
        let n: u32 = num.parse().map_err(|_| BadTimeframe(s.into()))?;
        if n == 0 {
            return Err(BadTimeframe(s.into()));
        }
        let unit = match unit {
            "s" => TfUnit::Second,
            "m" => TfUnit::Minute,
            "h" => TfUnit::Hour,
            "d" => TfUnit::Day,
            "w" => TfUnit::Week,
            "M" => TfUnit::Month,
            _ => return Err(BadTimeframe(s.into())),
        };
        Ok(Timeframe { n, unit })
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let u = match self.unit {
            TfUnit::Second => "s",
            TfUnit::Minute => "m",
            TfUnit::Hour => "h",
            TfUnit::Day => "d",
            TfUnit::Week => "w",
            TfUnit::Month => "M",
        };
        write!(f, "{}{}", self.n, u)
    }
}

impl Serialize for Timeframe {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timeframe {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_otw_spellings() {
        for s in ["1s", "1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M", "3M"] {
            assert_eq!(s.parse::<Timeframe>().unwrap().to_string(), s);
        }
        for bad in ["", "m", "0m", "1x", "1mo", "-1m"] {
            assert!(bad.parse::<Timeframe>().is_err(), "{bad}");
        }
    }

    #[test]
    fn divisibility_holds_only_where_bars_tile() {
        assert!(Timeframe::M5.divides(Timeframe::H1));
        assert!(Timeframe::H1.divides(Timeframe::D1));
        assert!(Timeframe::D1.divides(Timeframe::W1));
        assert!(Timeframe::D1.divides(Timeframe::MO1));
        assert!(!Timeframe::W1.divides(Timeframe::MO1));
        assert!(!Timeframe::H4.divides(Timeframe::H1));
        let m7: Timeframe = "7m".parse().unwrap();
        assert!(!m7.divides(Timeframe::H1));
        assert!(Timeframe::MO1.divides("3M".parse().unwrap()));
    }
}
