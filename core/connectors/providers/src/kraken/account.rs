//! Kraken spot account, read only: fills over a window, balances, open orders and the key's
//! own summary. Every private call is a signed POST with its parameters in the body.
//!
//! What the API's shape forces:
//!   - `TradesHistory` takes a window and covers the whole account, paged by `ofs` 50 rows
//!     at a time; 400 pages at most for one pull.
//!   - Rows name the pair by its key (`XXBTZUSD`) and orders by its altname (`XBTUSD`);
//!     both are written as the wsname (`XBT/USD`), what OTW stores for this account.
//!   - A balance names its asset by Kraken's code (`XXBT`); the name it trades under comes
//!     from the asset list, never from cutting a prefix off (`XXLM` is XLM, not LM).

use std::collections::BTreeMap;
use std::sync::Arc;

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Envelope, Event, Fill, InstrumentId,
    Mapped, Money, Order, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_secs_frac, Dec, OptDec};
use otw_connector_sdk::signer::encode_query;
use otw_connector_sdk::{HttpRequest, MapCtx, Method, NativeRequest};
use serde::Deserialize;

use super::{Answer, PairName, Session, REST, VENUE};

/// Rows one `TradesHistory` page returns.
const PAGE: u64 = 50;
/// Hard stop on the whole pull.
const MAX_PAGES: u64 = 400;

pub enum Pending {
    Summary,
    Balances,
    AssetNames,
    Orders,
    Fills { ofs: u64 },
}

fn private(endpoint: &'static str, method: &str, req_id: CorrId, params: &[(&str, String)]) -> HttpRequest {
    let mut h = HttpRequest::get(endpoint, REST, format!("/0/private/{method}"), req_id).signed();
    h.method = Method::Post;
    let pairs: Vec<(String, String)> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    // The signer puts the nonce in front.
    h.body = Some(encode_query(&pairs).into());
    h
}

pub fn encode(cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let mut out = Vec::new();
    let (req, p) = match kind {
        AccountKind::Summary => (private("balance", "Balance", *req_id, &[]), Pending::Summary),
        AccountKind::Balances => {
            if !Session::fresh(&s.assets, ctx.received_ts) {
                s.pending.insert((*req_id, "assets"), super::Pending::Account(Pending::AssetNames));
                out.push(NativeRequest::Http(HttpRequest::get("assets", REST, "/0/public/Assets", *req_id)));
            }
            (private("balance", "Balance", *req_id, &[]), Pending::Balances)
        }
        AccountKind::OpenOrders => {
            out.extend(s.need_pairs(*req_id, ctx.received_ts));
            (private("openOrders", "OpenOrders", *req_id, &[]), Pending::Orders)
        }
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let ofs = match cursor {
                Some(c) => c.parse::<u64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
                None => 0,
            };
            if ofs / PAGE >= MAX_PAGES {
                return Err(ConnectorError::invalid("this period holds more Kraken fills than one pull reads. Ask for a shorter period."));
            }
            out.extend(s.need_pairs(*req_id, ctx.received_ts));
            let params = [
                ("type", "all".to_string()),
                ("trades", "false".to_string()),
                ("start", range.from.secs().to_string()),
                ("end", range.to.secs().to_string()),
                ("ofs", ofs.to_string()),
            ];
            // The private-call counter decays slowly: the limiter paces the walk one page per
            // 1.2 s (capabilities.toml), as before the port.
            (private("tradesHistory", "TradesHistory", *req_id, &params), Pending::Fills { ofs })
        }
        AccountKind::Positions => return Err(ConnectorError::unsupported("Kraken spot holds no positions")),
    };
    s.pending.insert((*req_id, req.endpoint_id), super::Pending::Account(p));
    out.push(NativeRequest::Http(req));
    Ok(out)
}

#[derive(Deserialize)]
struct AssetRow {
    altname: String,
}

#[derive(Deserialize)]
struct TradesPage {
    #[serde(default)]
    trades: BTreeMap<String, TradeRow>,
}

#[derive(Deserialize)]
struct TradeRow {
    #[serde(default)]
    ordertxid: String,
    #[serde(default)]
    pair: String,
    time: Dec,
    #[serde(rename = "type", default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    vol: OptDec,
}

#[derive(Deserialize)]
struct OpenPage {
    #[serde(default)]
    open: BTreeMap<String, OrderRow>,
}

#[derive(Deserialize)]
struct OrderRow {
    #[serde(default)]
    opentm: Option<Dec>,
    #[serde(default)]
    vol: OptDec,
    #[serde(default)]
    vol_exec: OptDec,
    #[serde(default)]
    descr: Descr,
}

#[derive(Deserialize, Default)]
struct Descr {
    #[serde(default)]
    pair: String,
    #[serde(rename = "type", default)]
    side: String,
    #[serde(default)]
    ordertype: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    price2: OptDec,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kraken {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("kraken", "side", raw),
    }
}

/// The pair as the account side names it, with its quote; an unknown one keeps its own
/// spelling and no currency.
fn named(names: &std::collections::HashMap<String, PairName>, raw: &str) -> (String, Option<String>) {
    match names.get(raw) {
        Some(n) => (n.wsname.clone(), Some(n.quote.clone()).filter(|q| !q.is_empty())),
        None => (raw.to_string(), None),
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::AssetNames => {
            let page: Answer<BTreeMap<String, AssetRow>> = serde_json::from_slice(body).map_err(|e| decode_err("Assets", e))?;
            let names = page.result.into_iter().map(|(code, a)| (code, a.altname)).collect();
            s.assets = Some((ctx.received_ts, Arc::new(names)));
            Ok(vec![])
        }
        Pending::Summary | Pending::Balances => {
            let page: Answer<BTreeMap<String, OptDec>> = serde_json::from_slice(body).map_err(|e| decode_err("Balance", e))?;
            let names = s.assets();
            let held: Vec<Balance> = page
                .result
                .iter()
                .filter_map(|(code, q)| {
                    let qty = q.0.filter(|v| v.is_sign_positive() && !v.is_zero())?;
                    let asset = names.get(code).cloned().unwrap_or_else(|| code.clone());
                    Some(Balance { asset, total: Money(qty), available: Money(qty), name: None, fiat: false, account: None })
                })
                .collect();
            if matches!(p, Pending::Summary) {
                return Ok(vec![env(
                    ctx,
                    corr,
                    Event::AccountSummary(AccountSummary {
                        account_type: None,
                        permissions: vec![],
                        assets_held: u32::try_from(held.len()).unwrap_or(u32::MAX),
                        can_trade: None,
                        open_positions: None,
                        wallet_balance: None,
                        detail: Some(format!("key accepted, {} asset(s) with a balance", held.len())),
                    }),
                )]);
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)])
        }
        Pending::Orders => {
            let page: Answer<OpenPage> = serde_json::from_slice(body).map_err(|e| decode_err("OpenOrders", e))?;
            let names = s.pairs();
            let mut out = Vec::with_capacity(page.result.open.len());
            for (id, o) in page.result.open {
                let (symbol, quote) = named(&names, &o.descr.pair);
                out.push(Order {
                    id,
                    instrument: InstrumentId::new(VENUE, &symbol),
                    side: side(&o.descr.side),
                    order_type: o.descr.ordertype,
                    qty: Qty(o.vol.0.unwrap_or_default()),
                    filled_qty: Qty(o.vol_exec.0.unwrap_or_default()),
                    limit_price: positive(o.descr.price),
                    stop_price: positive(o.descr.price2),
                    placed_at: o.opentm.map(|t| ts_secs_frac(&t.0.to_string())).transpose()?,
                    quote_ccy: quote,
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Fills { ofs } => {
            let page: Answer<TradesPage> = serde_json::from_slice(body).map_err(|e| decode_err("TradesHistory", e))?;
            let names = s.pairs();
            let rows = page.result.trades;
            let n = rows.len() as u64;
            let mut fills = Vec::with_capacity(rows.len());
            for (txid, r) in rows {
                let (symbol, quote) = named(&names, &r.pair);
                fills.push(Fill {
                    id: txid,
                    order_id: Some(r.ordertxid),
                    instrument: InstrumentId::new(VENUE, &symbol),
                    ts: ts_secs_frac(&r.time.0.to_string())?,
                    side: side(&r.side),
                    qty: Qty(r.vol.0.unwrap_or_default()),
                    price: Price(r.price.0.unwrap_or_default()),
                    // Kraken bills a positive fee in the pair's quote currency.
                    fee: Money(r.fee.0.unwrap_or_default()),
                    fee_asset: quote.clone(),
                    quote_ccy: quote,
                    account: None,
                    venue: None,
                    class: None,
                    multiplier: None,
                });
            }
            let next = (n >= PAGE).then(|| (ofs + n).to_string());
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next)])
        }
    }
}
