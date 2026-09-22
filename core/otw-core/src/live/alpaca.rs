//! Alpaca live market data (v2 stocks, v1beta3 crypto, v1beta1 options).
//!
//! Alpaca publishes **one-minute bars**, not a channel per interval, so the timeframe on the
//! chart is built by [`super::agg::Bucketer`] and the finest live timeframe is a minute. A
//! *daily* candle is deliberately not offered: an equity session is not 1440 epoch-aligned
//! minutes, so folding minutes into a day would produce a bar that disagrees with the one the
//! REST history stores.
//!
//! The handshake is staged and that is why [`Feed::drain_sends`] exists: Alpaca greets with
//! `success/connected`, accepts `auth` only then, and `subscribe` only after
//! `success/authenticated`. Sending all three up front works by accident on a good day and
//! fails on a slow one.
//!
//! **The feed the connector is entitled to is a setting, not a guess** (`feed`, on the
//! connector). Alpaca sells the same API at two entitlements — free keys get IEX and the
//! indicative options feed, paid ones SIP and OPRA — and asking for the one you do not have
//! is a `409 insufficient subscription` rather than thinner data. The setting is read in the
//! clear, like the IB gateway address, because an entitlement you cannot see is an
//! entitlement you cannot debug.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

/// Alpaca streams one-minute bars; everything coarser is folded from them.
const SOURCE_SECS: i64 = 60;
/// A minute bar lands within a second or two of the minute it closes; wait a little longer
/// than the trade feeds do before the timer gives up on it.
const GRACE: i64 = 4;

pub struct AlpacaLive;

#[async_trait::async_trait]
impl StreamConnector for AlpacaLive {
    /// One socket per market: Alpaca publishes equities, crypto and options on three
    /// different URLs, so those are three connections however many symbols ride each.
    fn socket_class(&self, inst: &Instrument) -> Result<String> {
        Ok(market_of(&inst.asset_type)?.to_string())
    }

    /// Alpaca publishes one-minute bars, not a channel per interval, so every timeframe on
    /// the chart is folded from them by the hub.
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Bars { secs: SOURCE_SECS, grace: GRACE })
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(match inst.asset_type.as_str() {
            // The OCC symbol, parsed rather than passed through, so a malformed contract
            // fails here instead of subscribing to a channel that will never speak.
            "option" => crate::histdata::parse_option_symbol(&inst.symbol)?.occ(),
            _ => inst.symbol.clone(),
        })
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        let key = need(req.secrets, "api_key", req.connector)?;
        let secret = need(req.secrets, "api_secret", req.connector)?;
        let feed = feed_of(req.secrets)?;
        let first = req
            .instruments
            .first()
            .ok_or_else(|| anyhow!("alpaca live: no instrument to open with"))?;
        run_feed(
            AlpacaFeed {
                url: url_for(&first.asset_type, feed)?,
                key,
                secret,
                connector: req.connector.to_string(),
                feed,
                authed: false,
                pending: Vec::new(),
                sends: Vec::new(),
            },
            req.instruments,
        )
        .await
    }
}

/// The market an asset type streams on, which is also this connector's socket class.
fn market_of(asset_type: &str) -> Result<&'static str> {
    Ok(match asset_type {
        "crypto" => "crypto",
        "option" => "options",
        "equity" | "etf" => "stocks",
        other => {
            return Err(LiveError::err(
                LiveFault::Unsupported,
                format!("Alpaca has no live feed for {other}"),
            ))
        }
    })
}

/// Which entitlement this connector is on. One dial rather than one per asset class: Alpaca
/// sells them together (a free key is IEX *and* indicative options, a paid one SIP *and*
/// OPRA), so two fields could only ever be set to an impossible pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entitlement {
    /// Free plan: IEX equities, indicative options.
    Iex,
    /// Paid plan: full SIP tape, OPRA options.
    Sip,
}

impl Entitlement {
    fn label(self) -> &'static str {
        match self {
            Entitlement::Iex => "IEX",
            Entitlement::Sip => "SIP",
        }
    }
}

/// Parse the connector's `feed` setting. Absent = the free entitlement, which is what a key
/// created today has.
pub fn feed_of(settings: &HashMap<String, String>) -> Result<Entitlement> {
    match settings.get("feed").map(|f| f.trim().to_lowercase()).unwrap_or_default().as_str() {
        "" | "iex" => Ok(Entitlement::Iex),
        "sip" => Ok(Entitlement::Sip),
        other => Err(anyhow!(
            "{other:?} is not an Alpaca feed. Use iex (free plan) or sip (paid plan)"
        )),
    }
}

fn url_for(asset_type: &str, feed: Entitlement) -> Result<String> {
    let base = "wss://stream.data.alpaca.markets";
    Ok(match (asset_type, feed) {
        ("crypto", _) => format!("{base}/v1beta3/crypto/us"),
        ("option", Entitlement::Iex) => format!("{base}/v1beta1/indicative"),
        ("option", Entitlement::Sip) => format!("{base}/v1beta1/opra"),
        ("equity" | "etf", Entitlement::Iex) => format!("{base}/v2/iex"),
        ("equity" | "etf", Entitlement::Sip) => format!("{base}/v2/sip"),
        (other, _) => {
            return Err(LiveError::err(
                LiveFault::Unsupported,
                format!("Alpaca has no live feed for {other}"),
            ))
        }
    })
}

fn need(secrets: &HashMap<String, String>, name: &str, connector: &str) -> Result<String> {
    secrets
        .get(name)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            LiveError::err(
                LiveFault::Auth,
                format!("connector {connector} has no Alpaca {name} — add it on the connector"),
            )
        })
}

struct AlpacaFeed {
    url: String,
    key: String,
    secret: String,
    connector: String,
    feed: Entitlement,
    /// Alpaca accepts `subscribe` only after it has answered `auth`, so an instrument that
    /// joins before the handshake completes waits here rather than being dropped.
    authed: bool,
    pending: Vec<String>,
    /// Frames queued by the handshake, drained by the driver.
    sends: Vec<String>,
}

impl Feed for AlpacaFeed {
    fn url(&self) -> String {
        self.url.clone()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        let symbols = self.symbols(instruments);
        if symbols.is_empty() {
            return Vec::new();
        }
        if !self.authed {
            self.pending.extend(symbols);
            return Vec::new();
        }
        vec![json!({ "action": "subscribe", "bars": symbols }).to_string()]
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        let symbols = self.symbols(instruments);
        if symbols.is_empty() {
            return Vec::new();
        }
        self.pending.retain(|s| !symbols.contains(s));
        if !self.authed {
            return Vec::new();
        }
        vec![json!({ "action": "unsubscribe", "bars": symbols }).to_string()]
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        // Every Alpaca frame is an array, control messages included.
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("alpaca json: {e}"))?;
        let items = match &v {
            Value::Array(a) => a.clone(),
            other => vec![other.clone()],
        };
        let mut out = Vec::new();
        for item in items {
            match item.get("T").and_then(Value::as_str) {
                Some("success") => match item.get("msg").and_then(Value::as_str) {
                    Some("connected") => self.sends.push(
                        json!({ "action": "auth", "key": self.key, "secret": self.secret })
                            .to_string(),
                    ),
                    Some("authenticated") => {
                        self.authed = true;
                        let waiting = std::mem::take(&mut self.pending);
                        if !waiting.is_empty() {
                            self.sends.push(
                                json!({ "action": "subscribe", "bars": waiting }).to_string(),
                            );
                        }
                    }
                    _ => {}
                },
                Some("error") => return Err(self.explain(&item)),
                Some("b") => out.push(self.bar(&item)?),
                // `u` is a correction to a bar already published and `d` is the daily bar:
                // neither can move a candle the chart has already closed, and the REST seed
                // is what reconciles a bar that was wrong. Ignored on purpose.
                _ => {}
            }
        }
        Ok(out)
    }

    fn drain_sends(&mut self) -> Vec<String> {
        std::mem::take(&mut self.sends)
    }
}

impl AlpacaFeed {
    /// The channel names of these instruments, deduplicated: two timeframes on one symbol
    /// are one subscription on the wire.
    fn symbols(&self, instruments: &[Instrument]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in instruments {
            let s = match i.asset_type.as_str() {
                "option" => match crate::histdata::parse_option_symbol(&i.symbol) {
                    Ok(o) => o.occ(),
                    // A contract we cannot parse is one Alpaca would never answer for; the
                    // subscription that asked for it fails on its own coordinates.
                    Err(_) => continue,
                },
                _ => i.symbol.clone(),
            };
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// One minute bar, tagged with the symbol it belongs to. The fold up to the pane's
    /// timeframe happens in the hub, which is what lets one socket serve several timeframes
    /// of the same symbol.
    fn bar(&self, b: &Value) -> Result<FeedUpdate> {
        let f = |k: &str| b.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let symbol = b
            .get("S")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("alpaca bar symbol"))?
            .to_string();
        let ts = b
            .get("t")
            .and_then(Value::as_str)
            .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
            .ok_or_else(|| anyhow!("alpaca bar time"))?;
        Ok(FeedUpdate::bar(
            symbol,
            LiveEvent {
                bar_ts: ts,
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

    /// Turn Alpaca's numbered error into the sentence that names the fix. The code matters
    /// more than the text: 402 is a key to correct, 409 is a plan to upgrade and 406 is
    /// another program holding the only connection the free plan allows — three different
    /// actions that Alpaca's own `msg` does not distinguish for a reader.
    fn explain(&self, item: &Value) -> anyhow::Error {
        let code = item.get("code").and_then(Value::as_i64).unwrap_or(0);
        let msg = item.get("msg").and_then(Value::as_str).unwrap_or("").to_string();
        let c = &self.connector;
        let (fault, message) = match code {
            401 | 404 => (
                LiveFault::Auth,
                format!("Alpaca did not accept the login on connector {c} ({msg})"),
            ),
            402 => (
                LiveFault::Auth,
                format!("Alpaca rejected the API key on connector {c}. Check the key and secret, and that they are live keys rather than paper ones"),
            ),
            403 => (LiveFault::Transport, format!("Alpaca: {msg}")),
            405 => (
                LiveFault::Unsupported,
                format!("Alpaca refused the subscription: {msg}"),
            ),
            406 => (
                LiveFault::Conflict,
                format!("Alpaca allows one live connection per account and it is already in use. Close the other program (or browser tab) using connector {c}'s key, and this reconnects on its own"),
            ),
            408 => (
                LiveFault::Plan,
                format!("connector {c}'s Alpaca account has no market-data streaming enabled"),
            ),
            409 => (
                LiveFault::Plan,
                format!("connector {c}'s Alpaca plan does not include the {} feed. Set the connector's Feed setting to iex, or upgrade the plan", self.feed.label()),
            ),
            410 => (
                LiveFault::Unsupported,
                format!("Alpaca's {} feed does not carry this instrument", self.feed.label()),
            ),
            _ => (LiveFault::Transport, format!("Alpaca error {code}: {msg}")),
        };
        LiveError::err(fault, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_follows_the_entitlement_and_the_asset() {
        assert!(url_for("equity", Entitlement::Iex).unwrap().ends_with("/v2/iex"));
        assert!(url_for("etf", Entitlement::Sip).unwrap().ends_with("/v2/sip"));
        assert!(url_for("option", Entitlement::Iex).unwrap().ends_with("/indicative"));
        assert!(url_for("option", Entitlement::Sip).unwrap().ends_with("/opra"));
        // Crypto has no tiers: one socket whatever the plan.
        assert!(url_for("crypto", Entitlement::Iex).unwrap().ends_with("/v1beta3/crypto/us"));
        assert!(url_for("future", Entitlement::Sip).is_err());
    }

    #[test]
    fn an_instrument_that_joins_before_the_handshake_waits_for_it() {
        let mut feed = AlpacaFeed {
            url: "wss://x".into(),
            key: "k".into(),
            secret: "s".into(),
            connector: "alpaca".into(),
            feed: Entitlement::Iex,
            authed: false,
            pending: Vec::new(),
            sends: Vec::new(),
        };
        // Opening set: nothing on the wire yet, Alpaca has not answered `auth`.
        assert!(feed.subscribe(&[Instrument::new("AAPL", "equity", "1m")]).is_empty());
        feed.on_message(r#"[{"T":"success","msg":"connected"}]"#).unwrap();
        assert!(feed.drain_sends()[0].contains("auth"));
        feed.on_message(r#"[{"T":"success","msg":"authenticated"}]"#).unwrap();
        let sent = feed.drain_sends();
        assert!(sent[0].contains("subscribe") && sent[0].contains("AAPL"));
        // A pane joining after the handshake goes straight out.
        let now = feed.subscribe(&[Instrument::new("MSFT", "equity", "5m")]);
        assert!(now[0].contains("MSFT"));
    }

    #[test]
    fn two_timeframes_of_one_symbol_are_one_subscription() {
        let feed = AlpacaFeed {
            url: "wss://x".into(),
            key: "k".into(),
            secret: "s".into(),
            connector: "alpaca".into(),
            feed: Entitlement::Iex,
            authed: true,
            pending: Vec::new(),
            sends: Vec::new(),
        };
        let symbols = feed.symbols(&[
            Instrument::new("AAPL", "equity", "1m"),
            Instrument::new("AAPL", "equity", "1h"),
        ]);
        assert_eq!(symbols, vec!["AAPL".to_string()]);
    }

    #[test]
    fn a_bar_is_tagged_with_its_own_symbol() {
        let feed = AlpacaFeed {
            url: "wss://x".into(),
            key: "k".into(),
            secret: "s".into(),
            connector: "alpaca".into(),
            feed: Entitlement::Iex,
            authed: true,
            pending: Vec::new(),
            sends: Vec::new(),
        };
        let item: Value = serde_json::from_str(
            r#"{"T":"b","S":"MSFT","t":"2026-09-05T14:31:00Z","o":1,"h":2,"l":0.5,"c":1.5,"v":9}"#,
        )
        .unwrap();
        assert_eq!(feed.bar(&item).unwrap().channel, "MSFT");
    }

    #[test]
    fn the_feed_setting_defaults_to_the_free_one() {
        let empty = HashMap::new();
        assert_eq!(feed_of(&empty).unwrap(), Entitlement::Iex);
        let sip = HashMap::from([("feed".into(), " SIP ".into())]);
        assert_eq!(feed_of(&sip).unwrap(), Entitlement::Sip);
        assert!(feed_of(&HashMap::from([("feed".into(), "nasdaq".into())])).is_err());
    }
}
