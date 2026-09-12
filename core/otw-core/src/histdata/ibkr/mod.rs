//! Interactive Brokers, through a local IB Gateway or TWS.
//!
//! The odd one out among the connectors: there is no vendor URL and no API key. The user
//! runs IB Gateway (or TWS) on their own machine, ticks "Enable ActiveX and Socket
//! Clients", and this connector speaks the TWS socket protocol to it. What a connector
//! carries is therefore an **address**, not a credential: a host and a port, entered on
//! the connector and stored in the clear, because an address that cannot be read back
//! cannot be diagnosed. The client id is ours (see [`session`]) and is never asked for.
//!
//! Everything the app already does with a provider still applies: the download worker
//! chunks and pages, the quota is billed per request, the chart preview and the symbol
//! search go through the same connector. Only the transport differs.

pub(crate) mod contract;
pub(crate) mod resolve;
pub(crate) mod session;

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use ibapi::market_data::historical::Duration as IbDuration;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, ConfigField, Connector, SymbolHit};

pub struct Ibkr;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "host",
        label: "Gateway host",
        placeholder: "host.docker.internal",
        kind: "text",
        required: true,
        help: "Where IB Gateway or TWS is running. OpenTraderWorld runs in a container, so its own \
               localhost is not yours: use host.docker.internal for a gateway on this machine.",
    },
    ConfigField {
        name: "port",
        label: "API port",
        placeholder: "4002",
        kind: "number",
        required: true,
        help: "The port the gateway listens on: 4001 (IB Gateway, live), 4002 (IB Gateway, paper), \
               7496 (TWS, live), 7497 (TWS, paper).",
    },
];

static IBKR: Capability = Capability {
    provider: "ibkr",
    label: "Interactive Brokers",
    website: "https://www.interactivebrokers.com",
    docs_url: "https://interactivebrokers.github.io/tws-api/historical_bars.html",
    rate_limit: "Runs against your own IB Gateway or TWS, so there is no key and no vendor plan: \
                 the data is whatever your IB account is subscribed to, and an instrument you have \
                 no market-data subscription for returns an error rather than bars. Interactive \
                 Brokers allows 60 historical requests per rolling 10 minutes per account, which \
                 this connector paces for you, so a long backfill is slow by design. Futures and \
                 options need their own exchange subscription, and a contract you are not \
                 subscribed to reports that rather than returning empty. The gateway \
                 restarts itself once a day and the connection is re-opened on its own. Volume is \
                 reported as Interactive Brokers sends it.",
    required_secrets: &[],
    asset_types: contract::ASSET_TYPES,
    timeframes: &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"],
    // Historical bars come back split-adjusted, folded straight into OHLC rather than as a
    // separate series, so there is no distinct adjusted close to store.
    adjusted: false,
    // One worker chunk; `fetch_chunk` slices it again into windows IB accepts (see
    // `contract::max_window_secs`), so this number is about progress granularity only.
    max_bars_per_req: 2000,
    // The real limiter is the rolling window in `session`; this only keeps the worker from
    // queueing chunks back to back.
    min_interval_ms: 1_500,
    searchable: true,
    config_fields: FIELDS,
    testable: true,
    // Whatever the account is subscribed to, which is the same set the history covers.
    stream_asset_types: contract::ASSET_TYPES,
    // Live candles are folded from tick-by-tick prints, or from the five-second stream where
    // IB serves no ticks. Daily and weekly work too: their buckets are anchored on the
    // session stamp IB's own bars carry.
    stream_timeframes: &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are built from Interactive Brokers' tick-by-tick prints, so \
                  every timeframe works and the price moves on the trade. Options and \
                  non-CME indices have no tick stream at IB and fall back to its five-second \
                  bars. It needs a live market-data subscription on the instrument (delayed \
                  data is not a live feed), IB allows one live session per account (a running \
                  TWS or mobile app holds the seat), and it allows only as many simultaneous \
                  tick streams as the account has market-depth lines, a handful on a plain \
                  account.",
};

/// Safety valve on the inner slicing loop: a worker chunk should need a handful of IB
/// requests, never an unbounded number.
const MAX_SLICES: usize = 24;

#[async_trait::async_trait]
impl Connector for Ibkr {
    fn capability(&self) -> &'static Capability {
        &IBKR
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        session::address(config).map(|_| ())
    }

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
        let size = contract::bar_size(timeframe)?;
        let tf_secs = crate::histdata::timeframe_secs(timeframe)?;
        let what = contract::what_to_show(asset_type);
        let hours = contract::trading_hours(asset_type);
        let max_window = time::Duration::seconds(contract::max_window_secs(timeframe)?);
        let session = session::get(secrets).await?;
        // Which instrument this is, settled before the first bar is asked for: a futures
        // ticker names a listing the gateway has to confirm, and a job that dies on its
        // first chunk has already cost the user their place in the queue.
        let contract = resolve::contract_for(&session, ticker, asset_type).await?;

        // The worker's chunk is sized in bars; IB's limit is a duration that tightens as
        // the bar gets finer. Walk the chunk in windows IB accepts rather than let it
        // refuse the whole request, which would advance the worker's cursor over a gap.
        let mut bars = Vec::new();
        let mut cursor = from;
        let mut slices = 0;
        while cursor < to {
            slices += 1;
            if slices > MAX_SLICES {
                // The worker advances its cursor by the chunk it asked for, so a short
                // answer here would leave a hole nobody notices. Refuse instead.
                return Err(anyhow!(
                    "{ticker} {timeframe}: this window needs more than {MAX_SLICES} Interactive \
                     Brokers requests, download a shorter range"
                ));
            }
            let slice_to = (cursor + max_window).min(to);
            // IB asks for an end plus a duration, never a start: the slice is expressed as
            // "this much time back from `slice_to`".
            let span = duration_back((slice_to - cursor).whole_seconds(), tf_secs);
            let (c, w, h, s) = (contract.clone(), what, hours, size);
            let data = session
                .call(&format!("{ticker} {timeframe}"), move |client| {
                    let c = c.clone();
                    async move {
                        client.historical_data(&c, Some(slice_to), span, s, Some(w), h).await
                    }
                })
                .await?;
            bars.extend(data.bars.iter().map(to_bar));
            cursor = slice_to;
        }
        // A bar can land in two adjacent windows; the store upserts, but sorted, unique
        // input keeps the written count honest.
        bars.sort_by_key(|b: &Bar| b.ts);
        bars.dedup_by_key(|b| b.ts);
        Ok(Chunk { bars })
    }

    async fn search_symbols(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let needle = query.trim().to_string();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let session = session::get(secrets).await?;
        // Futures are not in IB's symbol samples: `reqMatchingSymbols` answers with
        // underlyings, so a root came back typed as the index it tracks and a dated local
        // symbol (`MNQU6`) came back with nothing at all. The contract database is the list
        // that holds them, and it is asked directly.
        if asset_type == "future" {
            return search_futures(&session, &needle, limit).await;
        }
        let hits = session
            .call("symbol search", move |client| {
                let needle = needle.clone();
                async move { client.matching_symbols(&needle).await }
            })
            .await?;
        Ok(hits
            .into_iter()
            .filter_map(|d| {
                let kind = contract::asset_type_of(&d.contract.security_type)?;
                // The caller may be asking for one asset type; ETFs and shares are both
                // STK to IB, so an "etf" filter keeps the equities rather than emptying.
                let wanted = match asset_type {
                    "" => true,
                    "etf" => kind == "equity",
                    other => other == kind,
                };
                let exchange = if d.contract.primary_exchange.0.is_empty() {
                    d.contract.exchange.0.clone()
                } else {
                    d.contract.primary_exchange.0.clone()
                };
                wanted.then(|| SymbolHit {
                    symbol: contract::format_ticker(
                        kind,
                        &d.contract.symbol.0,
                        &d.contract.currency.0,
                        &exchange,
                    ),
                    name: d.contract.local_symbol.clone(),
                    asset_type: kind.to_string(),
                    exchange,
                })
            })
            .take(limit)
            .collect())
    }

    async fn test(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let session = session::get(secrets).await?;
        // A fresh handshake is the point of the test: reuse would report a socket opened
        // minutes ago and hide the setting the user just changed.
        session.reset().await;
        let (client, client_id) = session.client().await?;
        let when = client
            .connection_time()
            .map(|t| t.date().to_string())
            .unwrap_or_else(|| "unknown".into());
        Ok(format!(
            "Connected to {} as client {client_id}. TWS API server version {}, gateway date {when}.",
            session.addr(),
            client.server_version()
        ))
    }
}

/// A futures search: one `reqContractDetails` over the root, listing the months IB lists.
///
/// The same round trip accepts IB's own local symbol, so `MNQ` and `MNQU6` both answer, and
/// every hit is written back in the dated form this module parses again (`MNQ.20260918@CME`),
/// never as a bare root that would leave the exchange to a guess.
async fn search_futures(
    session: &session::Session,
    needle: &str,
    limit: usize,
) -> Result<Vec<SymbolHit>> {
    // A half-typed ticker is not an error to show under the search box.
    let Ok(mut partial) = contract::build(needle, "future") else {
        return Ok(Vec::new());
    };
    // With no month a root builds the continuous series, which is one contract and not a
    // list; the dated contracts are a different security type to IB.
    if partial.security_type == ibapi::contracts::SecurityType::ContinuousFuture {
        partial.security_type = ibapi::contracts::SecurityType::Future;
    }
    let details = match session
        .call("futures search", move |client| {
            let partial = partial.clone();
            async move { client.contract_details(&partial).await }
        })
        .await
    {
        Ok(d) => d,
        Err(e) if session::unknown_contract(&e) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    // IB answers once per trading class, so the same contract can come back several times;
    // identity is the contract id. Sorted by month: the front contract is what is wanted.
    let mut found: Vec<ibapi::contracts::Contract> =
        details.into_iter().map(|d| d.contract).collect();
    found.sort_by(|a, b| {
        (&a.last_trade_date_or_contract_month, &a.exchange.0, a.contract_id).cmp(&(
            &b.last_trade_date_or_contract_month,
            &b.exchange.0,
            b.contract_id,
        ))
    });
    found.dedup_by_key(|c| c.contract_id);
    Ok(found
        .iter()
        .take(limit)
        .map(|c| SymbolHit {
            symbol: contract::label(c),
            // What TWS shows for that month, so the row reads the way the user copied it.
            name: c.local_symbol.clone(),
            asset_type: "future".to_string(),
            exchange: c.exchange.0.clone(),
        })
        .collect())
}

/// The fewest bars a request is allowed to ask for.
///
/// Interactive Brokers validates the duration **against the bar size** and refuses a window
/// too short for the bars asked ("Historical data requested duration is invalid", error
/// 321). A top-up of the last few missing candles is exactly that request, so the window is
/// widened backwards until it is worth making. It costs nothing: the extra bars are ones we
/// already hold, deduped here and re-written as an upsert, and the caller keeps the window
/// it asked for.
const MIN_BARS_PER_REQ: i64 = 30;

/// How far back to ask, in the unit Interactive Brokers accepts for that span and bar size.
///
/// The unit is part of what IB validates. A duration written in **seconds** is refused past
/// 86 400 whatever the bar size ("Historical data request for greater than 86400 seconds
/// rejected", error 321), so a month of hourly bars has to be asked for as days rather than
/// as 2 592 000 seconds; seconds are also refused for a bar coarser than half an hour, for
/// which IB counts in days whatever the span. Days are rounded up and carry past the
/// slice's own start, which costs an overlapping bar or two: they are deduped below, and
/// asking short would leave a hole instead.
fn duration_back(secs: i64, bar_secs: i64) -> IbDuration {
    const DAY: i64 = 86_400;
    /// The coarsest bar IB still serves against a duration written in seconds.
    const SECONDS_MAX_BAR: i64 = 1_800;
    let bar_secs = bar_secs.max(1);
    let secs = secs.max(bar_secs * MIN_BARS_PER_REQ).max(60);
    if secs <= DAY && bar_secs <= SECONDS_MAX_BAR {
        return IbDuration::seconds(secs as i32);
    }
    let days = (secs + DAY - 1) / DAY;
    // A duration in days is itself capped at a year; beyond that IB counts in years.
    if days <= 365 {
        IbDuration::days(days as i32)
    } else {
        IbDuration::years(((days + 364) / 365) as i32)
    }
}

/// One IB bar as the store wants it.
///
/// The stamp is kept exactly as Interactive Brokers sent it. Daily and coarser bars carry a
/// trading date, which the decoder already resolves to midnight of that day; intraday bars
/// keep their own instant. Normalizing is the alignment layer's job on read, and rewriting
/// a provider's stamp on the way in is precisely what that layer exists to avoid.
fn to_bar(b: &ibapi::market_data::historical::Bar) -> Bar {
    Bar {
        ts: b.date,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        // IB reports -1 when a series has no volume (cash forex, midpoint bars).
        volume: b.volume.max(0.0),
        adj_open: None,
        adj_high: None,
        adj_low: None,
        adj_close: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connector_with_no_address_is_refused_before_it_is_stored() {
        let cfg: HashMap<String, String> = HashMap::new();
        assert!(Ibkr.validate_config(&cfg).is_err());
        let cfg = HashMap::from([
            ("host".to_string(), "127.0.0.1".to_string()),
            ("port".to_string(), "4002".to_string()),
        ]);
        assert!(Ibkr.validate_config(&cfg).is_ok());
    }

    #[test]
    fn the_connector_asks_for_an_address_and_no_key() {
        assert!(IBKR.required_secrets.is_empty());
        let names: Vec<_> = IBKR.config_fields.iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["host", "port"]);
        assert!(IBKR.config_fields.iter().all(|f| f.required));
    }

    #[test]
    fn a_window_is_asked_for_in_a_unit_ib_accepts() {
        // Seconds are legal up to a day for a bar IB serves that way, and exactly a day is
        // still legal.
        assert_eq!(duration_back(3_600, 60).to_string(), "3600 S");
        assert_eq!(duration_back(86_400, 60).to_string(), "86400 S");
        // Past that IB refuses the unit itself (error 321), so the same span is days.
        assert_eq!(duration_back(86_401, 60).to_string(), "2 D");
        assert_eq!(duration_back(30 * 86_400, 60).to_string(), "30 D");
        assert_eq!(duration_back(365 * 86_400, 60).to_string(), "365 D");
        // And past a year, years: the widest window this connector ever asks for.
        assert_eq!(duration_back(730 * 86_400, 60).to_string(), "2 Y");
        // A bar coarser than half an hour is never asked for in seconds.
        assert_eq!(duration_back(2 * 3_600, 3_600).to_string(), "2 D");
        assert_eq!(duration_back(86_400, 86_400).to_string(), "30 D");
    }

    #[test]
    fn a_top_up_asks_for_a_window_ib_will_serve() {
        // The gap after the last stored bar is a candle or two; IB refuses a window that
        // short for the bar size, so it is widened backwards instead of being sent as is.
        assert_eq!(duration_back(300, 300).to_string(), "9000 S");
        assert_eq!(duration_back(60, 60).to_string(), "1800 S");
        assert_eq!(duration_back(0, 300).to_string(), "9000 S");
        // Widening never drops below a minute, whatever the arithmetic says.
        assert_eq!(duration_back(1, 1).to_string(), "60 S");
    }

    #[test]
    fn a_bar_keeps_its_stamp_and_never_a_negative_volume() {
        let bar = ibapi::market_data::historical::Bar {
            date: time::macros::datetime!(2024-03-15 0:00 UTC),
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            // IB sends -1 where a series has no volume (midpoint bars, cash forex). That
            // sentinel must not travel into the store as a negative quantity.
            volume: -1.0,
            wap: 0.0,
            count: 0,
        };
        let out = to_bar(&bar);
        assert_eq!(out.ts, time::macros::datetime!(2024-03-15 0:00 UTC));
        assert_eq!(out.volume, 0.0);
    }
}
