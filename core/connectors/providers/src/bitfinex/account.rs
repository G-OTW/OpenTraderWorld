//! The Bitfinex account, read only, both books at once: one key sees the exchange, margin
//! and funding wallets, the margin and derivatives positions, every working order and every
//! fill, so the spot adapter reads it all and the derivatives one declares no account.
//!
//! Every call is a signed POST with a JSON body (`{}` when it takes no parameter):
//!   - Summary: the key's permissions, then the wallets when the key may read them.
//!   - Balances: the wallets, summed per currency across the three wallets; a derivatives
//!     collateral (`USTF0`) counts as its currency (`UST`).
//!   - Fills: `trades/hist` answers every instrument in one list (no symbol to name), the
//!     newest first, 1000 rows a call. A full page asks again up to its oldest millisecond,
//!     leaving out the rows of that millisecond already read.
//!
//! Rows name instruments the way the market data does (`tBTCUSD`, `tBTCF0:USTF0`); each
//! carries the venue of the book it belongs to. Amounts count the base currency on both
//! books, so no contract size is needed.

use std::collections::{BTreeMap, HashMap};

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, AssetClass, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill,
    InstrumentId, Mapped, Money, Order, Position, Price, Qty, Side, TimeRange,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int};
use otw_connector_sdk::{HttpRequest, MapCtx, Method, NativeRequest};
use serde_json::value::RawValue;

use super::{currency, split_pair, Book, Session, AUTH};

/// Rows per `trades/hist` call. The docs allow 2500; ccxt writes 1000. The smaller is used,
/// unverified without a key.
const FILL_PAGE: usize = 1000;
const MAX_PAGES: u32 = 2_000;

pub enum Pending {
    Permissions,
    /// The wallets, after the permissions a summary reports.
    SummaryWallets { permissions: Vec<String>, can_trade: Option<bool> },
    Balances,
    Positions,
    Orders,
    Fills { cur: Cursor, range: TimeRange },
}

/// Where a fill walk stands: the window's end (inclusive, ms), the ids already read at that
/// very millisecond, pages used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub end: i64,
    pub seen: Vec<i64>,
    pub pages: u32,
}

fn bad(c: &str) -> ConnectorError {
    ConnectorError::invalid(format!("bad cursor `{c}`"))
}

impl Cursor {
    fn encode(&self) -> String {
        let ids: Vec<String> = self.seen.iter().map(i64::to_string).collect();
        format!("{}:{}:{}", self.end, ids.join(","), self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = c.split(':').collect();
        let [end, ids, pages] = parts.as_slice() else { return Err(bad(c)) };
        let seen = if ids.is_empty() { Vec::new() } else { ids.split(',').map(|i| i.parse::<i64>().map_err(|_| bad(c))).collect::<Result<_, _>>()? };
        Ok(Cursor { end: end.parse().map_err(|_| bad(c))?, seen, pages: pages.parse().map_err(|_| bad(c))? })
    }
}

fn private(endpoint: &'static str, path: &str, req_id: CorrId, body: String) -> HttpRequest {
    let mut h = HttpRequest::get(endpoint, AUTH, path, req_id).signed();
    h.method = Method::Post;
    h.body = Some(body.into());
    h
}

fn fills_request(req_id: CorrId, cur: &Cursor, range: &TimeRange) -> Result<HttpRequest, ConnectorError> {
    if cur.pages >= MAX_PAGES {
        return Err(ConnectorError::invalid("this period holds more Bitfinex fills than one pull reads. Ask for a shorter period."));
    }
    // Written by hand so the body signed is the body sent, in a fixed order.
    let body = format!(r#"{{"start":{},"end":{},"limit":{FILL_PAGE},"sort":-1}}"#, range.from.millis(), cur.end);
    Ok(private("tradesHist", "/v2/auth/r/trades/hist", req_id, body))
}

pub fn encode(cmd: &Command, pending: &mut HashMap<CorrId, super::Pending>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    let (req, p) = match kind {
        AccountKind::Summary => (private("permissions", "/v2/auth/r/permissions", req_id, "{}".into()), Pending::Permissions),
        AccountKind::Balances => (private("wallets", "/v2/auth/r/wallets", req_id, "{}".into()), Pending::Balances),
        AccountKind::Positions => (private("positions", "/v2/auth/r/positions", req_id, "{}".into()), Pending::Positions),
        AccountKind::OpenOrders => (private("orders", "/v2/auth/r/orders", req_id, "{}".into()), Pending::Orders),
        // Every instrument in one list: the names a caller passes are not needed.
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to.millis() <= range.from.millis() {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let cur = match cursor {
                Some(c) => Cursor::parse(c)?,
                None => Cursor { end: range.to.millis() - 1, seen: Vec::new(), pages: 0 },
            };
            (fills_request(req_id, &cur, &range)?, Pending::Fills { cur, range })
        }
    };
    pending.insert(req_id, super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

/// What to check when Bitfinex refuses the key.
pub fn hint(p: &Pending, e: ConnectorError) -> ConnectorError {
    let code = e.provider_code.as_deref().unwrap_or_default();
    let lower = e.message.to_ascii_lowercase();
    let fix = match (e.kind, code) {
        (ErrorKind::Auth, "10114") => {
            "Bitfinex refused the nonce: another program signed with this key using larger nonces. Give OpenTraderWorld a key of its own."
        }
        (ErrorKind::Auth, _) if lower.contains("nonce") => {
            "Bitfinex refused the nonce: another program signed with this key using larger nonces. Give OpenTraderWorld a key of its own."
        }
        (ErrorKind::Auth, _) => "Check the key and its secret, and that the key was confirmed from Bitfinex's email.",
        (ErrorKind::Permission, _) => match p {
            Pending::Fills { .. } => "The key needs read access to the history scope.",
            Pending::Positions => "The key needs read access to the positions scope.",
            Pending::Orders => "The key needs read access to the orders scope.",
            _ => "The key needs read access to the account and wallets scopes.",
        },
        _ => return e,
    };
    ConnectorError { message: format!("{}. {fix}", e.message), ..e }
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("bitfinex {what}: {e}"))
}

/// A row of positional fields; new fields may be appended at any time (docs).
struct Row(Vec<Box<RawValue>>);

impl Row {
    fn rows(body: &[u8], what: &str) -> Result<Vec<Row>, ConnectorError> {
        let raw: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err(what, e))?;
        Ok(raw.into_iter().map(Row).collect())
    }

    fn get(&self, i: usize) -> Option<&str> {
        self.0.get(i).map(|v| v.get()).filter(|t| *t != "null")
    }

    fn text(&self, i: usize) -> Option<String> {
        serde_json::from_str::<String>(self.get(i)?).ok()
    }

    fn dec(&self, i: usize) -> Option<Decimal> {
        serde_json::from_str::<Dec>(self.get(i)?).ok().map(|d| d.0)
    }

    fn int(&self, i: usize) -> Option<i64> {
        serde_json::from_str::<Int>(self.get(i)?).ok().map(|d| d.0)
    }
}

fn instrument(native: &str) -> InstrumentId {
    InstrumentId::new(Book::of(native).venue(), native)
}

/// What the row's name says it is: only a perpetual is not plainly a pair.
fn class_of(native: &str) -> Option<AssetClass> {
    (Book::of(native) == Book::Derivatives).then_some(AssetClass::Perp)
}

fn quote_of(native: &str) -> Option<String> {
    split_pair(native).map(|(_, q)| q)
}

fn side_of(amount: Decimal) -> Mapped<Side> {
    if amount.is_sign_positive() && !amount.is_zero() {
        Mapped::Known(Side::Buy)
    } else if amount.is_sign_negative() && !amount.is_zero() {
        Mapped::Known(Side::Sell)
    } else {
        otw_connector_sdk::enums::unknown("bitfinex", "side", "0")
    }
}

/// Which price of an order row is a limit and which a trigger: `PRICE` (16) is the limit of
/// a limit order and the trigger of a stop; a stop limit's limit is `PRICE_AUX_LIMIT` (19).
/// The `EXCHANGE ` prefix (an exchange-wallet order) does not change the roles.
fn price_roles(kind: &str, price: Option<Price>, aux: Option<Price>) -> (Option<Price>, Option<Price>) {
    match kind.trim().strip_prefix("EXCHANGE ").unwrap_or(kind.trim()) {
        "LIMIT" | "FOK" | "IOC" => (price, None),
        "MARKET" => (None, None),
        "STOP" | "TRAILING STOP" => (None, price),
        "STOP LIMIT" => (aux, price),
        other => {
            tracing::warn!(provider = "bitfinex", what = "order type", raw = other, "unknown native enum value");
            (price, None)
        }
    }
}

fn positive_price(d: Option<Decimal>) -> Option<Price> {
    d.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

const FIAT: &[&str] = &["USD", "EUR", "GBP", "JPY"];

/// `[TYPE, CURRENCY, BALANCE, UNSETTLED_INTEREST, AVAILABLE_BALANCE, ...]`, summed per
/// currency; an unknown available balance (null) counts as all of it.
fn balances(body: &[u8]) -> Result<(Vec<Balance>, Vec<String>), ConnectorError> {
    let mut by: BTreeMap<String, (Decimal, Decimal)> = BTreeMap::new();
    let mut wallets: Vec<String> = Vec::new();
    for r in Row::rows(body, "wallets")? {
        let (Some(wallet), Some(ccy), Some(total)) = (r.text(0), r.text(1), r.dec(2)) else { continue };
        if !total.is_sign_positive() || total.is_zero() {
            continue;
        }
        let available = r.dec(4).unwrap_or(total).min(total).max(Decimal::ZERO);
        let e = by.entry(currency(&ccy)).or_insert((Decimal::ZERO, Decimal::ZERO));
        e.0 += total;
        e.1 += available;
        if !wallets.contains(&wallet) {
            wallets.push(wallet);
        }
    }
    let out = by
        .into_iter()
        .map(|(asset, (total, available))| Balance { fiat: FIAT.contains(&asset.as_str()), asset, total: Money(total), available: Money(available), name: None, account: None })
        .collect();
    Ok((out, wallets))
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let evs = match p {
        Pending::Permissions => {
            // `[[SCOPE, READ, WRITE], ...]`.
            let mut permissions = Vec::new();
            let mut can_trade = None;
            let mut wallets = false;
            for r in Row::rows(body, "permissions")? {
                let Some(scope) = r.text(0) else { continue };
                let (read, write) = (r.int(1) == Some(1), r.int(2) == Some(1));
                if scope == "orders" {
                    can_trade = Some(write);
                }
                if scope == "wallets" {
                    wallets = read;
                }
                match (read, write) {
                    (true, true) => permissions.push(format!("{scope}: read, write")),
                    (true, false) => permissions.push(format!("{scope}: read")),
                    (false, true) => permissions.push(format!("{scope}: write")),
                    (false, false) => {}
                }
            }
            if !wallets {
                let detail = "Bitfinex: the key may not read the wallets, so balances stay empty".to_string();
                let summary = AccountSummary { account_type: None, permissions, assets_held: 0, can_trade, open_positions: None, wallet_balance: None, detail: Some(detail) };
                return Ok(vec![env(ctx, corr, Event::AccountSummary(summary))]);
            }
            s.pending.insert(corr, super::Pending::Account(Pending::SummaryWallets { permissions, can_trade }));
            s.next = Some((corr, NativeRequest::Http(private("wallets", "/v2/auth/r/wallets", corr, "{}".into()))));
            vec![]
        }
        Pending::SummaryWallets { permissions, can_trade } => {
            let (held, wallets) = balances(body)?;
            let detail = if wallets.is_empty() { "Bitfinex: no wallet holds anything".to_string() } else { format!("Bitfinex: {} currencies held in the {} wallet(s)", held.len(), wallets.join(", ")) };
            let summary = AccountSummary {
                account_type: None,
                permissions,
                assets_held: u32::try_from(held.len()).unwrap_or(u32::MAX),
                can_trade,
                open_positions: None,
                wallet_balance: None,
                detail: Some(detail),
            };
            vec![env(ctx, corr, Event::AccountSummary(summary))]
        }
        Pending::Balances => vec![chunk(ctx, corr, AccountItems::Balances(balances(body)?.0), None)],
        Pending::Positions => {
            // `[SYMBOL, STATUS, AMOUNT, BASE_PRICE, FUNDING, FUNDING_TYPE, PL, PL_PERC,
            // PRICE_LIQ, LEVERAGE, _, POSITION_ID, MTS_CREATE, MTS_UPDATE, _, TYPE, ...]`.
            let mut out = Vec::new();
            for r in Row::rows(body, "positions")? {
                let (Some(symbol), Some(qty)) = (r.text(0), r.dec(2)) else { continue };
                if qty.is_zero() || r.text(1).is_some_and(|st| !st.eq_ignore_ascii_case("ACTIVE")) {
                    continue;
                }
                out.push(Position {
                    instrument: instrument(&symbol),
                    qty: Qty(qty),
                    avg_price: positive_price(r.dec(3)),
                    unrealized_pnl: r.dec(6).map(Money),
                    currency: quote_of(&symbol),
                    mark_price: None,
                    venue: None,
                    account: None,
                    class: class_of(&symbol),
                    multiplier: None,
                    name: None,
                });
            }
            vec![chunk(ctx, corr, AccountItems::Positions(out), None)]
        }
        Pending::Orders => {
            // `[ID, GID, CID, SYMBOL, MTS_CREATE, MTS_UPDATE, AMOUNT, AMOUNT_ORIG, ORDER_TYPE,
            // TYPE_PREV, MTS_TIF, _, FLAGS, STATUS, _, _, PRICE, PRICE_AVG, PRICE_TRAILING,
            // PRICE_AUX_LIMIT, ...]`. `AMOUNT` is what is left, signed like the order.
            let mut orders = Vec::new();
            for r in Row::rows(body, "orders")? {
                let (Some(id), Some(symbol)) = (r.int(0), r.text(3)) else { continue };
                let left = r.dec(6).unwrap_or_default();
                let orig = r.dec(7).unwrap_or(left);
                let kind = r.text(8).unwrap_or_default();
                let (limit_price, stop_price) = price_roles(&kind, positive_price(r.dec(16)), positive_price(r.dec(19)));
                orders.push(Order {
                    id: id.to_string(),
                    instrument: instrument(&symbol),
                    side: side_of(if orig.is_zero() { left } else { orig }),
                    order_type: kind.to_ascii_lowercase(),
                    qty: Qty(orig.abs()),
                    filled_qty: Qty((orig.abs() - left.abs()).max(Decimal::ZERO)),
                    limit_price,
                    stop_price,
                    placed_at: r.int(4).map(ts_millis).transpose()?,
                    quote_ccy: quote_of(&symbol),
                    account: None,
                    venue: None,
                    class: class_of(&symbol),
                });
            }
            vec![chunk(ctx, corr, AccountItems::Orders(orders), None)]
        }
        Pending::Fills { cur, range } => {
            // `[ID, SYMBOL, MTS, ORDER_ID, EXEC_AMOUNT, EXEC_PRICE, ORDER_TYPE, ORDER_PRICE,
            // MAKER, FEE, FEE_CURRENCY, CID]`, the newest first. The fee is negative when paid.
            let rows = Row::rows(body, "trades")?;
            let n = rows.len();
            let mut fills = Vec::with_capacity(n);
            let mut stamps = Vec::with_capacity(n);
            for r in &rows {
                let (Some(id), Some(symbol), Some(mts), Some(amount)) = (r.int(0), r.text(1), r.int(2), r.dec(4)) else {
                    return Err(ConnectorError::decode("bitfinex trades: row shape"));
                };
                stamps.push((mts, id));
                if mts == cur.end && cur.seen.contains(&id) {
                    continue;
                }
                if mts < range.from.millis() || mts > cur.end {
                    continue;
                }
                fills.push(Fill {
                    id: id.to_string(),
                    order_id: r.int(3).map(|o| o.to_string()),
                    instrument: instrument(&symbol),
                    ts: ts_millis(mts)?,
                    side: side_of(amount),
                    qty: Qty(amount.abs()),
                    price: Price(r.dec(5).unwrap_or_default()),
                    fee: Money(-r.dec(9).unwrap_or_default()),
                    fee_asset: r.text(10).map(|c| currency(&c)).filter(|c| !c.is_empty()),
                    quote_ccy: quote_of(&symbol),
                    account: None,
                    venue: None,
                    class: class_of(&symbol),
                    multiplier: None,
                });
            }
            let oldest = stamps.iter().map(|(t, _)| *t).min();
            let at_oldest: Vec<i64> = stamps.iter().filter(|(t, _)| Some(*t) == oldest).map(|(_, id)| *id).collect();
            let next = match oldest {
                Some(o) if n >= FILL_PAGE => {
                    if o == cur.end && at_oldest.iter().all(|id| cur.seen.contains(id)) {
                        return Err(ConnectorError::invalid(format!(
                            "Bitfinex answers {FILL_PAGE} fills per call and had more than that in one millisecond; this period cannot be read over the API."
                        )));
                    }
                    let mut seen = if o == cur.end { cur.seen.clone() } else { Vec::new() };
                    seen.extend(at_oldest.iter().filter(|id| !seen.contains(id)).copied().collect::<Vec<_>>());
                    Some(Cursor { end: o, seen, pages: cur.pages + 1 })
                }
                _ => None,
            };
            vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))]
        }
    };
    Ok(evs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_round_trips() {
        let c = Cursor { end: 1_792_390_000_123, seen: vec![402088407, 402088408], pages: 3 };
        assert_eq!(Cursor::parse(&c.encode()).unwrap(), c);
        let c = Cursor { end: 5, seen: vec![], pages: 0 };
        assert_eq!(Cursor::parse(&c.encode()).unwrap(), c);
        assert!(Cursor::parse("1:x:2").is_err());
    }

    #[test]
    fn order_prices_follow_the_type() {
        let (p, a) = (Some(Price(Decimal::new(100, 0))), Some(Price(Decimal::new(99, 0))));
        assert_eq!(price_roles("EXCHANGE LIMIT", p, a), (p, None));
        assert_eq!(price_roles("STOP", p, a), (None, p));
        assert_eq!(price_roles("EXCHANGE STOP LIMIT", p, a), (a, p));
        assert_eq!(price_roles("MARKET", p, a), (None, None));
    }
}
