//! Alpaca trading account, read only: fills, positions (with their cost basis and Alpaca's
//! own mark), working orders, and the account summary. Paper and live are different hosts
//! with different keys (the account's `env`).
//!
//! An instrument is asked for, never inferred: `/v2/assets/{symbol}` says what it is, and an
//! option's multiplier comes from its contract (`/v2/options/contracts/{symbol}`), because a
//! hundredfold error in the point value is a hundredfold error in the PnL. A stock or an
//! option settles in the account's currency; a crypto pair states its quote after the slash.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, AssetClass, CanonicalSymbol, Command, ConnectorError, CorrId, Decimal, Envelope,
    Event, Fill, InstrumentId, InstrumentSpec, Mapped, Money, Order, Position, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest, RawFrame};
use serde::Deserialize;

use super::{class_market, decode_err, is_occ, segment, status_error, Market, Session, TRADING};

const PAGE: usize = 100;
const MAX_PAGES: u32 = 500;

pub enum Pending {
    Summary,
    Currency,
    Positions,
    Orders,
    Fills { pages: u32 },
    Asset { id: InstrumentId },
    Contract { id: InstrumentId },
}

fn get(endpoint: &'static str, path: impl Into<String>, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, TRADING, path, req_id).signed()
}

fn put(s: &mut Session, req_id: CorrId, h: HttpRequest, p: Pending, out: &mut Vec<NativeRequest>) {
    s.pending.insert((req_id, h.endpoint_id), super::Pending::Account(p));
    out.push(NativeRequest::Http(h));
}

pub fn encode(cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let mut out = Vec::new();
    // A stock or an option settles in the account's currency: read once per session.
    if matches!(kind, AccountKind::Fills | AccountKind::Positions | AccountKind::OpenOrders) && s.currency.is_none() {
        put(s, *req_id, get("currency", "/v2/account", *req_id), Pending::Currency, &mut out);
    }
    match kind {
        AccountKind::Summary => put(s, *req_id, get("account", "/v2/account", *req_id), Pending::Summary, &mut out),
        AccountKind::Positions => put(s, *req_id, get("positions", "/v2/positions", *req_id), Pending::Positions, &mut out),
        AccountKind::OpenOrders => put(
            s,
            *req_id,
            get("orders", "/v2/orders", *req_id).query("status", "open").query("limit", 500).query("direction", "desc"),
            Pending::Orders,
            &mut out,
        ),
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            // Cursor `<pages>:<last row id>`.
            let (pages, token) = match cursor {
                Some(c) => {
                    let (p, t) = c.split_once(':').ok_or_else(|| ConnectorError::invalid(format!("bad cursor `{c}`")))?;
                    (p.parse::<u32>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?, Some(t.to_string()))
                }
                None => (0, None),
            };
            if pages >= MAX_PAGES {
                return Err(ConnectorError::invalid("this period holds more Alpaca fills than one pull reads. Ask for a shorter period."));
            }
            let rfc = |t: otw_connector_model::Timestamp| {
                t.to_offset_datetime()
                    .and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok())
                    .ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))
            };
            let mut h = get("fills", "/v2/account/activities/FILL", *req_id)
                .query("after", rfc(range.from)?)
                .query("until", rfc(range.to)?)
                .query("direction", "asc")
                .query("page_size", PAGE);
            if let Some(t) = token {
                h = h.query("page_token", t);
            }
            put(s, *req_id, h, Pending::Fills { pages }, &mut out);
        }
        AccountKind::Balances => return Err(ConnectorError::unsupported("Alpaca reports positions, not balances")),
    }
    Ok(out)
}

/// What these instruments are: the account's currency once, the asset for each, and the
/// contract for an option.
pub fn resolve(cmd: &Command, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::ResolveInstruments { req_id, ids } = cmd else {
        return Err(ConnectorError::invalid("not a lookup"));
    };
    let mut out = Vec::new();
    if s.currency.is_none() {
        put(s, *req_id, get("currency", "/v2/account", *req_id), Pending::Currency, &mut out);
    }
    // One endpoint id per symbol: several answers of one lookup are told apart by it.
    if ids.len() > ENDPOINTS.len() {
        return Err(ConnectorError::invalid(format!("Alpaca resolves {} instruments per lookup", ENDPOINTS.len())));
    }
    for (id, (asset_ep, contract_ep)) in ids.iter().zip(ENDPOINTS) {
        put(s, *req_id, get(asset_ep, format!("/v2/assets/{}", segment(&id.native)), *req_id), Pending::Asset { id: id.clone() }, &mut out);
        if is_occ(&id.native) {
            put(s, *req_id, get(contract_ep, format!("/v2/options/contracts/{}", segment(&id.native)), *req_id), Pending::Contract { id: id.clone() }, &mut out);
        }
    }
    Ok(out)
}

/// The most instruments one lookup asks for.
pub const RESOLVE_BATCH: usize = ENDPOINTS.len();

/// Endpoint ids for the lookups of one batch (the pending map is keyed by request and
/// endpoint, so each symbol of a batch needs its own).
const ENDPOINTS: &[(&str, &str)] = &[
    ("asset0", "contract0"), ("asset1", "contract1"), ("asset2", "contract2"), ("asset3", "contract3"),
    ("asset4", "contract4"), ("asset5", "contract5"), ("asset6", "contract6"), ("asset7", "contract7"),
    ("asset8", "contract8"), ("asset9", "contract9"), ("asset10", "contract10"), ("asset11", "contract11"),
    ("asset12", "contract12"), ("asset13", "contract13"), ("asset14", "contract14"), ("asset15", "contract15"),
    ("asset16", "contract16"), ("asset17", "contract17"), ("asset18", "contract18"), ("asset19", "contract19"),
    ("asset20", "contract20"), ("asset21", "contract21"), ("asset22", "contract22"), ("asset23", "contract23"),
    ("asset24", "contract24"), ("asset25", "contract25"), ("asset26", "contract26"), ("asset27", "contract27"),
    ("asset28", "contract28"), ("asset29", "contract29"), ("asset30", "contract30"), ("asset31", "contract31"),
];

#[derive(Deserialize)]
struct AccountRow {
    #[serde(default)]
    account_number: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    currency: String,
}

#[derive(Deserialize)]
struct PositionRow {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    qty: OptDec,
    #[serde(default)]
    side: String,
    #[serde(default)]
    avg_entry_price: OptDec,
    #[serde(default)]
    current_price: OptDec,
    #[serde(default)]
    unrealized_pl: OptDec,
    #[serde(default)]
    exchange: String,
    #[serde(default)]
    asset_class: String,
}

#[derive(Deserialize)]
struct OrderRow {
    #[serde(default)]
    id: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    order_type: Option<String>,
    #[serde(default)]
    qty: OptDec,
    #[serde(default)]
    filled_qty: OptDec,
    #[serde(default)]
    limit_price: OptDec,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    submitted_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    asset_class: String,
}

#[derive(Deserialize)]
struct FillRow {
    #[serde(default)]
    id: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    qty: OptDec,
    #[serde(default)]
    price: OptDec,
    transaction_time: String,
}

#[derive(Deserialize)]
struct AssetRow {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    exchange: String,
    #[serde(default)]
    class: String,
}

#[derive(Deserialize)]
struct ContractRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    multiplier: OptDec,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

/// A short sale is a sell: the direction is the trade's, whether it opens or closes is the
/// journal's business.
fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" | "sell_short" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("alpaca", "side", raw),
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

/// The id a row's symbol has, by the class the row states (or Alpaca's notation when it
/// states none).
fn id_of(symbol: &str, class: &str) -> Result<InstrumentId, ConnectorError> {
    let venue = match class_market(class) {
        Some(AssetClass::Spot) => super::VENUE_CRYPTO,
        Some(AssetClass::Option) => super::VENUE_OPTIONS,
        Some(_) => super::VENUE,
        None if symbol.contains('/') => super::VENUE_CRYPTO,
        None if is_occ(symbol) => super::VENUE_OPTIONS,
        None => super::VENUE,
    };
    Ok(InstrumentId::new(venue, &symbol.to_ascii_uppercase()))
}

/// A crypto pair states its quote after the slash, Alpaca's own notation.
fn pair_quote(id: &InstrumentId) -> Option<String> {
    (super::market_of(id) == Market::Crypto).then(|| id.native.split_once('/').map(|(_, q)| q.to_string())).flatten()
}

/// What a row settles in: the pair's quote, else the account's currency.
fn settles_in(id: &InstrumentId, s: &Session) -> Option<String> {
    pair_quote(id).or_else(|| s.currency.clone())
}

pub fn decode(p: Pending, corr: CorrId, frame: &RawFrame, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    // A lookup that finds nothing is no instrument, not a failed call.
    if matches!(p, Pending::Asset { .. } | Pending::Contract { .. } | Pending::Currency) {
        if status_error(frame).is_some() {
            if matches!(p, Pending::Currency) {
                s.currency = Some("USD".into());
            }
            return Ok(vec![]);
        }
    } else if let Some(e) = status_error(frame) {
        return Err(e);
    }
    match p {
        Pending::Summary => {
            let a: AccountRow = serde_json::from_slice(body).map_err(|e| decode_err("account", e))?;
            Ok(vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: None,
                    permissions: vec![],
                    assets_held: 0,
                    can_trade: None,
                    open_positions: None,
                    wallet_balance: None,
                    detail: Some(format!("account {} is {}, in {}", a.account_number, a.status, a.currency)),
                }),
            )])
        }
        Pending::Currency => {
            let a: AccountRow = serde_json::from_slice(body).map_err(|e| decode_err("account", e))?;
            s.currency = Some(if a.currency.is_empty() { "USD".into() } else { a.currency });
            Ok(vec![])
        }
        Pending::Asset { id } => {
            let a: AssetRow = serde_json::from_slice(body).map_err(|e| decode_err("asset", e))?;
            let Some(class) = class_market(&a.class) else { return Ok(vec![]) };
            let currency = pair_quote(&id).or_else(|| s.currency.clone()).unwrap_or_else(|| "USD".into());
            let mut spec = InstrumentSpec::spot(id, &a.symbol, &currency);
            spec.symbol = CanonicalSymbol(a.name);
            spec.asset_class = class;
            spec.exchange = Some(a.exchange).filter(|e| !e.is_empty());
            // An option's multiplier is its contract's: the contract answer carries it.
            if class == AssetClass::Option {
                return Ok(vec![]);
            }
            Ok(vec![env(ctx, corr, Event::Instrument { spec: Box::new(spec) })])
        }
        Pending::Contract { id } => {
            let c: ContractRow = serde_json::from_slice(body).map_err(|e| decode_err("option contract", e))?;
            let currency = s.currency.clone().unwrap_or_else(|| "USD".into());
            let mut spec = InstrumentSpec::spot(id.clone(), &id.native, &currency);
            spec.symbol = CanonicalSymbol(c.name);
            spec.asset_class = AssetClass::Option;
            spec.contract = otw_connector_model::ContractType::Option;
            spec.multiplier = c.multiplier.0.filter(|m| m.is_sign_positive() && !m.is_zero()).unwrap_or(Decimal::ONE);
            Ok(vec![env(ctx, corr, Event::Instrument { spec: Box::new(spec) })])
        }
        Pending::Positions => {
            let rows: Vec<PositionRow> = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            let mut out = Vec::with_capacity(rows.len());
            for p in rows {
                let qty = p.qty.0.unwrap_or_default().abs();
                let id = id_of(&p.symbol, &p.asset_class)?;
                out.push(Position {
                    currency: settles_in(&id, s),
                    instrument: id,
                    qty: Qty(if p.side == "short" { -qty } else { qty }),
                    avg_price: positive(p.avg_entry_price),
                    unrealized_pnl: p.unrealized_pl.0.map(Money),
                    mark_price: positive(p.current_price),
                    venue: Some(p.exchange).filter(|e| !e.is_empty()),
                    account: None,
                    class: None,
                    multiplier: None,
                    name: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out), None)])
        }
        Pending::Orders => {
            let rows: Vec<OrderRow> = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            let mut out = Vec::with_capacity(rows.len());
            for o in rows {
                let id = id_of(&o.symbol, &o.asset_class)?;
                out.push(Order {
                    quote_ccy: settles_in(&id, s),
                    instrument: id,
                    id: o.id,
                    side: side(&o.side),
                    // `type` is the live field; `order_type` Alpaca's deprecated alias.
                    order_type: o.kind.filter(|k| !k.is_empty()).or(o.order_type).unwrap_or_default(),
                    qty: Qty(o.qty.0.unwrap_or_default()),
                    filled_qty: Qty(o.filled_qty.0.unwrap_or_default()),
                    limit_price: positive(o.limit_price),
                    stop_price: positive(o.stop_price),
                    placed_at: o.submitted_at.or(o.created_at).as_deref().and_then(|t| ts_rfc3339(t).ok()),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Fills { pages } => {
            let rows: Vec<FillRow> = serde_json::from_slice(body).map_err(|e| decode_err("fills", e))?;
            let n = rows.len();
            let last = rows.last().map(|r| r.id.clone()).filter(|i| !i.is_empty());
            let mut out = Vec::with_capacity(n);
            for r in rows {
                if r.symbol.is_empty() {
                    continue;
                }
                let id = id_of(&r.symbol, "")?;
                out.push(Fill {
                    quote_ccy: settles_in(&id, s),
                    instrument: id,
                    id: r.id,
                    order_id: Some(r.order_id),
                    ts: ts_rfc3339(&r.transaction_time)?,
                    side: side(&r.side),
                    qty: Qty(r.qty.0.unwrap_or_default()),
                    price: Price(r.price.0.unwrap_or_default()),
                    // Alpaca bills no commission on a fill (crypto and options fees arrive as
                    // their own activities).
                    fee: Money(Decimal::ZERO),
                    fee_asset: None,
                    account: None,
                    venue: None,
                    class: None,
                    multiplier: None,
                });
            }
            // A short page is the last one; the next page starts after the last row's id.
            let next = (n >= PAGE).then_some(last).flatten().map(|t| format!("{}:{t}", pages + 1));
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(out), next)])
        }
    }
}
