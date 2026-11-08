//! The `http` kind: candles pulled from any JSON REST endpoint the user describes.
//!
//! Everything a Rust provider would hard-code is a setting here: the URL template, the
//! timeframe tokens, where the rows sit in the answer, which column is which, the clock the
//! timestamps are written in, and whether a bar is stamped at its open or at its close. The
//! rows are read by the file import's own reader ([`crate::histdata_import::build`]), so a
//! timestamp, a price and an empty cell mean the same thing here as in an uploaded CSV.
//!
//! The server fetches a URL the user typed, so before any request the host is resolved and
//! a loopback, link-local or private address is refused unless the connector says
//! `allow_private_hosts`. The checked address is pinned for the request and redirects are
//! not followed, so neither a DNS answer that changes nor a `302` can move the call inside.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use time_tz::{OffsetDateTimeExt, PrimitiveDateTimeExt, TimeZone, Tz};

use otw_store::histdata::Bar;

use crate::histdata::caps::CapabilitySpec;
use crate::histdata::{Capability, Chunk, ConfigField, Connector, SymbolHit, SUPPORTED_TIMEFRAMES};
use crate::import::parse::Grid;

/// Kind id: what the create form posts, and the prefix of every minted provider id.
pub const KIND: &str = "http";

/// The one credential this kind knows: the value of the auth header or query parameter.
pub const AUTH_SECRET: &str = "auth_value";

const TIMEOUT: Duration = Duration::from_secs(20);

/// Requests one walk may spend on its window (a candle chunk, a value series). A source whose
/// page is far smaller than `max_bars` runs out of these and says so instead of storing a hole.
const MAX_PAGES: usize = 64;

/// Largest answer read, in bytes. A page of 100 000 bars as JSON stays well under it.
const MAX_BODY: usize = 32 << 20;

/// Bars the connection test asks for.
const TEST_BARS: i64 = 20;

/// Bars the connection test returns as its sample.
const SAMPLE_BARS: usize = 10;

/// The bar fields a column mapping can name.
const FIELDS: [&str; 6] = ["ts", "open", "high", "low", "close", "volume"];

/// Placeholders a URL or body template may carry.
const PLACEHOLDERS: [&str; 12] = [
    "symbol", "tf", "limit", "asset_type", "from", "to", "from_s", "to_s", "from_ms", "to_ms", "from_iso", "to_iso",
];

// Mapping settings, shared with the `push` kind.
pub(crate) const COLUMNS_FIELD: ConfigField = ConfigField {
    name: "columns",
    label: "Columns",
    placeholder: "",
    kind: "json",
    required: true,
    help: "Which value is which, as JSON. A number is a position when each row is a list, \
           a text is a key when each row is an object or when the answer is one object of \
           parallel lists. ts and close are required. Example: \
           {\"ts\": 0, \"open\": 1, \"high\": 2, \"low\": 3, \"close\": 4, \"volume\": 5}",
};

pub(crate) const TS_UNIT_FIELD: ConfigField = ConfigField {
    name: "ts_unit",
    label: "Timestamp unit",
    placeholder: "",
    kind: "text",
    required: true,
    help: "s (epoch seconds), ms (epoch milliseconds) or iso (ISO 8601 text; one without \
           an offset is read as UTC).",
};

pub(crate) const STAMP_FIELD: ConfigField = ConfigField {
    name: "stamp",
    label: "Bar stamp",
    placeholder: "open",
    kind: "text",
    required: false,
    help: "open when a bar's time is its opening instant, close when it is its closing \
           one. A close stamp is moved back once, by one bar.",
};

pub(crate) const DAILY_TZ_FIELD: ConfigField = ConfigField {
    name: "daily_tz",
    label: "Daily time zone",
    placeholder: "UTC",
    kind: "text",
    required: false,
    help: "Where the source's day starts, as an IANA name (America/New_York). Daily and \
           weekly bars are stored at that day's midnight.",
};

pub(crate) const ASSET_TYPES_FIELD: ConfigField = ConfigField {
    name: "asset_types",
    label: "Asset types",
    placeholder: "",
    kind: "text",
    required: false,
    help: "Comma-separated groups this source files its bars under (crypto, equity, fx, \
           ...). Empty means other.",
};

pub(crate) const SHAPE_FIELD: ConfigField = ConfigField {
    name: "shape",
    label: "Shape",
    placeholder: "ohlcv",
    kind: "text",
    required: false,
    help: "ohlcv (candles, stored as datasets) or value (one number per period, stored as a \
           Fundamentals series and drawn on the chart as a step line). A value source maps \
           period_start, value and optionally released_at in Columns, and sets Frequency.",
};

pub(crate) const FREQUENCY_FIELD: ConfigField = ConfigField {
    name: "frequency",
    label: "Frequency",
    placeholder: "1M",
    kind: "text",
    required: false,
    help: "Value shape only: 1d, 1W, 1M, 1Q, 1Y or irregular (each row stands alone). A pulled \
           series with nothing stored starts 5 years back at 1d, 10 at 1W, 30 at 1M and coarser \
           (and irregular), then each refresh resumes from its last stored period.",
};

pub(crate) const UNIT_FIELD: ConfigField = ConfigField {
    name: "unit",
    label: "Unit",
    placeholder: "",
    kind: "text",
    required: false,
    help: "Value shape only: the unit the values are in (%, index, USD bn, ...).",
};

pub(crate) const SYMBOLS_FIELD: ConfigField = ConfigField {
    name: "symbols",
    label: "Symbols",
    placeholder: "",
    kind: "text",
    required: false,
    help: "Comma-separated symbols for the symbol search, when the source has no list endpoint.",
};

static FIELDS_DECL: &[ConfigField] = &[
    ConfigField {
        name: "url",
        label: "URL template",
        placeholder: "",
        kind: "text",
        required: true,
        help: "The bars endpoint, with {symbol} and optionally {tf} {from} {to} {limit}. \
               {from}/{to} are written in the timestamp unit below; {from_s} {from_ms} \
               {from_iso} (and the same for to) force one. Example: \
               https://api.example.com/klines?symbol={symbol}&interval={tf}&start={from}&end={to}&limit={limit}",
    },
    ConfigField {
        name: "method",
        label: "Method",
        placeholder: "GET",
        kind: "text",
        required: false,
        help: "GET or POST.",
    },
    ConfigField {
        name: "body",
        label: "POST body",
        placeholder: "",
        kind: "json",
        required: false,
        help: "POST only: the JSON body sent, with the same placeholders as the URL.",
    },
    ConfigField {
        name: "tf_map",
        label: "Timeframes",
        placeholder: "",
        kind: "json",
        required: false,
        help: "Chart timeframe to the source's own token, as JSON. Only the timeframes listed \
               here are offered; required for candles. Example: {\"1m\": \"1\", \"1h\": \"60\", \"1d\": \"D\"}",
    },
    ConfigField {
        name: "rows_path",
        label: "Rows path",
        placeholder: "",
        kind: "text",
        required: false,
        help: "Dot path to the rows in the answer, e.g. data.candles (a number indexes a \
               list). Empty when the answer is the rows itself.",
    },
    SHAPE_FIELD,
    COLUMNS_FIELD,
    TS_UNIT_FIELD,
    STAMP_FIELD,
    DAILY_TZ_FIELD,
    ASSET_TYPES_FIELD,
    FREQUENCY_FIELD,
    UNIT_FIELD,
    ConfigField {
        name: "max_bars",
        label: "Bars per request",
        placeholder: "1000",
        kind: "number",
        required: false,
        help: "The most bars one request returns; {limit} is set to it and downloads page by it.",
    },
    ConfigField {
        name: "min_interval_ms",
        label: "Delay between requests (ms)",
        placeholder: "250",
        kind: "number",
        required: false,
        help: "Kept between two requests so a long download stays under the source's rate limit.",
    },
    crate::live::poll::POLL_SECS_FIELD,
    ConfigField {
        name: "auth_header",
        label: "Auth header name",
        placeholder: "",
        kind: "text",
        required: false,
        help: "Send the auth_value credential in this header (X-API-Key, Authorization, ...).",
    },
    ConfigField {
        name: "auth_query",
        label: "Auth query parameter",
        placeholder: "",
        kind: "text",
        required: false,
        help: "Or send it as this query parameter (apikey, token, ...). Not both.",
    },
    SYMBOLS_FIELD,
    ConfigField {
        name: "symbols_url",
        label: "Symbols URL",
        placeholder: "",
        kind: "text",
        required: false,
        help: "An endpoint answering the list of symbols, for the symbol search.",
    },
    ConfigField {
        name: "symbols_path",
        label: "Symbols path",
        placeholder: "",
        kind: "text",
        required: false,
        help: "Dot path to the list in that answer. Each entry is a text, or an object whose \
               symbols_key holds the symbol.",
    },
    ConfigField {
        name: "symbols_key",
        label: "Symbols key",
        placeholder: "",
        kind: "text",
        required: false,
        help: "Key of the symbol in each entry when the entries are objects. Empty means symbol.",
    },
    ConfigField {
        name: "test_symbol",
        label: "Test symbol",
        placeholder: "",
        kind: "text",
        required: false,
        help: "The symbol the connection test asks for. Empty means the first of Symbols.",
    },
    ConfigField {
        name: "allow_private_hosts",
        label: "Allow private hosts",
        placeholder: "",
        kind: "text",
        required: false,
        help: "true lets this connector reach a loopback or private network address (a \
               server on your own LAN). Refused otherwise.",
    },
];

/// The kind as the providers list and the create form show it.
static TEMPLATE: Capability = Capability {
    provider: KIND,
    label: "Custom HTTP source",
    website: "",
    docs_url: "",
    rate_limit: "Your own JSON endpoint: requests are spaced by the delay you set (250 ms by default).",
    required_secrets: &[AUTH_SECRET],
    asset_types: &[],
    timeframes: &[],
    adjusted: false,
    max_bars_per_req: 1000,
    min_interval_ms: 250,
    searchable: false,
    config_fields: FIELDS_DECL,
    testable: true,
    fundamentals: &[],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TsUnit {
    Secs,
    Millis,
    Iso,
}

impl TsUnit {
    /// The file import's spelling of the same unit.
    fn import_unit(self) -> &'static str {
        match self {
            TsUnit::Secs => "s",
            TsUnit::Millis => "ms",
            TsUnit::Iso => "text",
        }
    }
}

/// Where a bar field sits in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Col {
    Index(usize),
    Key(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Auth {
    Header(String),
    Query(String),
}

/// A connector's settings, read and checked.
#[derive(Debug, Clone)]
pub(crate) struct Spec {
    pub method: reqwest::Method,
    pub url: String,
    pub body: Option<String>,
    pub tf_map: BTreeMap<String, String>,
    pub rows_path: String,
    pub columns: Vec<(&'static str, Col)>,
    pub ts_unit: TsUnit,
    pub stamp_close: bool,
    pub daily_tz: &'static Tz,
    pub asset_types: Vec<String>,
    pub max_bars: u32,
    pub min_interval_ms: u64,
    pub auth: Option<Auth>,
    pub symbols: Vec<String>,
    pub symbols_url: Option<String>,
    pub symbols_path: String,
    pub symbols_key: String,
    pub test_symbol: Option<String>,
    pub allow_private_hosts: bool,
    /// Set when the source is value-shaped: its rows are observations, not bars.
    pub value: Option<super::value::ValueMapping>,
}

pub(crate) fn get<'a>(cfg: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    cfg.get(key).map(|s| s.trim()).filter(|s| !s.is_empty())
}

pub(crate) fn list(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Every `{name}` in a template must be one this kind fills: a typo would otherwise reach the
/// source verbatim and come back as an answer about something else.
fn check_placeholders(field: &str, template: &str) -> Result<()> {
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let name = &after[..close];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_') && !PLACEHOLDERS.contains(&name) {
            bail!("{field}: {{{name}}} is not a placeholder. Use {}", PLACEHOLDERS.map(|p| format!("{{{p}}}")).join(" "));
        }
        rest = &after[close..];
    }
    Ok(())
}

fn json_object(field: &str, raw: &str) -> Result<serde_json::Map<String, Value>> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => bail!("{field} must be a JSON object"),
        Err(e) => bail!("{field} is not valid JSON: {e}"),
    }
}

/// How one row becomes one bar: which value is which, the clock the timestamps are written
/// in, where a bar is stamped, where a day starts, and the groups the bars are filed under.
/// Shared by the `http` kind (rows read off an answer) and the `push` kind (rows posted to a
/// webhook), so a row means the same thing whichever way it arrived.
#[derive(Debug, Clone)]
pub(crate) struct Mapping {
    pub columns: Vec<(&'static str, Col)>,
    pub ts_unit: TsUnit,
    pub stamp_close: bool,
    pub daily_tz: &'static Tz,
    pub asset_types: Vec<String>,
}

impl Mapping {
    /// No bar mapping: the place-holder a value-shaped source carries.
    pub fn none() -> Mapping {
        Mapping {
            columns: Vec::new(),
            ts_unit: TsUnit::Iso,
            stamp_close: false,
            daily_tz: time_tz::timezones::db::UTC,
            asset_types: vec!["other".into()],
        }
    }

    /// The mapping settings (`columns`, `ts_unit`, `stamp`, `daily_tz`, `asset_types`), read
    /// and checked.
    pub fn parse(cfg: &HashMap<String, String>) -> Result<Mapping> {
        let raw_cols = get(cfg, "columns").ok_or_else(|| anyhow!("Columns is required"))?;
        let mut columns: Vec<(&'static str, Col)> = Vec::new();
        for (field, col) in json_object("columns", raw_cols)? {
            let Some(f) = FIELDS.iter().copied().find(|f| *f == field) else {
                bail!("columns: {field:?} is not a bar field. Use {}", FIELDS.join(", "));
            };
            let col = match col {
                Value::Number(n) => Col::Index(
                    n.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| anyhow!("columns: {field} must be a position (0, 1, ...) or a key"))?,
                ),
                Value::String(s) if !s.trim().is_empty() => Col::Key(s.trim().to_string()),
                _ => bail!("columns: {field} must be a position (0, 1, ...) or a key"),
            };
            columns.push((f, col));
        }
        for required in ["ts", "close"] {
            if !columns.iter().any(|(f, _)| *f == required) {
                bail!("columns: {required} is required");
            }
        }
        columns.sort_by_key(|(f, _)| FIELDS.iter().position(|x| x == f));

        let ts_unit = match get(cfg, "ts_unit") {
            Some("s") => TsUnit::Secs,
            Some("ms") => TsUnit::Millis,
            Some("iso") => TsUnit::Iso,
            Some(other) => bail!("timestamp unit {other:?}: use s, ms or iso"),
            None => bail!("Timestamp unit is required: s, ms or iso"),
        };
        let stamp_close = match get(cfg, "stamp") {
            None | Some("open") => false,
            Some("close") => true,
            Some(other) => bail!("bar stamp {other:?}: use open or close"),
        };
        let daily_tz = match get(cfg, "daily_tz") {
            None => time_tz::timezones::db::UTC,
            Some(name) => time_tz::timezones::get_by_name(name)
                .ok_or_else(|| anyhow!("daily time zone {name:?} is not an IANA name (America/New_York, Europe/Paris, UTC)"))?,
        };
        let mut asset_types: Vec<String> = list(get(cfg, "asset_types")).into_iter().map(|s| s.to_ascii_lowercase()).collect();
        asset_types.dedup();
        if asset_types.is_empty() {
            asset_types.push("other".into());
        }
        Ok(Mapping { columns, ts_unit, stamp_close, daily_tz, asset_types })
    }

    /// Every bar of a rows value (a list of lists, a list of objects, or one object of
    /// parallel lists), at its opening instant, sorted, one per period.
    pub fn bars(&self, rows: &Value, timeframe: &str) -> Result<Vec<Bar>> {
        parse_rows(rows, &self.columns, self.ts_unit, self.stamp_close, self.daily_tz, timeframe)
    }
}

impl Spec {
    pub fn parse(cfg: &HashMap<String, String>) -> Result<Spec> {
        let url = get(cfg, "url").ok_or_else(|| anyhow!("URL template is required"))?.to_string();
        if !url.contains("{symbol}") {
            bail!("the URL template must contain {{symbol}}: without it every symbol would get the same bars");
        }
        check_placeholders("URL template", &url)?;
        let probe = reqwest::Url::parse(&render(&url, &Vars::probe(), TsUnit::Secs, Enc::Url))
            .map_err(|e| anyhow!("URL template is not a URL: {e}"))?;
        if !matches!(probe.scheme(), "http" | "https") {
            bail!("URL template must start with http:// or https://");
        }
        let method = match get(cfg, "method").map(str::to_ascii_uppercase).as_deref() {
            None | Some("GET") => reqwest::Method::GET,
            Some("POST") => reqwest::Method::POST,
            Some(other) => bail!("method {other:?}: use GET or POST"),
        };
        let body = get(cfg, "body").map(str::to_string);
        if let Some(b) = &body {
            check_placeholders("POST body", b)?;
            if method != reqwest::Method::POST {
                bail!("a POST body is set but the method is GET");
            }
        }

        super::value::check_shape(cfg)?;
        let value = if super::value::is_value_shape(cfg) { Some(super::value::ValueMapping::parse(cfg)?) } else { None };
        let raw_tf = match (get(cfg, "tf_map"), &value) {
            (Some(raw), _) => raw,
            // A value source has no timeframes: its frequency is its own.
            (None, Some(_)) => "{}",
            (None, None) => bail!("Timeframes (tf_map) is required"),
        };
        let mut tf_map = BTreeMap::new();
        for (tf, token) in json_object("tf_map", raw_tf)? {
            if !SUPPORTED_TIMEFRAMES.contains(&tf.as_str()) {
                bail!("tf_map: {tf:?} is not a chart timeframe. Use {}", SUPPORTED_TIMEFRAMES.join(", "));
            }
            let token = match token {
                Value::String(s) => s.trim().to_string(),
                Value::Number(n) => n.to_string(),
                _ => bail!("tf_map: the token for {tf} must be a text or a number"),
            };
            if token.is_empty() {
                bail!("tf_map: the token for {tf} is empty");
            }
            tf_map.insert(tf, token);
        }
        if tf_map.is_empty() && value.is_none() {
            bail!("tf_map maps no timeframe: add at least one, e.g. {{\"1d\": \"1d\"}}");
        }

        let Mapping { columns, ts_unit, stamp_close, daily_tz, asset_types } = match &value {
            Some(v) => Mapping { ts_unit: v.ts_unit, ..Mapping::none() },
            None => Mapping::parse(cfg)?,
        };
        let number = |key: &str, default: u64, max: u64| -> Result<u64> {
            match get(cfg, key) {
                None => Ok(default),
                Some(s) => {
                    let n: u64 = s.parse().map_err(|_| anyhow!("{key} must be a whole number"))?;
                    if n > max {
                        bail!("{key} is at most {max}");
                    }
                    Ok(n)
                }
            }
        };
        let max_bars = number("max_bars", 1000, 100_000)? as u32;
        if max_bars == 0 {
            bail!("max_bars must be at least 1");
        }
        let min_interval_ms = number("min_interval_ms", 250, 600_000)?;
        crate::live::poll::poll_secs_setting(cfg)?;
        let auth = match (get(cfg, "auth_header"), get(cfg, "auth_query")) {
            (Some(_), Some(_)) => bail!("set the auth header or the auth query parameter, not both"),
            (Some(h), None) => {
                reqwest::header::HeaderName::from_bytes(h.as_bytes()).map_err(|_| anyhow!("{h:?} is not a header name"))?;
                Some(Auth::Header(h.to_string()))
            }
            (None, Some(q)) => Some(Auth::Query(q.to_string())),
            (None, None) => None,
        };
        let symbols = list(get(cfg, "symbols"));
        let symbols_url = get(cfg, "symbols_url").map(str::to_string);
        if let Some(u) = &symbols_url {
            let parsed = reqwest::Url::parse(u).map_err(|e| anyhow!("symbols URL is not a URL: {e}"))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                bail!("symbols URL must start with http:// or https://");
            }
        }
        let symbols_path = get(cfg, "symbols_path").unwrap_or("").to_string();
        if !symbols_path.is_empty() && symbols_url.is_none() {
            bail!("a symbols path is set but no symbols URL");
        }
        let allow_private_hosts = match get(cfg, "allow_private_hosts").map(str::to_ascii_lowercase).as_deref() {
            None | Some("false") | Some("no") => false,
            Some("true") | Some("yes") => true,
            Some(other) => bail!("allow_private_hosts {other:?}: use true or false"),
        };
        Ok(Spec {
            method,
            url,
            body,
            tf_map,
            rows_path: get(cfg, "rows_path").unwrap_or("").to_string(),
            columns,
            ts_unit,
            stamp_close,
            daily_tz,
            asset_types,
            max_bars,
            min_interval_ms,
            auth,
            symbols,
            symbols_url,
            symbols_path,
            symbols_key: get(cfg, "symbols_key").unwrap_or("symbol").to_string(),
            test_symbol: get(cfg, "test_symbol").map(str::to_string),
            allow_private_hosts,
            value,
        })
    }

    /// Whether the request names its window: without it the source answers the same bars for
    /// every window, so there is nothing to walk.
    fn windowed(&self) -> bool {
        let names = ["{from", "{to"];
        names.iter().any(|n| self.url.contains(n) || self.body.as_deref().is_some_and(|b| b.contains(n)))
    }
}

// ── Templates ────────────────────────────────────────────────────────────────

/// What a template's placeholders are filled with.
pub(crate) struct Vars<'a> {
    pub symbol: &'a str,
    pub tf: &'a str,
    pub asset_type: &'a str,
    pub limit: u32,
    pub from: OffsetDateTime,
    pub to: OffsetDateTime,
}

impl Vars<'static> {
    fn probe() -> Self {
        Vars { symbol: "X", tf: "1", asset_type: "other", limit: 1, from: OffsetDateTime::UNIX_EPOCH, to: OffsetDateTime::UNIX_EPOCH }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Enc {
    /// Percent-encoded, for a URL.
    Url,
    /// Escaped as the inside of a JSON string, for a body.
    Json,
}

fn stamp(t: OffsetDateTime, unit: TsUnit) -> String {
    match unit {
        TsUnit::Secs => t.unix_timestamp().to_string(),
        TsUnit::Millis => (t.unix_timestamp_nanos() / 1_000_000).to_string(),
        TsUnit::Iso => t.format(&Rfc3339).unwrap_or_default(),
    }
}

pub(crate) fn render(template: &str, v: &Vars, unit: TsUnit, enc: Enc) -> String {
    let esc = |s: &str| match enc {
        Enc::Url => crate::histdata::enc(s),
        Enc::Json => {
            let quoted = serde_json::to_string(s).unwrap_or_default();
            quoted[1..quoted.len().saturating_sub(1)].to_string()
        }
    };
    let pairs: [(&str, String); 12] = [
        ("{symbol}", esc(v.symbol)),
        ("{tf}", esc(v.tf)),
        ("{asset_type}", esc(v.asset_type)),
        ("{limit}", v.limit.to_string()),
        ("{from}", esc(&stamp(v.from, unit))),
        ("{to}", esc(&stamp(v.to, unit))),
        ("{from_s}", stamp(v.from, TsUnit::Secs)),
        ("{to_s}", stamp(v.to, TsUnit::Secs)),
        ("{from_ms}", stamp(v.from, TsUnit::Millis)),
        ("{to_ms}", stamp(v.to, TsUnit::Millis)),
        ("{from_iso}", esc(&stamp(v.from, TsUnit::Iso))),
        ("{to_iso}", esc(&stamp(v.to, TsUnit::Iso))),
    ];
    let mut out = template.to_string();
    for (k, val) in pairs {
        out = out.replace(k, &val);
    }
    out
}

// ── Reading an answer ────────────────────────────────────────────────────────

/// The value at a dot path (`data.candles`, `result.0`). Empty = the answer itself.
pub(crate) fn at_path<'a>(root: &'a Value, path: &str) -> Result<&'a Value> {
    let mut v = root;
    for seg in path.split('.').map(str::trim).filter(|s| !s.is_empty()) {
        v = match v {
            Value::Array(items) => seg
                .parse::<usize>()
                .ok()
                .and_then(|i| items.get(i))
                .ok_or_else(|| anyhow!("path {path:?}: {seg:?} is not a position in a list of {}", items.len()))?,
            Value::Object(map) => map.get(seg).ok_or_else(|| {
                let keys: Vec<&str> = map.keys().take(12).map(String::as_str).collect();
                anyhow!("path {path:?}: the answer has no {seg:?} (it holds: {})", keys.join(", "))
            })?,
            _ => bail!("path {path:?}: {seg:?} is inside a value that is neither a list nor an object"),
        };
    }
    Ok(v)
}

/// One JSON value as the text the import reader parses. A float is written out in full:
/// the reader skips letters, so `1.5e-7` would otherwise read as 1.57.
fn cell(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::Number(n)) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i.to_string(),
            (_, Some(u), _) => u.to_string(),
            (_, _, Some(f)) => format!("{f}"),
            _ => n.to_string(),
        },
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.contains(['e', 'E']) {
                if let Ok(f) = t.parse::<f64>() {
                    return format!("{f}");
                }
            }
            t.to_string()
        }
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
    }
}

/// The rows of an answer as a grid with one column per mapped field, whichever of the three
/// shapes it has: a list of lists, a list of objects, or one object of parallel lists.
pub(crate) fn to_grid(rows: &Value, columns: &[(&'static str, Col)]) -> Result<Grid> {
    let headers: Vec<String> = columns.iter().map(|(f, _)| f.to_string()).collect();
    let grid_rows: Vec<Vec<String>> = match rows {
        Value::Array(items) => {
            let first = items.iter().find(|v| !v.is_null());
            match first {
                None => Vec::new(),
                Some(Value::Array(_)) => {
                    let idx: Vec<usize> = columns
                        .iter()
                        .map(|(f, c)| match c {
                            Col::Index(i) => Ok(*i),
                            Col::Key(k) => k.parse::<usize>().map_err(|_| {
                                anyhow!("columns: {f} is the key {k:?} but each row is a list: give its position (0, 1, ...)")
                            }),
                        })
                        .collect::<Result<_>>()?;
                    items
                        .iter()
                        .enumerate()
                        .map(|(n, row)| match row {
                            Value::Array(cells) => Ok(idx.iter().map(|i| cell(cells.get(*i))).collect()),
                            _ => Err(anyhow!("row {}: not a list like the others", n + 1)),
                        })
                        .collect::<Result<_>>()?
                }
                Some(Value::Object(_)) => {
                    let keys: Vec<&str> = columns
                        .iter()
                        .map(|(f, c)| match c {
                            Col::Key(k) => Ok(k.as_str()),
                            Col::Index(i) => Err(anyhow!("columns: {f} is position {i} but each row is an object: give its key")),
                        })
                        .collect::<Result<_>>()?;
                    items
                        .iter()
                        .enumerate()
                        .map(|(n, row)| match row {
                            Value::Object(obj) => Ok(keys.iter().map(|k| cell(obj.get(*k))).collect()),
                            _ => Err(anyhow!("row {}: not an object like the others", n + 1)),
                        })
                        .collect::<Result<_>>()?
                }
                Some(_) => bail!("the rows are neither lists nor objects: check rows_path"),
            }
        }
        Value::Object(obj) => {
            let mut lists: Vec<&Vec<Value>> = Vec::new();
            for (f, c) in columns {
                let Col::Key(k) = c else {
                    bail!("columns: {f} is a position but the answer is one object of lists: give its key");
                };
                match obj.get(k) {
                    Some(Value::Array(a)) => lists.push(a),
                    Some(_) => bail!("columns: {k:?} is not a list in the answer"),
                    None => {
                        let keys: Vec<&str> = obj.keys().take(12).map(String::as_str).collect();
                        bail!("columns: the answer has no {k:?} (it holds: {})", keys.join(", "))
                    }
                }
            }
            let n = lists.first().map_or(0, |l| l.len());
            for ((_, c), l) in columns.iter().zip(&lists) {
                if l.len() != n {
                    let Col::Key(k) = c else { unreachable!() };
                    bail!("the list {k:?} holds {} values where the timestamps hold {n}", l.len());
                }
            }
            (0..n).map(|i| lists.iter().map(|l| cell(l.get(i))).collect()).collect()
        }
        _ => bail!("the rows are neither a list nor an object: check rows_path"),
    };
    let n = grid_rows.len();
    Ok(Grid {
        delimiter: ',',
        header_row: 0,
        headers,
        rows: grid_rows,
        row_lines: (1..=n).collect(),
        preamble: Vec::new(),
        sections: Vec::new(),
        section: None,
    })
}

/// When the bar stamped `ts` opened. A close stamp moves back one bar (TradeStation's rule).
/// A daily or weekly bar is a session, so it is stored at its day's midnight in `tz`. An open
/// stamp at midnight UTC is a date with no time (`2024-01-05`), so its own date is the day; any
/// other open stamp is the day it falls on in `tz`. A close stamp is read one second earlier,
/// so a bar closing at midnight stays on its own day, and a weekly close lands on that week's
/// Monday.
pub(crate) fn opened_at(ts: OffsetDateTime, timeframe: &str, stamp_close: bool, tz: &'static Tz) -> Result<OffsetDateTime> {
    let secs = crate::histdata::timeframe_secs(timeframe)?;
    if secs < 86_400 {
        return Ok(if stamp_close { ts - time::Duration::seconds(secs) } else { ts });
    }
    let utc = ts.to_offset(time::UtcOffset::UTC);
    let mut date = if !stamp_close && utc.time() == time::Time::MIDNIGHT {
        utc.date()
    } else {
        let t = if stamp_close { ts - time::Duration::seconds(1) } else { ts };
        t.to_timezone(tz).date()
    };
    if stamp_close && timeframe == "1w" {
        date -= time::Duration::days(i64::from(date.weekday().number_days_from_monday()));
    }
    let midnight = date.midnight();
    midnight
        .assume_timezone(tz)
        .take_first()
        .map(|d| d.to_offset(time::UtcOffset::UTC))
        .ok_or_else(|| anyhow!("{date} has no midnight in {}", tz.name()))
}

/// Every bar of one answer, at its opening instant, sorted, one per period.
pub(crate) fn parse_bars(spec: &Spec, body: &Value, timeframe: &str) -> Result<Vec<Bar>> {
    let rows = at_path(body, &spec.rows_path)?;
    parse_rows(rows, &spec.columns, spec.ts_unit, spec.stamp_close, spec.daily_tz, timeframe)
}

/// Rows already found in an answer (or posted to a webhook), read as bars through the file
/// import's reader.
pub(crate) fn parse_rows(
    rows: &Value,
    columns: &[(&'static str, Col)],
    ts_unit: TsUnit,
    stamp_close: bool,
    daily_tz: &'static Tz,
    timeframe: &str,
) -> Result<Vec<Bar>> {
    let grid = to_grid(rows, columns)?;
    let mapping = crate::histdata_import::Mapping {
        decimal: ".".into(),
        date_order: "ymd".into(),
        ts_unit: ts_unit.import_unit().into(),
        columns: (0..grid.width()).map(|i| (i.to_string(), grid.headers[i].clone())).collect(),
        ..Default::default()
    };
    let profiles: Vec<_> = (0..grid.width()).map(|i| crate::import::detect::profile_column(&grid, i)).collect();
    let built = crate::histdata_import::build(&grid, &mapping, &profiles);
    if let Some(first) = built.errors.first() {
        bail!(
            "{} of {} rows could not be read. Row {}: {}",
            built.errors.len(),
            grid.rows.len(),
            first.row,
            first.message
        );
    }
    let mut bars: Vec<Bar> = Vec::with_capacity(built.bars.len());
    for b in built.bars {
        let mut bar = b.bar;
        bar.ts = opened_at(bar.ts, timeframe, stamp_close, daily_tz)?;
        bars.push(bar);
    }
    bars.sort_by_key(|b| b.ts);
    bars.dedup_by(|later, earlier| {
        // `dedup_by` keeps the first of a run; the later row is the newer reading.
        if later.ts == earlier.ts {
            *earlier = later.clone();
            true
        } else {
            false
        }
    });
    Ok(bars)
}

// ── Where the request may go ─────────────────────────────────────────────────

/// Whether an address is one a user-typed URL must not reach by default: loopback,
/// link-local, private and unique-local ranges, the unspecified address, and an IPv4 one of
/// those wrapped in IPv6.
pub(crate) fn refused(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || o[0] == 0
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return refused(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

fn refusal(host: &str, ip: IpAddr) -> anyhow::Error {
    anyhow!(
        "refusing to fetch {host}: it resolves to {ip}, a loopback, link-local or private address. \
         Set allow_private_hosts to true on this connector to reach a server on your own network"
    )
}

/// Check where `url` goes before anything is sent, and return the address to pin the request
/// to (None for an IP literal, which needs no lookup, or when private hosts are allowed).
pub(crate) async fn check_host(url: &reqwest::Url, allow_private: bool) -> Result<Option<(String, SocketAddr)>> {
    if allow_private {
        return Ok(None);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let host = url.host_str().ok_or_else(|| anyhow!("the URL has no host"))?;
    // An IPv6 literal comes back bracketed.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        if refused(ip) {
            return Err(refusal(bare, ip));
        }
        return Ok(None);
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("cannot resolve {host}"))?
        .collect();
    let first = *addrs.first().ok_or_else(|| anyhow!("{host} resolves to no address"))?;
    if let Some(bad) = addrs.iter().find(|a| refused(a.ip())) {
        return Err(refusal(host, bad.ip()));
    }
    Ok(Some((host.to_string(), first)))
}

// ── The connector ────────────────────────────────────────────────────────────

pub struct HttpSource {
    cap: &'static Capability,
    /// The row's settings; the map a call receives (settings plus credentials) goes over it.
    config: HashMap<String, String>,
}

impl HttpSource {
    pub fn template() -> Self {
        HttpSource { cap: &TEMPLATE, config: HashMap::new() }
    }

    /// A minted connector. Settings that do not parse still give a connector, listed with what
    /// is wrong, so it can be fixed or deleted; every fetch refuses with the same reason.
    pub fn new(provider: &str, name: &str, config: HashMap<String, String>) -> Self {
        let label = if name.trim().is_empty() { TEMPLATE.label.to_string() } else { name.trim().to_string() };
        let spec = CapabilitySpec {
            provider: provider.to_string(),
            label,
            website: String::new(),
            docs_url: String::new(),
            rate_limit: String::new(),
            required_secrets: Vec::new(),
            asset_types: Vec::new(),
            timeframes: Vec::new(),
            max_bars_per_req: TEMPLATE.max_bars_per_req,
            min_interval_ms: TEMPLATE.min_interval_ms,
            searchable: false,
            testable: true,
            config_fields: FIELDS_DECL,
            fundamentals: false,
        };
        let spec = match Spec::parse(&config) {
            // A value source serves a Fundamentals series per symbol: no bars, no timeframes.
            Ok(s) if s.value.is_some() => CapabilitySpec {
                rate_limit: format!(
                    "Custom HTTP value source ({} ms between requests): its series are added from Fundamentals.",
                    s.min_interval_ms
                ),
                required_secrets: if s.auth.is_some() { vec![AUTH_SECRET.to_string()] } else { Vec::new() },
                min_interval_ms: s.min_interval_ms,
                searchable: !s.symbols.is_empty() || s.symbols_url.is_some(),
                fundamentals: true,
                ..spec
            },
            Ok(s) => CapabilitySpec {
                website: reqwest::Url::parse(&render(&s.url, &Vars::probe(), s.ts_unit, Enc::Url))
                    .ok()
                    .and_then(|u| u.host_str().map(|h| format!("{}://{h}", u.scheme())))
                    .unwrap_or_default(),
                rate_limit: format!(
                    "Custom HTTP source: {} bars per request, {} ms between requests.",
                    s.max_bars, s.min_interval_ms
                ),
                required_secrets: if s.auth.is_some() { vec![AUTH_SECRET.to_string()] } else { Vec::new() },
                asset_types: s.asset_types.clone(),
                timeframes: s.tf_map.keys().cloned().collect(),
                max_bars_per_req: s.max_bars,
                min_interval_ms: s.min_interval_ms,
                searchable: !s.symbols.is_empty() || s.symbols_url.is_some(),
                ..spec
            },
            Err(e) => CapabilitySpec { rate_limit: format!("Settings incomplete: {e:#}"), ..spec },
        };
        let cap = Capability::runtime(spec).unwrap_or(&TEMPLATE);
        HttpSource { cap, config }
    }

    /// The settings with the call's map (settings plus credentials, fresh from the store) over them.
    fn merged(&self, secrets: &HashMap<String, String>) -> HashMap<String, String> {
        let mut m = self.config.clone();
        m.extend(secrets.iter().map(|(k, v)| (k.clone(), v.clone())));
        m
    }

    fn spec(&self, cfg: &HashMap<String, String>) -> Result<Spec> {
        if self.cap.provider == KIND {
            bail!("a custom HTTP source is used through its own connector: create one first");
        }
        Spec::parse(cfg)
    }

    /// One request, answered as JSON.
    async fn request(&self, spec: &Spec, url: &str, body: Option<String>, cfg: &HashMap<String, String>) -> Result<Value> {
        let mut url = reqwest::Url::parse(url).map_err(|e| anyhow!("{}: the URL is not valid: {e}", self.cap.label))?;
        let secret = match &spec.auth {
            Some(_) => Some(cfg.get(AUTH_SECRET).filter(|v| !v.is_empty()).ok_or_else(|| {
                anyhow!("{} needs its {AUTH_SECRET} credential: set it on the connector", self.cap.label)
            })?),
            None => None,
        };
        if let (Some(Auth::Query(name)), Some(value)) = (&spec.auth, secret) {
            url.query_pairs_mut().append_pair(name, value);
        }
        let pin = check_host(&url, spec.allow_private_hosts).await?;
        let mut builder = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent("OpenTraderWorld/histdata")
            .redirect(reqwest::redirect::Policy::none());
        if let Some((host, addr)) = pin {
            builder = builder.resolve(&host, addr);
        }
        let client = builder.build().map_err(|e| anyhow!("building the http client: {e}"))?;
        let mut req = client.request(spec.method.clone(), url.clone());
        if let (Some(Auth::Header(name)), Some(value)) = (&spec.auth, secret) {
            req = req.header(name.as_str(), value.as_str());
        }
        if let Some(b) = body {
            req = req.header(reqwest::header::CONTENT_TYPE, "application/json").body(b);
        }
        // The credential may ride in the query string, and a reqwest error prints its URL.
        let resp = crate::rate::send(self.cap.provider, req)
            .await
            .map_err(|e| e.without_url())
            .with_context(|| format!("{} request", self.cap.label))?;
        let status = resp.status();
        if status.is_redirection() {
            let to = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("?");
            bail!("{} answered {status}, a redirect to {to}: redirects are not followed, put the final URL in the template", self.cap.label);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            // Kept as a reqwest error so the worker reads it as a wait, not a failure.
            return Err(resp.error_for_status().map_err(|e| e.without_url()).err().map(anyhow::Error::new).unwrap_or_else(|| anyhow!("{status}")))
                .with_context(|| format!("{} status", self.cap.label));
        }
        // Read under a cap: the endpoint is the user's, its answer size is not ours to trust.
        let mut resp = resp;
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(part) = resp.chunk().await.map_err(|e| e.without_url()).with_context(|| format!("{} body", self.cap.label))? {
            bytes.extend_from_slice(&part);
            if bytes.len() > MAX_BODY {
                bail!("{} answered more than {} MB: ask for fewer bars per request", self.cap.label, MAX_BODY / (1 << 20));
            }
        }
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            let text: String = text.chars().take(300).collect();
            bail!("{} answered {status}: {text}", self.cap.label);
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            let text: String = String::from_utf8_lossy(&bytes).chars().take(200).collect();
            anyhow!("{} did not answer JSON ({e}): {text}", self.cap.label)
        })
    }

    /// One request for `[from, to)`, every bar it carried, at its opening instant.
    #[allow(clippy::too_many_arguments)]
    async fn page(
        &self,
        spec: &Spec,
        cfg: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Vec<Bar>> {
        let token = spec.tf_map.get(timeframe).ok_or_else(|| {
            anyhow!("{} maps no token for {timeframe}: add it to tf_map", self.cap.label)
        })?;
        // A close-stamped source is asked for the stamps the window's bars carry.
        let shift = if spec.stamp_close { time::Duration::seconds(crate::histdata::timeframe_secs(timeframe)?) } else { time::Duration::ZERO };
        let vars = Vars { symbol: ticker, tf: token, asset_type, limit: spec.max_bars, from: from + shift, to: to + shift };
        let url = render(&spec.url, &vars, spec.ts_unit, Enc::Url);
        let body = spec.body.as_deref().map(|b| render(b, &vars, spec.ts_unit, Enc::Json));
        let json = self.request(spec, &url, body, cfg).await?;
        parse_bars(spec, &json, timeframe).with_context(|| format!("{} {ticker} {timeframe}", self.cap.label))
    }
}

/// Counts each request of a walk against a connector's declared quota, and stops the walk
/// with the reason once that quota is spent, the way a download parks on it.
pub(crate) struct Meter {
    pub pool: sqlx::PgPool,
    pub connector: uuid::Uuid,
}

impl Meter {
    async fn spend(&self, label: &str) -> Result<()> {
        if let Some(resets) = crate::fundamentals::skips::quota_spent(&self.pool, self.connector).await {
            bail!(
                "{label}: the connector's declared quota is spent until {}, the series was not fetched to the end",
                resets.format(&Rfc3339).unwrap_or_default()
            );
        }
        crate::fundamentals::bump(&self.pool, self.connector).await;
        Ok(())
    }
}

/// Where a value series with nothing stored starts: 5 years back at 1d, 10 at 1W, 30 at
/// 1M and coarser (and irregular). A walk from 1970 would not finish on a small page.
fn no_observation(label: &str, code: &str, missing: usize) -> anyhow::Error {
    if missing > 0 {
        anyhow!("{label} answered {missing} rows for {code}, none with a value: check the value column in Columns")
    } else {
        anyhow!("{label} answered no observation for {code}: check the code and rows_path")
    }
}

pub(crate) fn value_history_start(frequency: &str, now: OffsetDateTime) -> OffsetDateTime {
    let years: i64 = match frequency {
        "D" => 5,
        "W" => 10,
        _ => 30,
    };
    now - time::Duration::days(years * 365 + years / 4)
}

fn day_start(d: time::Date) -> OffsetDateTime {
    OffsetDateTime::new_utc(d, time::Time::MIDNIGHT)
}

impl HttpSource {
    /// One request of a value-shaped source for `[from, to)`, `{symbol}` the series code.
    async fn value_page(
        &self,
        spec: &Spec,
        vm: &super::value::ValueMapping,
        cfg: &HashMap<String, String>,
        code: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        missing: &std::sync::Mutex<std::collections::BTreeSet<time::Date>>,
    ) -> Result<Vec<super::value::Obs>> {
        let at = spec.asset_types.first().cloned().unwrap_or_else(|| "other".into());
        let vars = Vars { symbol: code, tf: "", asset_type: &at, limit: spec.max_bars, from, to };
        let url = render(&spec.url, &vars, vm.ts_unit, Enc::Url);
        let body = spec.body.as_deref().map(|b| render(b, &vars, vm.ts_unit, Enc::Json));
        let json = self.request(spec, &url, body, cfg).await?;
        let rows = at_path(&json, &spec.rows_path)?;
        let read = vm.read(rows).with_context(|| format!("{} {code}", self.cap.label))?;
        missing.lock().unwrap_or_else(|e| e.into_inner()).extend(read.missing);
        Ok(read.obs)
    }

    fn value_spec(&self, cfg: &HashMap<String, String>) -> Result<(Spec, super::value::ValueMapping)> {
        let spec = self.spec(cfg)?;
        let Some(vm) = spec.value.clone() else {
            bail!("{} serves candles, not a value series: set shape to value", self.cap.label);
        };
        Ok((spec, vm))
    }

    /// The observations of one series from `since` (its last stored period, else
    /// [`value_history_start`]) to now, walked page by page like a candle window
    /// ([`HttpSource::walk`]). A source whose request names no window is asked once and kept
    /// whole. An empty answer is an error naming the code, never an empty series.
    pub(crate) async fn fetch_observations(
        &self,
        secrets: &HashMap<String, String>,
        code: &str,
        since: Option<time::Date>,
        meter: Option<&Meter>,
    ) -> Result<(Vec<super::value::Obs>, usize)> {
        let cfg = self.merged(secrets);
        let (spec, vm) = self.value_spec(&cfg)?;
        let now = OffsetDateTime::now_utc();
        let from = match since {
            _ if !spec.windowed() => OffsetDateTime::UNIX_EPOCH,
            Some(d) => day_start(d),
            None => value_history_start(vm.frequency, now),
        };
        let missing = std::sync::Mutex::new(std::collections::BTreeSet::new());
        let got = self
            // Rows sit on dates: what follows the last one starts the next day, and a period
            // that may have been published since is asked for (one empty answer at most).
            .walk(&spec, code, "observations", from, now, (time::Duration::DAY, time::Duration::ZERO), meter, |o: &super::value::Obs| day_start(o.period_start), |f, t| {
                self.value_page(&spec, &vm, &cfg, code, f, t, &missing)
            })
            .await?;
        let mut missing = missing.into_inner().unwrap_or_else(|e| e.into_inner());
        if got.is_empty() {
            return Err(no_observation(&self.cap.label, code, missing.len()));
        }
        let obs: Vec<super::value::Obs> = got.into_values().collect();
        // Pages overlap: a period one left empty may carry a value in another.
        missing.retain(|d| obs.binary_search_by_key(d, |o| o.period_start).is_err());
        Ok((obs, missing.len()))
    }

    /// One request over the default history window, to check a series code exists before it
    /// is stored: the full walk is the refresh's.
    pub(crate) async fn probe_observations(&self, secrets: &HashMap<String, String>, code: &str) -> Result<Vec<super::value::Obs>> {
        let cfg = self.merged(secrets);
        let (spec, vm) = self.value_spec(&cfg)?;
        let now = OffsetDateTime::now_utc();
        let missing = std::sync::Mutex::new(std::collections::BTreeSet::new());
        let obs = self.value_page(&spec, &vm, &cfg, code, value_history_start(vm.frequency, now), now, &missing).await?;
        if obs.is_empty() {
            return Err(no_observation(&self.cap.label, code, missing.into_inner().unwrap_or_else(|e| e.into_inner()).len()));
        }
        Ok(obs)
    }

    /// The value mapping, when this source is value-shaped.
    pub(crate) fn value_mapping(&self) -> Option<super::value::ValueMapping> {
        Spec::parse(&self.config).ok().and_then(|s| s.value)
    }

    /// `[from, to)` walked until it is covered: a source that pages forward from `{from}`, or
    /// back from `{to}`, is asked again for the part its page left out, so a page smaller than
    /// the window is a second request rather than a hole (`step` and `slack` as in
    /// [`uncovered_span`]). Rows outside the asked part are
    /// dropped, and a row already held is not replaced. Requests keep `min_interval_ms`
    /// apart, each one is counted on `meter` when given, and a window still short after
    /// [`MAX_PAGES`] requests is an error naming the setting, never a truncated answer. A
    /// source whose request names no window is asked once.
    #[allow(clippy::too_many_arguments)]
    async fn walk<T, F, Fut>(
        &self,
        spec: &Spec,
        what: &str,
        rows: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        (step, slack): (time::Duration, time::Duration),
        meter: Option<&Meter>,
        at: impl Fn(&T) -> OffsetDateTime,
        mut page: F,
    ) -> Result<BTreeMap<OffsetDateTime, T>>
    where
        F: FnMut(OffsetDateTime, OffsetDateTime) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<T>>>,
    {
        let mut got: BTreeMap<OffsetDateTime, T> = BTreeMap::new();
        let mut pending = vec![(from, to)];
        let mut requests = 0usize;
        while let Some((f, t)) = pending.pop() {
            if requests >= MAX_PAGES {
                bail!(
                    "{}: {MAX_PAGES} requests did not cover {what} from {} to {}. \
                     The source answers fewer {rows} per request than max_bars ({}): lower it to its page size",
                    self.cap.label,
                    f.format(&Rfc3339).unwrap_or_default(),
                    t.format(&Rfc3339).unwrap_or_default(),
                    spec.max_bars
                );
            }
            if requests > 0 {
                tokio::time::sleep(Duration::from_millis(spec.min_interval_ms)).await;
            }
            if let Some(m) = meter {
                m.spend(&self.cap.label).await?;
            }
            requests += 1;
            let answer = page(f, t).await?;
            let fresh: Vec<T> = answer
                .into_iter()
                .filter(|r| {
                    let ts = at(r);
                    ts >= f && ts < t && !got.contains_key(&ts)
                })
                .collect();
            let (Some(first), Some(last)) = (fresh.iter().map(&at).min(), fresh.iter().map(&at).max()) else {
                continue;
            };
            if !spec.windowed() {
                got.extend(fresh.into_iter().map(|r| (at(&r), r)));
                break;
            }
            pending.extend(uncovered_span(first, last, f, t, step, slack));
            got.extend(fresh.into_iter().map(|r| (at(&r), r)));
        }
        Ok(got)
    }
}

/// The parts of `[from, to)` a page left uncovered, to ask for next. A side counts as covered
/// within one bar: the forming bar and a weekend are not holes worth a request.
#[cfg(test)]
pub(crate) fn uncovered(
    got: &[Bar],
    from: OffsetDateTime,
    to: OffsetDateTime,
    tf: time::Duration,
) -> Vec<(OffsetDateTime, OffsetDateTime)> {
    let (Some(first), Some(last)) = (got.first(), got.last()) else { return Vec::new() };
    uncovered_span(first.ts, last.ts, from, to, tf, tf)
}

/// The holes of `[from, to)` around a page whose earliest row sits at `first` and latest at
/// `last`, rows `step` apart. The tail is asked for once `slack` more than a row fits after
/// `last`: one bar for candles (the forming one streams live), none for value rows.
fn uncovered_span(
    first: OffsetDateTime,
    last: OffsetDateTime,
    from: OffsetDateTime,
    to: OffsetDateTime,
    step: time::Duration,
    slack: time::Duration,
) -> Vec<(OffsetDateTime, OffsetDateTime)> {
    let mut out = Vec::new();
    if first >= from + step {
        out.push((from, first));
    }
    if last + step + slack <= to {
        out.push((last + step, to));
    }
    out
}

#[async_trait::async_trait]
impl Connector for HttpSource {
    fn capability(&self) -> &'static Capability {
        self.cap
    }

    /// One window, walked until it is covered ([`HttpSource::walk`]).
    async fn fetch_chunk(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        let cfg = self.merged(secrets);
        let spec = self.spec(&cfg)?;
        if spec.value.is_some() {
            bail!(
                "{} is a value source: {ticker} is a Fundamentals series, add it there (it has no {timeframe} bars)",
                self.cap.label
            );
        }
        let tf = time::Duration::seconds(crate::histdata::timeframe_secs(timeframe)?);
        let what = format!("{ticker} {timeframe}");
        let got = self
            .walk(&spec, &what, "bars", from, to, (tf, tf), None, |b: &Bar| b.ts, |f, t| {
                self.page(&spec, &cfg, ticker, asset_type, timeframe, f, t)
            })
            .await?;
        Ok(Chunk::of(got.into_values().collect()))
    }

    async fn search_symbols(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let cfg = self.merged(secrets);
        let spec = self.spec(&cfg)?;
        let at = if asset_type.is_empty() { spec.asset_types[0].clone() } else { asset_type.to_string() };
        let hit = |symbol: String, name: String| SymbolHit { symbol, name, asset_type: at.clone(), exchange: self.cap.label.to_string() };
        if !spec.symbols.is_empty() {
            let all: Vec<SymbolHit> = spec.symbols.iter().map(|s| hit(s.clone(), String::new())).collect();
            return Ok(crate::histdata::filter_symbols(&all, query, limit));
        }
        let Some(url) = &spec.symbols_url else {
            bail!("{} has no symbol search: set Symbols or a Symbols URL, or type the symbol", self.cap.label);
        };
        let key = format!("{}:{url}", self.cap.provider);
        let all = match crate::histdata::cached_symbols(&key) {
            Some(all) => all,
            None => {
                // The list is read with a plain GET, whatever the bars endpoint takes.
                let json = self.request(&Spec { method: reqwest::Method::GET, ..spec.clone() }, url, None, &cfg).await?;
                let items = at_path(&json, &spec.symbols_path)?;
                let Value::Array(items) = items else {
                    bail!("{}: the symbols answer is not a list at {:?}", self.cap.label, spec.symbols_path);
                };
                let mut all = Vec::with_capacity(items.len());
                for it in items {
                    match it {
                        Value::String(s) if !s.trim().is_empty() => all.push(hit(s.trim().to_string(), String::new())),
                        Value::Object(o) => {
                            let Some(sym) = o.get(&spec.symbols_key).map(|v| cell(Some(v))).filter(|s| !s.is_empty()) else {
                                continue;
                            };
                            let name = o.get("name").map(|v| cell(Some(v))).unwrap_or_default();
                            all.push(hit(sym, name));
                        }
                        _ => {}
                    }
                }
                if all.is_empty() {
                    bail!("{}: the symbols list is empty (key {:?})", self.cap.label, spec.symbols_key);
                }
                crate::histdata::cache_symbols(&key, &all);
                all
            }
        };
        Ok(crate::histdata::filter_symbols(&all, query, limit))
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        Spec::parse(config).map(|_| ())
    }

    async fn test(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
        self.test_sample(client, secrets).await.map(|(detail, _)| detail)
    }

    /// The last few bars of the test symbol, parsed exactly as a download would store them.
    async fn test_sample(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<(String, Option<Value>)> {
        let cfg = self.merged(secrets);
        let spec = self.spec(&cfg)?;
        let symbol = spec
            .test_symbol
            .clone()
            .or_else(|| spec.symbols.first().cloned())
            .ok_or_else(|| anyhow!("set a test symbol (or a Symbols list) so the test knows what to ask for"))?;
        if spec.value.is_some() {
            let (obs, missing) = self.fetch_observations(secrets, &symbol, None, None).await?;
            let (first, last) = (obs[0], obs[obs.len() - 1]);
            let detail = format!(
                "{} observations of {symbol}, {} to {}{}",
                obs.len(),
                first.period_start,
                last.period_start,
                super::value::missing_note(missing)
            );
            let sample: Vec<Value> = obs
                .iter()
                .rev()
                .take(SAMPLE_BARS)
                .rev()
                .map(|o| {
                    json!({
                        "period_start": o.period_start.to_string(),
                        "released_at": o.released_at.and_then(|t| t.format(&Rfc3339).ok()),
                        "value": o.value,
                    })
                })
                .collect();
            return Ok((detail, Some(json!({ "symbol": symbol, "observations": sample }))));
        }
        let tf = if spec.tf_map.contains_key("1d") {
            "1d".to_string()
        } else {
            spec.tf_map.keys().max_by_key(|k| SUPPORTED_TIMEFRAMES.iter().position(|s| s == k)).cloned().unwrap_or_default()
        };
        let secs = crate::histdata::timeframe_secs(&tf)?;
        let to = OffsetDateTime::now_utc();
        let from = to - time::Duration::seconds(secs * TEST_BARS);
        let chunk = self.fetch_chunk(client, secrets, &symbol, &spec.asset_types[0], &tf, from, to).await?;
        let bars = chunk.bars;
        let (Some(first), Some(last)) = (bars.first(), bars.last()) else {
            bail!(
                "{symbol} {tf}: the source answered, but no bar falls in the last {TEST_BARS} periods. \
                 Check rows_path, the timestamp unit and the window placeholders"
            );
        };
        let fmt = |t: OffsetDateTime| t.format(&Rfc3339).unwrap_or_default();
        let detail = format!("{} bars of {symbol} at {tf}, {} to {}", bars.len(), fmt(first.ts), fmt(last.ts));
        let sample: Vec<Value> = bars
            .iter()
            .rev()
            .take(SAMPLE_BARS)
            .rev()
            .map(|b| json!({ "ts": fmt(b.ts), "open": b.open, "high": b.high, "low": b.low, "close": b.close, "volume": b.volume }))
            .collect();
        Ok((detail, Some(json!({ "symbol": symbol, "timeframe": tf, "bars": sample }))))
    }
}
