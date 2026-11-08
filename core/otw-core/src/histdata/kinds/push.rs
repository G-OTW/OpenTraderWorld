//! The `push` kind: bars or ticks the user's own script POSTs to a webhook.
//!
//! Nothing is fetched. A webhook endpoint bound to a connector of this kind (target
//! `histdata`, see `webhooks_inbound`) receives `{"symbol", "timeframe", "bars": [...]}` or
//! `{"symbol", "tick": {"ts", "price", "size"}}`. Bars are read with the same column mapping
//! as the `http` kind ([`super::http::Mapping`]) and written to the dataset of their
//! coordinates; both shapes are broadcast to the live charts watching them. This is the
//! escape hatch for every source the `http` kind cannot read (signed requests, OAuth, cursor
//! paging, protobuf): the script handles it in any language and posts rows.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::http::{self, Mapping, TsUnit};
use super::value::{Obs, ValueMapping};
use crate::histdata::caps::CapabilitySpec;
use crate::histdata::{Capability, Chunk, ConfigField, Connector, SymbolHit, SUPPORTED_TIMEFRAMES};

/// Kind id: what the create form posts, and the prefix of every minted provider id.
pub const KIND: &str = "push";

/// Bars one POST may carry. The body is capped well below this anyway (`MAX_BODY_BYTES`).
const MAX_BARS: usize = 10_000;

static FIELDS_DECL: &[ConfigField] = &[
    ConfigField {
        name: "timeframes",
        label: "Timeframes",
        placeholder: "1m, 1h, 1d",
        kind: "text",
        required: false,
        help: "Comma-separated timeframes this source pushes bars at, and the ones a live chart \
               may fold its ticks into. Required for candles.",
    },
    http::SHAPE_FIELD,
    http::COLUMNS_FIELD,
    http::TS_UNIT_FIELD,
    http::STAMP_FIELD,
    http::DAILY_TZ_FIELD,
    http::ASSET_TYPES_FIELD,
    http::FREQUENCY_FIELD,
    http::UNIT_FIELD,
    http::SYMBOLS_FIELD,
];

static TEMPLATE: Capability = Capability {
    provider: KIND,
    label: "Custom push source",
    website: "",
    docs_url: "",
    rate_limit: "Your own script POSTs bars or ticks to a webhook bound to this connector: nothing is fetched.",
    required_secrets: &[],
    asset_types: &[],
    timeframes: &[],
    adjusted: false,
    max_bars_per_req: 1000,
    min_interval_ms: 0,
    searchable: false,
    config_fields: FIELDS_DECL,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

/// A push connector's settings, read and checked.
#[derive(Debug, Clone)]
pub(crate) struct Spec {
    pub timeframes: Vec<String>,
    pub mapping: Mapping,
    pub symbols: Vec<String>,
    /// Set when the source is value-shaped: it receives observations, not bars.
    pub value: Option<ValueMapping>,
}

impl Spec {
    pub fn parse(cfg: &HashMap<String, String>) -> Result<Spec> {
        super::value::check_shape(cfg)?;
        if super::value::is_value_shape(cfg) {
            let vm = ValueMapping::parse(cfg)?;
            return Ok(Spec {
                timeframes: Vec::new(),
                mapping: Mapping { ts_unit: vm.ts_unit, ..Mapping::none() },
                symbols: http::list(http::get(cfg, "symbols")),
                value: Some(vm),
            });
        }
        let mut timeframes: Vec<String> = Vec::new();
        for tf in http::list(http::get(cfg, "timeframes")) {
            if !SUPPORTED_TIMEFRAMES.contains(&tf.as_str()) {
                bail!("timeframes: {tf:?} is not a chart timeframe. Use {}", SUPPORTED_TIMEFRAMES.join(", "));
            }
            if !timeframes.contains(&tf) {
                timeframes.push(tf);
            }
        }
        if timeframes.is_empty() {
            bail!("Timeframes is required: list at least one, e.g. 1m, 1d");
        }
        Ok(Spec { timeframes, mapping: Mapping::parse(cfg)?, symbols: http::list(http::get(cfg, "symbols")), value: None })
    }
}

/// One posted payload, read.
#[derive(Debug, Clone)]
pub(crate) enum Payload {
    /// Bars at one timeframe, at their opening instant, sorted, one per period.
    Bars { symbol: String, asset_type: String, timeframe: String, bars: Vec<Bar> },
    /// One trade print.
    Tick { symbol: String, asset_type: String, price: f64, size: f64, ts: OffsetDateTime },
    /// Observations of one value series, sorted, one per period.
    Observations { series: String, title: Option<String>, observations: Vec<Obs>, missing: usize },
}

/// A tick's time, in the connector's timestamp unit. A number is read in the unit, a text as
/// a number first (some scripts quote every value), then as ISO 8601.
fn tick_time(v: &Value, unit: TsUnit) -> Result<OffsetDateTime> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => bail!("tick.ts must be a number or a text"),
    };
    match (n, unit) {
        (Some(n), TsUnit::Secs) => OffsetDateTime::from_unix_timestamp_nanos((n * 1e9) as i128)
            .map_err(|e| anyhow!("tick.ts {n}: {e}")),
        (Some(n), TsUnit::Millis) => OffsetDateTime::from_unix_timestamp_nanos((n * 1e6) as i128)
            .map_err(|e| anyhow!("tick.ts {n}: {e}")),
        (_, _) => {
            let s = v.as_str().ok_or_else(|| anyhow!("tick.ts: an ISO 8601 text is expected (ts_unit is iso)"))?;
            let s = s.trim();
            OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                .or_else(|_| OffsetDateTime::parse(&format!("{s}Z"), &time::format_description::well_known::Rfc3339))
                .map_err(|e| anyhow!("tick.ts {s:?} is not ISO 8601: {e}"))
        }
    }
}

fn number(v: Option<&Value>, field: &str) -> Result<Option<f64>> {
    let n = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    match n {
        Some(n) if n.is_finite() => Ok(Some(n)),
        _ => bail!("{field} must be a number"),
    }
}

/// Read one posted body against the connector's settings. Every problem is named: a bar that
/// does not read is an error for the whole payload, never a silent drop.
pub(crate) fn parse_payload(spec: &Spec, body: &Value) -> Result<Payload> {
    if let Some(vm) = &spec.value {
        let (series, title, read) = super::value::parse_push(vm, body)?;
        return Ok(Payload::Observations { series, title, observations: read.obs, missing: read.missing.len() });
    }
    let obj = body.as_object().ok_or_else(|| anyhow!("the body must be a JSON object with symbol and bars or tick"))?;
    let symbol = obj
        .get("symbol")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("symbol is required"))?
        .to_string();
    // The group the bars are filed under: the connector's first unless the body names one.
    let asset_type = match obj.get("asset_type").and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase()) {
        None => spec.mapping.asset_types[0].clone(),
        Some(a) if spec.mapping.asset_types.contains(&a) => a,
        Some(a) => bail!("asset_type {a:?} is not one this source files: {}", spec.mapping.asset_types.join(", ")),
    };
    match (obj.get("bars"), obj.get("tick")) {
        (Some(_), Some(_)) => bail!("send bars or a tick, not both"),
        (Some(rows), None) => {
            let timeframe = obj
                .get("timeframe")
                .and_then(Value::as_str)
                .map(str::trim)
                .ok_or_else(|| anyhow!("timeframe is required with bars"))?
                .to_string();
            if !spec.timeframes.contains(&timeframe) {
                bail!("timeframe {timeframe:?} is not one this source pushes: its timeframes are {}", spec.timeframes.join(", "));
            }
            if rows.as_array().is_some_and(|a| a.len() > MAX_BARS) {
                bail!("at most {MAX_BARS} bars per payload: send history in batches");
            }
            let bars = spec.mapping.bars(rows, &timeframe)?;
            if bars.is_empty() {
                bail!("bars is empty");
            }
            Ok(Payload::Bars { symbol, asset_type, timeframe, bars })
        }
        (None, Some(tick)) => {
            let t = tick.as_object().ok_or_else(|| anyhow!("tick must be an object with ts, price and size"))?;
            let ts = tick_time(t.get("ts").ok_or_else(|| anyhow!("tick.ts is required"))?, spec.mapping.ts_unit)?;
            let price = number(t.get("price"), "tick.price")?.ok_or_else(|| anyhow!("tick.price is required"))?;
            if price <= 0.0 {
                bail!("tick.price must be positive");
            }
            let size = number(t.get("size"), "tick.size")?.unwrap_or(0.0).max(0.0);
            Ok(Payload::Tick { symbol, asset_type, price, size, ts })
        }
        (None, None) => bail!("the body carries neither bars nor tick"),
    }
}

/// What a webhook needs to read a payload for a connector row: a push source's own settings,
/// or an `http` source's mapping and mapped timeframes (a pulled source can be pushed to as
/// well, which is how a script fills what its endpoint cannot page back to).
pub(crate) fn spec_for(provider: &str, config: &HashMap<String, String>) -> Result<Spec> {
    match super::kind_of(provider) {
        Some(KIND) if super::is_instance(provider) => Spec::parse(config),
        Some(http::KIND) if super::is_instance(provider) => {
            let s = http::Spec::parse(config)?;
            if let Some(vm) = s.value {
                return Ok(Spec {
                    timeframes: Vec::new(),
                    mapping: Mapping { ts_unit: vm.ts_unit, ..Mapping::none() },
                    symbols: s.symbols,
                    value: Some(vm),
                });
            }
            Ok(Spec {
                timeframes: s.tf_map.keys().cloned().collect(),
                mapping: Mapping {
                    columns: s.columns,
                    ts_unit: s.ts_unit,
                    stamp_close: s.stamp_close,
                    daily_tz: s.daily_tz,
                    asset_types: s.asset_types,
                },
                symbols: s.symbols,
                value: None,
            })
        }
        _ => bail!("{provider} is not a custom push or HTTP source: bind the webhook to one of those"),
    }
}

/// Where a daily or weekly candle folded from ticks starts, as seconds into the timeframe's
/// cycle: today's midnight in the connector's daily time zone, this week's Monday midnight
/// for a week. `None` for an intraday timeframe, which stays on the epoch grid.
pub(crate) fn period_offset(spec: &Spec, tf_secs: i64, now: OffsetDateTime) -> Option<i64> {
    use time_tz::{OffsetDateTimeExt, PrimitiveDateTimeExt};
    if tf_secs < 86_400 {
        return None;
    }
    let mut date = now.to_timezone(spec.mapping.daily_tz).date();
    if tf_secs >= 7 * 86_400 {
        date -= time::Duration::days(i64::from(date.weekday().number_days_from_monday()));
    }
    let midnight = date.midnight().assume_timezone(spec.mapping.daily_tz).take_first()?;
    Some(midnight.unix_timestamp().rem_euclid(tf_secs))
}

pub struct PushSource {
    cap: &'static Capability,
    config: HashMap<String, String>,
}

impl PushSource {
    pub fn template() -> Self {
        PushSource { cap: &TEMPLATE, config: HashMap::new() }
    }

    /// A minted connector. Settings that do not parse still give a listed connector, with the
    /// reason in its rate-limit line, so it can be fixed.
    pub fn new(provider: &str, name: &str, config: HashMap<String, String>) -> Self {
        let label = if name.trim().is_empty() { TEMPLATE.label.to_string() } else { name.trim().to_string() };
        let base = CapabilitySpec {
            provider: provider.to_string(),
            label,
            website: String::new(),
            docs_url: String::new(),
            rate_limit: TEMPLATE.rate_limit.to_string(),
            required_secrets: Vec::new(),
            asset_types: Vec::new(),
            timeframes: Vec::new(),
            max_bars_per_req: TEMPLATE.max_bars_per_req,
            min_interval_ms: 0,
            searchable: false,
            testable: false,
            config_fields: FIELDS_DECL,
            fundamentals: false,
        };
        let spec = match Spec::parse(&config) {
            Ok(s) if s.value.is_some() => CapabilitySpec {
                rate_limit: "Your own script POSTs observations to a webhook bound to this connector: its series appear in Fundamentals.".into(),
                searchable: !s.symbols.is_empty(),
                fundamentals: true,
                ..base
            },
            Ok(s) => CapabilitySpec {
                asset_types: s.mapping.asset_types.clone(),
                timeframes: s.timeframes.clone(),
                searchable: !s.symbols.is_empty(),
                ..base
            },
            Err(e) => CapabilitySpec { rate_limit: format!("Settings incomplete: {e:#}"), ..base },
        };
        PushSource { cap: Capability::runtime(spec).unwrap_or(&TEMPLATE), config }
    }

    pub(crate) fn spec(&self) -> Result<Spec> {
        if self.cap.provider == KIND {
            bail!("a custom push source is used through its own connector: create one first");
        }
        Spec::parse(&self.config)
    }
}

#[async_trait::async_trait]
impl Connector for PushSource {
    fn capability(&self) -> &'static Capability {
        self.cap
    }

    async fn fetch_chunk(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        ticker: &str,
        _asset_type: &str,
        timeframe: &str,
        _from: OffsetDateTime,
        _to: OffsetDateTime,
    ) -> Result<Chunk> {
        bail!(
            "{} is a push source, nothing to download: {ticker} {timeframe} arrives through its webhook",
            self.cap.label
        )
    }

    async fn search_symbols(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        query: &str,
        asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let spec = self.spec()?;
        if spec.symbols.is_empty() {
            bail!("{} has no symbol search: set Symbols, or type the symbol", self.cap.label);
        }
        let at = if asset_type.is_empty() { spec.mapping.asset_types[0].clone() } else { asset_type.to_string() };
        let all: Vec<SymbolHit> = spec
            .symbols
            .iter()
            .map(|s| SymbolHit { symbol: s.clone(), name: String::new(), asset_type: at.clone(), exchange: self.cap.label.to_string() })
            .collect();
        Ok(crate::histdata::filter_symbols(&all, query, limit))
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        Spec::parse(config).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::macros::datetime;

    fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn spec_of(columns: &str, unit: &str) -> Spec {
        Spec::parse(&settings(&[("timeframes", "1m, 1d"), ("columns", columns), ("ts_unit", unit)])).unwrap()
    }

    #[test]
    fn bars_as_lists_read_through_the_mapping() {
        let spec = spec_of(r#"{"ts": 0, "open": 1, "high": 2, "low": 3, "close": 4, "volume": 5}"#, "ms");
        let body = json!({
            "symbol": "ACME",
            "timeframe": "1m",
            "bars": [[1704067260000i64, 2, 3, 1, 2.5, 10], [1704067200000i64, 1, 2, 0.5, 1.5, 7]],
        });
        let Payload::Bars { symbol, asset_type, timeframe, bars } = parse_payload(&spec, &body).unwrap() else { panic!("bars") };
        assert_eq!((symbol.as_str(), asset_type.as_str(), timeframe.as_str()), ("ACME", "other", "1m"));
        // Sorted on the way in, whatever order the script sent.
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].ts, datetime!(2024-01-01 00:00:00 UTC));
        assert_eq!((bars[1].open, bars[1].high, bars[1].low, bars[1].close, bars[1].volume), (2.0, 3.0, 1.0, 2.5, 10.0));
    }

    #[test]
    fn bars_as_objects_read_through_the_mapping() {
        let spec = spec_of(r#"{"ts": "t", "close": "c", "volume": "v"}"#, "iso");
        let body = json!({
            "symbol": "ACME",
            "timeframe": "1d",
            "bars": [{"t": "2024-01-02T00:00:00Z", "c": "101.5", "v": 3}],
        });
        let Payload::Bars { bars, .. } = parse_payload(&spec, &body).unwrap() else { panic!("bars") };
        assert_eq!(bars[0].ts, datetime!(2024-01-02 00:00:00 UTC));
        assert_eq!(bars[0].close, 101.5);
    }

    #[test]
    fn a_tick_is_read_in_the_connector_unit() {
        let spec = spec_of(r#"{"ts": 0, "close": 1}"#, "s");
        let body = json!({"symbol": "ACME", "tick": {"ts": 1704067230.5, "price": "12.25", "size": 3}});
        let Payload::Tick { symbol, price, size, ts, .. } = parse_payload(&spec, &body).unwrap() else { panic!("tick") };
        assert_eq!(symbol, "ACME");
        assert_eq!((price, size), (12.25, 3.0));
        assert_eq!(ts, datetime!(2024-01-01 00:00:30.5 UTC));
        // ISO when the unit says so, size optional.
        let iso = spec_of(r#"{"ts": 0, "close": 1}"#, "iso");
        let body = json!({"symbol": "ACME", "tick": {"ts": "2024-01-01T00:00:30Z", "price": 12}});
        let Payload::Tick { size, ts, .. } = parse_payload(&iso, &body).unwrap() else { panic!("tick") };
        assert_eq!((size, ts), (0.0, datetime!(2024-01-01 00:00:30 UTC)));
    }

    #[test]
    fn a_payload_that_does_not_read_is_named() {
        let spec = spec_of(r#"{"ts": 0, "close": 1}"#, "s");
        let err = |b: Value| parse_payload(&spec, &b).unwrap_err().to_string();
        assert!(err(json!({"bars": []})).contains("symbol"));
        assert!(err(json!({"symbol": "A", "bars": [[1, 2]]})).contains("timeframe is required"));
        assert!(err(json!({"symbol": "A", "timeframe": "1h", "bars": [[1, 2]]})).contains("not one this source pushes"));
        assert!(err(json!({"symbol": "A", "tick": {"ts": 1}})).contains("tick.price"));
        assert!(err(json!({"symbol": "A", "tick": {"ts": 1, "price": -1}})).contains("positive"));
        assert!(err(json!({"symbol": "A"})).contains("neither"));
        assert!(err(json!({"symbol": "A", "asset_type": "equity", "tick": {"ts": 1, "price": 1}})).contains("asset_type"));
        assert!(err(json!({"symbol": "A", "timeframe": "1m", "bars": [["x", 2]]})).contains("could not be read"));
    }

    #[test]
    fn settings_need_timeframes_and_a_mapping() {
        assert!(Spec::parse(&settings(&[("columns", r#"{"ts": 0, "close": 1}"#), ("ts_unit", "s")])).is_err());
        assert!(Spec::parse(&settings(&[("timeframes", "2m"), ("columns", r#"{"ts": 0, "close": 1}"#), ("ts_unit", "s")])).is_err());
        assert!(Spec::parse(&settings(&[("timeframes", "1m")])).is_err());
        let c = PushSource::new("push-0000beef", "Script", settings(&[("timeframes", "1d,1m"), ("columns", r#"{"ts": 0, "close": 1}"#), ("ts_unit", "s")]));
        let cap = c.capability();
        assert_eq!(cap.timeframes, ["1m", "1d"]);
        assert_eq!(cap.asset_types, ["other"]);
    }

    #[test]
    fn a_daily_candle_starts_at_the_connector_midnight() {
        let spec = Spec::parse(&settings(&[
            ("timeframes", "1d"),
            ("columns", r#"{"ts": 0, "close": 1}"#),
            ("ts_unit", "s"),
            ("daily_tz", "America/New_York"),
        ]))
        .unwrap();
        // Midnight in New York in January is 05:00 UTC.
        let now = datetime!(2024-01-10 12:00:00 UTC);
        assert_eq!(period_offset(&spec, 86_400, now), Some(5 * 3600));
        assert_eq!(period_offset(&spec_of(r#"{"ts": 0, "close": 1}"#, "s"), 86_400, now), Some(0));
        // A week starts on Monday 2024-01-08 at 05:00 UTC, wherever the epoch put Thursday.
        let monday = datetime!(2024-01-08 05:00:00 UTC).unix_timestamp();
        assert_eq!(period_offset(&spec, 604_800, now), Some(monday.rem_euclid(604_800)));
        assert_eq!(period_offset(&spec, 3600, now), None);
    }
}
