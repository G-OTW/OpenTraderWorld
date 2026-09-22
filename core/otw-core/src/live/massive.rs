//! Massive (Polygon.io) live aggregates.
//!
//! One socket per **market cluster** (`socket.massive.com/{stocks,options,crypto,forex,
//! indices,futures}`), the same split the REST side already has, and a staged handshake: the server
//! greets, `auth` is accepted only then, and `subscribe` only after `auth_success` — which is
//! what [`Feed::drain_sends`] is for.
//!
//! **Live is a paid feature here.** A free Massive key is end-of-day: it opens the socket and
//! the auth comes back refused. That is the single most likely outcome of a user pressing
//! "go live" with this provider, so it is classified [`LiveFault::Plan`] and reported as a
//! plan to upgrade rather than retried forever behind a dot that never turns green.
//!
//! Two conventions differ from the REST connector on purpose, because the vendor's do:
//! the aggregate channels are `AM.` (stocks, options, indices), `XA.` (crypto) and `CA.`
//! (forex), and a crypto/forex pair is written with a separator on the socket (`BTC-USD`,
//! `EUR/USD`) where the REST ticker glues it (`X:BTCUSD`, `C:EURUSD`). [`channel_for`]
//! is the one place that translation lives.
//!
//! **Futures are a cluster, not a separate host.** Their REST history is a service of its own
//! (`api.massive.com/futures/v1`, `1session` resolutions, nanosecond stamps), which is why
//! the socket was left out until the vendor's own documentation settled it: the WebSocket
//! side is `WS /futures/AM`, listed beside `WS /stocks/AM`, so it is this host with a
//! `futures` cluster path, the same `AM.<ticker>` channel, and the same `{ev,sym,s,o,h,l,c,v}`
//! payload with `s` in Unix milliseconds. The ticker carries its contract month (`6CH5`) and
//! takes no prefix. Aggregates are windowed in Central Time, which changes nothing here: the
//! fold anchors on the provider's own stamps.
//!
//! Streaming futures is its own entitlement (Futures Starter and up); a plan without it is
//! refused at subscribe and surfaces as [`LiveFault::Plan`] like every other Massive refusal.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

/// The socket host, following the REST one (`api.massive.com`) through the Polygon rebrand.
/// One constant on purpose: if the vendor splits the two hosts, this is the only line to move.
const HOST: &str = "wss://socket.massive.com";

/// Aggregate channels publish one bar a minute; anything coarser is folded from them.
const SOURCE_SECS: i64 = 60;
/// A minute aggregate is published a few seconds after the minute it covers, so the close
/// timer has to wait longer than it would for a trade feed.
const GRACE: i64 = 8;

/// Quote currencies a glued crypto pair can end in, longest first so `BTCUSDT` splits at
/// `USDT` rather than `USD`. Only used to un-glue a REST-style ticker; a pair the user already
/// wrote with a separator is left alone.
const CRYPTO_QUOTES: &[&str] = &[
    "USDT", "USDC", "TUSD", "BUSD", "USD", "EUR", "GBP", "JPY", "BTC", "ETH",
];

pub struct MassiveLive;

#[async_trait::async_trait]
impl StreamConnector for MassiveLive {
    /// One socket per market cluster: the vendor splits them by URL, so a stocks pane and a
    /// crypto pane are two connections whatever else is on screen.
    fn socket_class(&self, inst: &Instrument) -> Result<String> {
        Ok(channel_for(&inst.asset_type, &inst.symbol)?.0.to_string())
    }

    /// Aggregate channels publish one bar a minute; the hub folds them up to each pane's
    /// timeframe.
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Bars { secs: SOURCE_SECS, grace: GRACE })
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(channel_for(&inst.asset_type, &inst.symbol)?.1)
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        let key = req
            .secrets
            .get("api_key")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                LiveError::err(
                    LiveFault::Auth,
                    format!(
                        "connector {} has no Massive api_key , add it on the connector",
                        req.connector
                    ),
                )
            })?;
        let first = req
            .instruments
            .first()
            .ok_or_else(|| anyhow!("massive live: no instrument to open with"))?;
        let (cluster, _) = channel_for(&first.asset_type, &first.symbol)?;
        run_feed(
            MassiveFeed {
                cluster,
                key,
                connector: req.connector.to_string(),
                authed: false,
                pending: Vec::new(),
                sends: Vec::new(),
            },
            req.instruments,
        )
        .await
    }
}

/// The cluster socket and the channel string for one instrument.
pub fn channel_for(asset_type: &str, ticker: &str) -> Result<(&'static str, String)> {
    let t = ticker.trim();
    Ok(match asset_type {
        "equity" | "etf" => ("stocks", format!("AM.{}", t.to_ascii_uppercase())),
        "option" => (
            "options",
            format!("AM.O:{}", crate::histdata::parse_option_symbol(t)?.occ()),
        ),
        "index" => (
            "indices",
            format!("AM.I:{}", t.trim_start_matches("I:").to_ascii_uppercase()),
        ),
        "crypto" => ("crypto", format!("XA.{}", crypto_pair(t))),
        "fx" => ("forex", format!("CA.{}", fx_pair(t))),
        // The contract ticker as it is written (`ESZ5`, `6CH5`), no prefix: the futures
        // cluster names its instruments exactly the way its REST aggregates do.
        "future" => ("futures", format!("AM.{}", t.to_ascii_uppercase())),
        other => {
            return Err(LiveError::err(
                LiveFault::Unsupported,
                format!("Massive has no live feed for {other}"),
            ))
        }
    })
}

/// `BTCUSD` → `BTC-USD`; a pair already carrying a separator keeps it.
fn crypto_pair(t: &str) -> String {
    let raw = t.trim_start_matches("X:").to_ascii_uppercase();
    if raw.contains('-') || raw.contains('/') {
        return raw.replace('/', "-");
    }
    for q in CRYPTO_QUOTES {
        if raw.len() > q.len() && raw.ends_with(q) {
            let (base, quote) = raw.split_at(raw.len() - q.len());
            return format!("{base}-{quote}");
        }
    }
    raw
}

/// `EURUSD` → `EUR/USD`; a pair already carrying a separator keeps it.
fn fx_pair(t: &str) -> String {
    let raw = t.trim_start_matches("C:").to_ascii_uppercase();
    if raw.contains('/') || raw.contains('-') {
        return raw.replace('-', "/");
    }
    if raw.len() == 6 {
        let (base, quote) = raw.split_at(3);
        return format!("{base}/{quote}");
    }
    raw
}

struct MassiveFeed {
    cluster: &'static str,
    key: String,
    connector: String,
    /// Massive accepts `subscribe` only after `auth_success`, so instruments that join
    /// before that wait here.
    authed: bool,
    pending: Vec<String>,
    sends: Vec<String>,
}

impl Feed for MassiveFeed {
    fn url(&self) -> String {
        format!("{HOST}/{}", self.cluster)
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        let channels = self.channels(instruments);
        if channels.is_empty() {
            return Vec::new();
        }
        if !self.authed {
            self.pending.extend(channels);
            return Vec::new();
        }
        vec![json!({ "action": "subscribe", "params": channels.join(",") }).to_string()]
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        let channels = self.channels(instruments);
        if channels.is_empty() {
            return Vec::new();
        }
        self.pending.retain(|c| !channels.contains(c));
        if !self.authed {
            return Vec::new();
        }
        vec![json!({ "action": "unsubscribe", "params": channels.join(",") }).to_string()]
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("massive json: {e}"))?;
        let items = match &v {
            Value::Array(a) => a.clone(),
            other => vec![other.clone()],
        };
        let mut out = Vec::new();
        for item in items {
            match item.get("ev").and_then(Value::as_str) {
                Some("status") => {
                    if let Some(e) = self.on_status(&item) {
                        return Err(e);
                    }
                }
                // One aggregate event per cluster; the instrument is named by the payload
                // (`sym` for stocks/options/indices, `pair` for crypto and forex), which is
                // what the update is tagged with now that a socket carries several.
                Some(ev @ ("AM" | "XA" | "CA")) => out.push(self.bar(ev, &item)?),
                _ => {}
            }
        }
        Ok(out)
    }

    fn drain_sends(&mut self) -> Vec<String> {
        std::mem::take(&mut self.sends)
    }
}

impl MassiveFeed {
    /// Advance the handshake, or return the failure to end the session with.
    fn on_status(&mut self, item: &Value) -> Option<anyhow::Error> {
        let status = item.get("status").and_then(Value::as_str).unwrap_or("");
        let msg = item.get("message").and_then(Value::as_str).unwrap_or("").to_string();
        let c = &self.connector;
        match status {
            "connected" => {
                self.sends.push(json!({ "action": "auth", "params": self.key }).to_string());
                None
            }
            "auth_success" => {
                self.authed = true;
                let waiting = std::mem::take(&mut self.pending);
                if !waiting.is_empty() {
                    self.sends.push(
                        json!({ "action": "subscribe", "params": waiting.join(",") }).to_string(),
                    );
                }
                None
            }
            // The free key is the common case and it is not a typo in the key: Massive sells
            // the socket separately from the REST history, so say which of the two it is.
            "auth_failed" => Some(LiveError::err(
                LiveFault::Plan,
                format!(
                    "Massive refused the live connection for connector {c}. Live streaming is a \
                     paid Massive feature and is not part of the free (end-of-day) plan; the same \
                     key still downloads history ({msg})"
                ),
            )),
            "auth_timeout" => Some(LiveError::err(
                LiveFault::Auth,
                format!("Massive did not accept the login for connector {c} in time ({msg})"),
            )),
            "error" => Some(self.explain_error(&msg)),
            // "success" (subscribed), "max_connections" handled above as error text.
            _ => None,
        }
    }

    fn explain_error(&self, msg: &str) -> anyhow::Error {
        let low = msg.to_ascii_lowercase();
        let c = &self.connector;
        if low.contains("maximum number of websocket connections") || low.contains("max connection")
        {
            return LiveError::err(
                LiveFault::Conflict,
                format!(
                    "Massive allows one live connection per cluster and connector {c}'s key is \
                     already using it. Close the other program on this key and this reconnects on \
                     its own"
                ),
            );
        }
        if low.contains("not authorized") || low.contains("insufficient") || low.contains("upgrade")
        {
            return LiveError::err(
                LiveFault::Plan,
                format!("connector {c}'s Massive plan does not cover this live feed ({msg})"),
            );
        }
        if low.contains("unknown ticker") || low.contains("invalid ticker") {
            return LiveError::err(
                LiveFault::Symbol,
                format!("Massive does not know one of the subscribed symbols ({msg})"),
            );
        }
        LiveError::err(LiveFault::Transport, format!("Massive: {msg}"))
    }

    /// The channel names of these instruments, deduplicated: two timeframes on one symbol
    /// are one subscription on the wire.
    fn channels(&self, instruments: &[Instrument]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in instruments {
            // A symbol this cluster cannot express is refused by the subscription that asked
            // for it, not by the socket carrying the others.
            if let Ok((_, ch)) = channel_for(&i.asset_type, &i.symbol) {
                if !out.contains(&ch) {
                    out.push(ch);
                }
            }
        }
        out
    }

    /// One minute aggregate, tagged with the channel it belongs to. The fold up to the
    /// pane's timeframe is the hub's.
    fn bar(&self, ev: &str, b: &Value) -> Result<FeedUpdate> {
        let f = |k: &str| b.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let name = b
            .get("sym")
            .or_else(|| b.get("pair"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("massive aggregate symbol"))?;
        let start_ms = b
            .get("s")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("massive aggregate start"))?;
        Ok(FeedUpdate::bar(
            format!("{ev}.{name}"),
            LiveEvent {
                bar_ts: OffsetDateTime::from_unix_timestamp(start_ms / 1000)?,
                open: f("o"),
                high: f("h"),
                low: f("l"),
                close: f("c"),
                volume: f("v"),
                closed: true,
                provider_ts: OffsetDateTime::now_utc(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_market_has_its_own_cluster_and_prefix() {
        assert_eq!(channel_for("equity", "aapl").unwrap(), ("stocks", "AM.AAPL".into()));
        assert_eq!(channel_for("index", "I:SPX").unwrap(), ("indices", "AM.I:SPX".into()));
        assert_eq!(channel_for("crypto", "BTCUSD").unwrap(), ("crypto", "XA.BTC-USD".into()));
        assert_eq!(channel_for("fx", "EURUSD").unwrap(), ("forex", "CA.EUR/USD".into()));
        // Futures ride their own cluster, with the contract ticker unprefixed.
        assert_eq!(channel_for("future", "esz5").unwrap(), ("futures", "AM.ESZ5".into()));
        // A market the vendor has no cluster for is refused, not guessed at.
        assert!(channel_for("bond", "T 4.5 2030").is_err());
    }

    #[test]
    fn an_aggregate_is_tagged_with_the_channel_it_came_from() {
        let feed = MassiveFeed {
            cluster: "stocks",
            key: "k".into(),
            connector: "massive".into(),
            authed: true,
            pending: Vec::new(),
            sends: Vec::new(),
        };
        let stock: Value =
            serde_json::from_str(r#"{"ev":"AM","sym":"AAPL","s":1750000000000,"o":1,"c":2}"#)
                .unwrap();
        assert_eq!(feed.bar("AM", &stock).unwrap().channel, "AM.AAPL");
        // Crypto and forex name the instrument `pair`, not `sym`.
        let pair: Value =
            serde_json::from_str(r#"{"ev":"XA","pair":"BTC-USD","s":1750000000000,"o":1,"c":2}"#)
                .unwrap();
        assert_eq!(feed.bar("XA", &pair).unwrap().channel, "XA.BTC-USD");
    }

    #[test]
    fn subscriptions_wait_for_the_auth_answer() {
        let mut feed = MassiveFeed {
            cluster: "stocks",
            key: "k".into(),
            connector: "massive".into(),
            authed: false,
            pending: Vec::new(),
            sends: Vec::new(),
        };
        assert!(feed.subscribe(&[Instrument::new("AAPL", "equity", "1m")]).is_empty());
        feed.on_message(r#"[{"ev":"status","status":"auth_success"}]"#).unwrap();
        let sent = feed.drain_sends();
        assert!(sent[0].contains("subscribe") && sent[0].contains("AM.AAPL"));
    }

    #[test]
    fn a_glued_pair_splits_at_the_longest_quote() {
        assert_eq!(crypto_pair("BTCUSDT"), "BTC-USDT");
        assert_eq!(crypto_pair("X:ETHUSD"), "ETH-USD");
        // Already separated: left alone (normalized to the socket's separator).
        assert_eq!(crypto_pair("SOL/USD"), "SOL-USD");
        assert_eq!(fx_pair("C:GBPJPY"), "GBP/JPY");
        assert_eq!(fx_pair("EUR-USD"), "EUR/USD");
    }
}
