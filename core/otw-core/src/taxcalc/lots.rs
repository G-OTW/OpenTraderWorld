//! Tax lots: which acquisition a disposal is matched against, measured in the tax currency.
//!
//! Pure: the caller resolves every rate first and hands each fill the rate of **its own
//! day**. Two rules decide the number:
//!
//!   - **The matching method is the residence's.** `Average` is the French weighted average
//!     price (PMP), `Fifo` the US and German default, `UkPool` the HMRC sequence (same day,
//!     then the next 30 days, then the section 104 pool).
//!   - **Each leg converts at its own date.** A share bought in USD in March and sold in
//!     October costs its March euros and fetches its October euros; the currency move is part
//!     of the taxable gain. Margin products (futures, CFDs, perpetuals) never exchange the
//!     notional, so their realized PnL converts on the day it is realized instead.
//!
//! Nothing is guessed: a sale with nothing to match is an error naming the fix, and an
//! instrument with a fill that has no rate is left out whole, since one unpriced purchase
//! poisons every later sale's basis.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::Serialize;
use time::{Date, Duration, OffsetDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Average,
    Fifo,
    UkPool,
}

impl Method {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "average" => Some(Self::Average),
            "fifo" => Some(Self::Fifo),
            "uk_pool" => Some(Self::UkPool),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Average => "average",
            Self::Fifo => "fifo",
            Self::UkPool => "uk_pool",
        }
    }
}

/// How a foreign-currency result becomes a tax-currency result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Valuation {
    /// The full price changes hands at each fill (shares, ETFs, option premiums, spot
    /// crypto): cost converts at the purchase date, proceeds at the sale date.
    Spot,
    /// Only the difference settles (futures, CFDs, margin forex, perpetuals): the realized
    /// PnL converts on the day it is realized. Always matched first in, first out.
    Margin,
}

/// One execution, in its own currency, with the rate that turns that currency into the tax
/// currency on the fill's own day.
#[derive(Debug, Clone)]
pub struct Fill {
    pub id: String,
    pub symbol: String,
    pub asset_class: String,
    pub bucket: &'static str,
    pub valuation: Valuation,
    pub at: OffsetDateTime,
    pub buy: bool,
    pub qty: f64,
    /// qty × price × multiplier, in `currency`.
    pub gross: f64,
    /// Commission, already in `currency`.
    pub fee: f64,
    pub currency: String,
    /// 1 unit of `currency` in the tax currency on `at`'s date. None = no rate.
    pub rate: Option<f64>,
}

/// One matched disposal. Amounts in the tax currency unless named `native`.
#[derive(Debug, Clone, Serialize)]
pub struct Disposal {
    pub fill_id: String,
    pub symbol: String,
    pub asset_class: String,
    pub bucket: &'static str,
    /// The acquisition this was matched against, when one lot is identifiable (FIFO).
    #[serde(with = "time::serde::rfc3339::option")]
    pub opened_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub closed_at: OffsetDateTime,
    pub qty: f64,
    /// A short position covered by this fill.
    pub short: bool,
    pub proceeds: f64,
    pub cost: f64,
    pub gain: f64,
    pub native_gain: f64,
    pub currency: String,
    pub holding_days: Option<i64>,
    /// average | fifo | same_day | bed_breakfast | s104.
    pub rule: &'static str,
    /// Loss deferred by the anti-wash rule onto replacement shares (already added back
    /// into `gain`).
    pub disallowed: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenPosition {
    pub symbol: String,
    /// Signed: negative is a short still open.
    pub qty: f64,
    /// What it cost (or, short, what it fetched), in the tax currency.
    pub basis: f64,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub disposals: Vec<Disposal>,
    pub open: Vec<OpenPosition>,
    pub errors: Vec<String>,
}

/// Quantities below this are rounding dust, not a position.
const EPS: f64 = 1e-9;

/// Anti-wash rule: a loss on a sale is deferred onto shares of the same instrument bought
/// inside the window around it (US wash sale, CA superficial loss, ES two months). The
/// deferred amount is added to the replacement shares' cost, so it comes back when they
/// are sold.
#[derive(Debug, Clone, Default)]
pub struct WashRule {
    pub before_days: i64,
    pub after_days: i64,
    pub before_months: u32,
    pub after_months: u32,
    /// Asset classes covered. Empty = all.
    pub classes: Vec<String>,
}

impl WashRule {
    fn covers(&self, class: &str) -> bool {
        self.classes.is_empty() || self.classes.iter().any(|c| c == class)
    }
    fn start(&self, t: OffsetDateTime) -> OffsetDateTime {
        let d = add_months(t.date(), -(self.before_months as i32)) - Duration::days(self.before_days);
        d.with_time(time::Time::MIDNIGHT).assume_offset(t.offset())
    }
    fn end(&self, t: OffsetDateTime) -> OffsetDateTime {
        let d = add_months(t.date(), self.after_months as i32) + Duration::days(self.after_days);
        d.with_time(time::Time::from_hms(23, 59, 59).unwrap()).assume_offset(t.offset())
    }
}

/// `d` moved by whole months, the day clamped to the target month's length.
fn add_months(d: Date, months: i32) -> Date {
    if months == 0 {
        return d;
    }
    let total = d.year() * 12 + (d.month() as i32 - 1) + months;
    let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u8);
    let month = time::Month::try_from(m).expect("1..=12");
    let day = d.day().min(time::util::days_in_month(month, y));
    Date::from_calendar_date(y, month, day).unwrap_or(d)
}

/// Match every instrument's fills under `method`. `allow_short`: a sell with nothing open
/// opens a short (a margin account) instead of being an error (a cash account).
pub fn match_fills(fills: Vec<Fill>, method: Method, allow_short: bool, wash: Option<&WashRule>) -> Outcome {
    let mut per: BTreeMap<String, Vec<Fill>> = BTreeMap::new();
    for f in fills {
        per.entry(f.symbol.trim().to_uppercase()).or_default().push(f);
    }
    let mut out = Outcome::default();
    for (symbol, mut list) in per {
        list.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
        if let Some(f) = list.iter().find(|f| f.rate.is_none()) {
            out.errors.push(format!(
                "{symbol} left out: no {} rate for {}. Add the rate (Settings, FX) and read again",
                f.currency,
                f.at.date()
            ));
            continue;
        }
        let margin = list.iter().any(|f| f.valuation == Valuation::Margin);
        let m = if margin { Method::Fifo } else { method };
        match m {
            Method::UkPool => uk_pool(&symbol, &list, &mut out),
            _ => running(&symbol, &list, m, allow_short || margin, wash, &mut out),
        }
    }
    out
}

/// A lot: per-unit amounts. Long lots carry what a unit cost, short lots what it fetched.
#[derive(Debug, Clone)]
struct Lot {
    qty: f64,
    native: f64,
    tax: f64,
    at: OffsetDateTime,
    /// Quantity already used as replacement shares for a washed loss.
    wash_used: f64,
}

/// A washed loss still looking for replacement shares bought after the sale.
struct Pending {
    until: OffsetDateTime,
    qty: f64,
    per_unit: f64,
    disposal: usize,
}

/// Average and FIFO: a running position per instrument, long or short.
fn running(
    symbol: &str,
    list: &[Fill],
    method: Method,
    allow_short: bool,
    wash: Option<&WashRule>,
    out: &mut Outcome,
) {
    let mut lots: VecDeque<Lot> = VecDeque::new();
    let mut pending: Vec<Pending> = Vec::new();
    // +1 long, -1 short, 0 flat.
    let mut side = 0.0_f64;
    for f in list {
        let rate = f.rate.unwrap_or(1.0);
        let dir = if f.buy { 1.0 } else { -1.0 };
        let mut left = f.qty;
        // Per unit of this fill: a buy pays price + fee, a sell receives price − fee.
        let unit_native = if f.buy { (f.gross + f.fee) / f.qty } else { (f.gross - f.fee) / f.qty };

        // Close against the opposite side first.
        let mut losses: Vec<(usize, f64, f64)> = Vec::new();
        if side != 0.0 && side != dir {
            while left > EPS {
                let Some(lot) = lots.front_mut() else { break };
                let q = left.min(lot.qty);
                let short = side < 0.0;
                // Long closed by a sell: proceeds = this fill, cost = the lot. Short covered
                // by a buy: proceeds = the lot, cost = this fill.
                let (p_native, c_native, p_tax, c_tax) = if short {
                    (q * lot.native, q * unit_native, q * lot.tax, q * unit_native * rate)
                } else {
                    (q * unit_native, q * lot.native, q * unit_native * rate, q * lot.tax)
                };
                let native_gain = p_native - c_native;
                let (proceeds, cost) = match f.valuation {
                    Valuation::Spot => (p_tax, c_tax),
                    Valuation::Margin => (p_native * rate, c_native * rate),
                };
                out.disposals.push(Disposal {
                    fill_id: f.id.clone(),
                    symbol: symbol.to_string(),
                    asset_class: f.asset_class.clone(),
                    bucket: f.bucket,
                    opened_at: (method == Method::Fifo).then_some(lot.at),
                    closed_at: f.at,
                    qty: q,
                    short,
                    proceeds,
                    cost,
                    gain: proceeds - cost,
                    native_gain,
                    currency: f.currency.clone(),
                    holding_days: (method == Method::Fifo).then(|| (f.at - lot.at).whole_days()),
                    rule: method.key(),
                    disallowed: 0.0,
                });
                if !short && proceeds < cost && wash.is_some_and(|w| w.covers(&f.asset_class)) {
                    losses.push((out.disposals.len() - 1, q, (cost - proceeds) / q));
                }
                lot.qty -= q;
                left -= q;
                if lot.qty <= EPS {
                    lots.pop_front();
                }
            }
            if lots.is_empty() {
                side = 0.0;
            }
        }
        // Washed losses: replacement shares bought in the window before the sale and still
        // held take the loss first; the rest waits for purchases in the window after it.
        if let Some(w) = wash {
            let from = w.start(f.at);
            for (idx, q, per_unit) in losses {
                let mut budget = q;
                for lot in lots.iter_mut().filter(|l| side > 0.0 && l.at >= from && l.at <= f.at) {
                    let take = budget.min(lot.qty - lot.wash_used);
                    if take <= EPS {
                        continue;
                    }
                    lot.tax += per_unit * take / lot.qty;
                    lot.wash_used += take;
                    out.disposals[idx].disallowed += per_unit * take;
                    out.disposals[idx].gain += per_unit * take;
                    budget -= take;
                }
                if budget > EPS {
                    pending.push(Pending { until: w.end(f.at), qty: budget, per_unit, disposal: idx });
                }
            }
        }
        if left <= EPS {
            continue;
        }
        // What remains opens (or adds to) a position in this fill's direction.
        if !f.buy && !allow_short {
            out.errors.push(format!(
                "{symbol}: sale of {} on {} has no purchase to match. Move the start of the \
                 read back to the purchase date",
                fmt_qty(left),
                f.at.date()
            ));
            continue;
        }
        side = dir;
        let mut lot = Lot { qty: left, native: unit_native, tax: unit_native * rate, at: f.at, wash_used: 0.0 };
        if f.buy {
            pending.retain(|p| p.until >= f.at && p.qty > EPS);
            for p in pending.iter_mut() {
                let take = p.qty.min(lot.qty - lot.wash_used);
                if take <= EPS {
                    break;
                }
                lot.tax += p.per_unit * take / lot.qty;
                lot.wash_used += take;
                p.qty -= take;
                out.disposals[p.disposal].disallowed += p.per_unit * take;
                out.disposals[p.disposal].gain += p.per_unit * take;
            }
        }
        match (method, lots.back_mut()) {
            // PMP: one pooled lot, per-unit amounts reweighted.
            (Method::Average, Some(pool)) => {
                let q = pool.qty + lot.qty;
                pool.native = (pool.native * pool.qty + lot.native * lot.qty) / q;
                pool.tax = (pool.tax * pool.qty + lot.tax * lot.qty) / q;
                pool.wash_used += lot.wash_used;
                pool.qty = q;
                pool.at = lot.at;
            }
            _ => lots.push_back(lot),
        }
    }
    let qty: f64 = lots.iter().map(|l| l.qty).sum();
    if qty > EPS {
        out.open.push(OpenPosition {
            symbol: symbol.to_string(),
            qty: side * qty,
            basis: lots.iter().map(|l| l.qty * l.tax).sum(),
        });
    }
}

/// One day's fills of one instrument, summed.
#[derive(Default, Clone)]
struct Day {
    buy_qty: f64,
    buy_native: f64,
    buy_tax: f64,
    sell_qty: f64,
    sell_native: f64,
    sell_tax: f64,
    /// Buy quantity already identified with a disposal (same day or an earlier 30-day match).
    buy_used: f64,
    /// Sell quantity already identified.
    sell_used: f64,
    sell_id: String,
    sell_at: Option<OffsetDateTime>,
    asset_class: String,
    bucket: &'static str,
    currency: String,
}

/// HMRC share identification (TCGA 1992 s.104 to s.106A): all same-day acquisitions first,
/// then acquisitions in the 30 days after the disposal (earliest first), then the pool.
/// A cash position only: a sale the three rules cannot cover is an error.
fn uk_pool(symbol: &str, list: &[Fill], out: &mut Outcome) {
    let mut days: BTreeMap<Date, Day> = BTreeMap::new();
    for f in list {
        let rate = f.rate.unwrap_or(1.0);
        let d = days.entry(f.at.date()).or_default();
        d.asset_class = f.asset_class.clone();
        d.bucket = f.bucket;
        d.currency = f.currency.clone();
        if f.buy {
            d.buy_qty += f.qty;
            d.buy_native += f.gross + f.fee;
            d.buy_tax += (f.gross + f.fee) * rate;
        } else {
            d.sell_qty += f.qty;
            d.sell_native += f.gross - f.fee;
            d.sell_tax += (f.gross - f.fee) * rate;
            if d.sell_id.is_empty() {
                d.sell_id = f.id.clone();
            }
            d.sell_at = Some(d.sell_at.map_or(f.at, |a: OffsetDateTime| a.max(f.at)));
        }
    }
    let keys: Vec<Date> = days.keys().copied().collect();

    let push = |out: &mut Outcome, d: &Day, q: f64, c_native: f64, c_tax: f64, rule: &'static str| {
        let p_native = d.sell_native * q / d.sell_qty;
        let p_tax = d.sell_tax * q / d.sell_qty;
        out.disposals.push(Disposal {
            fill_id: d.sell_id.clone(),
            symbol: symbol.to_string(),
            asset_class: d.asset_class.clone(),
            bucket: d.bucket,
            opened_at: None,
            closed_at: d.sell_at.expect("a sell day has a sell"),
            qty: q,
            short: false,
            proceeds: p_tax,
            cost: c_tax,
            gain: p_tax - c_tax,
            native_gain: p_native - c_native,
            currency: d.currency.clone(),
            holding_days: None,
            rule,
            disallowed: 0.0,
        });
    };

    // 1. Same day.
    for k in &keys {
        let d = days.get_mut(k).unwrap();
        let q = d.sell_qty.min(d.buy_qty);
        if q > EPS {
            d.buy_used += q;
            d.sell_used += q;
            let (cn, ct) = (d.buy_native * q / d.buy_qty, d.buy_tax * q / d.buy_qty);
            let snapshot = d.clone();
            push(out, &snapshot, q, cn, ct, "same_day");
        }
    }
    // 2. Bed and breakfast: the next 30 days, earliest first, disposals in date order.
    for (i, k) in keys.iter().enumerate() {
        let mut left = days[k].sell_qty - days[k].sell_used;
        if left <= EPS {
            continue;
        }
        let limit = *k + Duration::days(30);
        for later in keys[i + 1..].iter().take_while(|l| **l <= limit) {
            if left <= EPS {
                break;
            }
            let b = &days[later];
            let avail = b.buy_qty - b.buy_used;
            if avail <= EPS {
                continue;
            }
            let q = left.min(avail);
            let (cn, ct) = (b.buy_native * q / b.buy_qty, b.buy_tax * q / b.buy_qty);
            days.get_mut(later).unwrap().buy_used += q;
            let d = days.get_mut(k).unwrap();
            d.sell_used += q;
            left -= q;
            let snapshot = d.clone();
            push(out, &snapshot, q, cn, ct, "bed_breakfast");
        }
    }
    // 3. The section 104 pool, walked in date order.
    let (mut pq, mut pn, mut pt) = (0.0_f64, 0.0_f64, 0.0_f64);
    for k in &keys {
        let d = days[k].clone();
        let add = d.buy_qty - d.buy_used;
        if add > EPS {
            pn += d.buy_native * add / d.buy_qty;
            pt += d.buy_tax * add / d.buy_qty;
            pq += add;
        }
        let sell = d.sell_qty - d.sell_used;
        if sell <= EPS {
            continue;
        }
        let q = sell.min(pq);
        if q > EPS {
            let (cn, ct) = (pn * q / pq, pt * q / pq);
            pn -= cn;
            pt -= ct;
            pq -= q;
            push(out, &d, q, cn, ct, "s104");
        }
        if sell - q > EPS {
            out.errors.push(format!(
                "{symbol}: sale of {} on {k} has no purchase to match. Move the start of the \
                 read back to the purchase date",
                fmt_qty(sell - q)
            ));
        }
    }
    if pq > EPS {
        out.open.push(OpenPosition { symbol: symbol.to_string(), qty: pq, basis: pt });
    }
}

fn fmt_qty(q: f64) -> String {
    let s = format!("{q:.8}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ── France: digital assets, CGI art. 150 VH bis ─────────────────────────────────────────

/// A crypto event in the French sense: only what crosses into legal tender counts.
/// Crypto-to-crypto swaps (stablecoins included) are neutral and never reach this.
#[derive(Debug, Clone)]
pub struct CryptoEvent {
    pub id: String,
    pub symbol: String,
    pub at: OffsetDateTime,
    /// true: bought with fiat (adds to the total acquisition price). false: sold for fiat.
    pub acquisition: bool,
    /// Fiat amount in the tax currency: paid (fees included) or received (fees deducted).
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CryptoCession {
    pub id: String,
    pub symbol: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub in_year: bool,
    /// Sale price net of fees (C).
    pub proceeds: f64,
    /// Global value of every digital asset held just before the sale (V), as entered.
    pub global_value: Option<f64>,
    /// Net total acquisition price at the time of the sale (A).
    pub acquisition_total: Option<f64>,
    /// A × C / V: the share of acquisition cost this sale consumes.
    pub acquisition_share: Option<f64>,
    pub gain: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct CryptoOutcome {
    pub cessions: Vec<CryptoCession>,
    /// Sum of the year's sale prices: at or under the threshold the year is exempt.
    pub year_proceeds: f64,
    /// Net of the year's gains and losses (losses offset only this year's crypto gains).
    pub year_gain: Option<f64>,
    pub exempt: bool,
    /// Net total acquisition price carried after the last event.
    pub acquisition_left: Option<f64>,
    pub errors: Vec<String>,
}

/// Annual cession total at or under which French crypto gains are exempt (CGI 150 VH bis).
pub const FR_CRYPTO_EXEMPT_EUR: f64 = 305.0;

/// The French global-portfolio method. Every sale since the first purchase matters, not
/// only the year's: each consumes a share of the acquisition price, so a missing global
/// value (V) blocks every later sale rather than being assumed.
///
/// `opening_acquisition` is the net total acquisition price before the first event read
/// (purchases on other platforms, or last year's form 2086 carry).
pub fn fr_global_portfolio(
    mut events: Vec<CryptoEvent>,
    values: &HashMap<String, f64>,
    opening_acquisition: f64,
    year: i32,
) -> CryptoOutcome {
    events.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    let mut out = CryptoOutcome::default();
    let mut a = Some(opening_acquisition.max(0.0));
    let mut year_gain = Some(0.0_f64);
    for e in events {
        let in_year = e.at.year() == year;
        if e.acquisition {
            a = a.map(|x| x + e.amount);
            continue;
        }
        let c = e.amount;
        if in_year {
            out.year_proceeds += c;
        }
        let v = values.get(&e.id).copied().filter(|v| *v > 0.0);
        let computed = match (a, v) {
            (Some(total), Some(v)) if v + 1e-6 >= c => {
                let share = total * c / v;
                Some((total, share, c - share))
            }
            (Some(_), Some(v)) => {
                out.errors.push(format!(
                    "{} on {}: the global value ({v:.2}) cannot be below the sale price ({c:.2})",
                    e.symbol,
                    e.at.date()
                ));
                None
            }
            (Some(_), None) => {
                out.errors.push(format!(
                    "{} on {}: enter the value of all digital assets held just before this sale",
                    e.symbol,
                    e.at.date()
                ));
                None
            }
            (None, _) => None,
        };
        match computed {
            Some((total, share, gain)) => {
                a = Some(total - share);
                if in_year {
                    year_gain = year_gain.map(|g| g + gain);
                }
                out.cessions.push(CryptoCession {
                    id: e.id,
                    symbol: e.symbol,
                    at: e.at,
                    in_year,
                    proceeds: c,
                    global_value: v,
                    acquisition_total: Some(total),
                    acquisition_share: Some(share),
                    gain: Some(gain),
                });
            }
            None => {
                // Everything after an undetermined sale is undetermined too.
                a = None;
                if in_year {
                    year_gain = None;
                }
                out.cessions.push(CryptoCession {
                    id: e.id,
                    symbol: e.symbol,
                    at: e.at,
                    in_year,
                    proceeds: c,
                    global_value: v,
                    acquisition_total: None,
                    acquisition_share: None,
                    gain: None,
                });
            }
        }
    }
    out.exempt = out.year_proceeds <= FR_CRYPTO_EXEMPT_EUR;
    out.year_gain = year_gain;
    out.acquisition_left = a;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn fill(id: &str, at: OffsetDateTime, buy: bool, qty: f64, price: f64, rate: f64) -> Fill {
        Fill {
            id: id.into(),
            symbol: "AAPL".into(),
            asset_class: "stock".into(),
            bucket: "capital",
            valuation: Valuation::Spot,
            at,
            buy,
            qty,
            gross: qty * price,
            fee: 0.0,
            currency: "USD".into(),
            rate: Some(rate),
        }
    }

    /// Each leg at its own rate: bought 100 USD at 0.80, sold 100 USD at 1.00. The native
    /// gain is zero, the tax-currency gain is the currency move (+20).
    #[test]
    fn legs_convert_at_their_own_dates() {
        let out = match_fills(
            vec![
                fill("b", datetime!(2025-03-01 10:00 UTC), true, 1.0, 100.0, 0.80),
                fill("s", datetime!(2025-10-01 10:00 UTC), false, 1.0, 100.0, 1.00),
            ],
            Method::Fifo,
            false,
            None,
        );
        let d = &out.disposals[0];
        assert_eq!(d.native_gain, 0.0);
        assert!((d.gain - 20.0).abs() < 1e-9);
        assert_eq!(d.holding_days, Some(214));
    }

    /// Margin products convert the realized PnL on the close date, not the notional.
    #[test]
    fn margin_converts_the_pnl_on_the_close_date() {
        let mut b = fill("b", datetime!(2025-03-01 10:00 UTC), true, 1.0, 5000.0, 0.80);
        let mut s = fill("s", datetime!(2025-03-05 10:00 UTC), false, 1.0, 5100.0, 0.90);
        b.valuation = Valuation::Margin;
        s.valuation = Valuation::Margin;
        let out = match_fills(vec![b, s], Method::Average, false, None);
        assert!((out.disposals[0].gain - 90.0).abs() < 1e-9);
    }

    /// PMP: 10 at 100 then 10 at 200, sell 10 at 180: cost 1500, gain 300. FIFO: cost 1000.
    #[test]
    fn average_and_fifo_differ() {
        let fills = || {
            vec![
                fill("b1", datetime!(2025-01-02 10:00 UTC), true, 10.0, 100.0, 1.0),
                fill("b2", datetime!(2025-02-02 10:00 UTC), true, 10.0, 200.0, 1.0),
                fill("s1", datetime!(2025-03-02 10:00 UTC), false, 10.0, 180.0, 1.0),
            ]
        };
        let avg = match_fills(fills(), Method::Average, false, None);
        assert_eq!(avg.disposals.len(), 1);
        assert!((avg.disposals[0].gain - 300.0).abs() < 1e-9);
        assert!((avg.open[0].basis - 1500.0).abs() < 1e-9);
        let fifo = match_fills(fills(), Method::Fifo, false, None);
        assert!((fifo.disposals[0].gain - 800.0).abs() < 1e-9);
    }

    /// FIFO splits a sale across lots so each carries its own holding period.
    #[test]
    fn fifo_splits_across_lots() {
        let out = match_fills(
            vec![
                fill("b1", datetime!(2024-01-02 10:00 UTC), true, 5.0, 100.0, 1.0),
                fill("b2", datetime!(2025-01-02 10:00 UTC), true, 5.0, 100.0, 1.0),
                fill("s1", datetime!(2025-02-02 10:00 UTC), false, 8.0, 100.0, 1.0),
            ],
            Method::Fifo,
            false,
            None,
        );
        assert_eq!(out.disposals.len(), 2);
        assert_eq!(out.disposals[0].qty, 5.0);
        assert!(out.disposals[0].holding_days.unwrap() > 365);
        assert_eq!(out.disposals[1].qty, 3.0);
        assert!(out.disposals[1].holding_days.unwrap() < 365);
    }

    /// A cash account's orphan sale is an error, never a gain against nothing.
    #[test]
    fn orphan_sale_is_an_error() {
        let out = match_fills(
            vec![fill("s", datetime!(2025-03-02 10:00 UTC), false, 1.0, 100.0, 1.0)],
            Method::Average,
            false,
            None,
        );
        assert!(out.disposals.is_empty());
        assert_eq!(out.errors.len(), 1);
    }

    /// An instrument with an unpriced fill is left out whole.
    #[test]
    fn missing_rate_drops_the_instrument() {
        let mut b = fill("b", datetime!(2025-03-01 10:00 UTC), true, 1.0, 100.0, 1.0);
        b.rate = None;
        let s = fill("s", datetime!(2025-10-01 10:00 UTC), false, 1.0, 120.0, 1.0);
        let out = match_fills(vec![b, s], Method::Fifo, false, None);
        assert!(out.disposals.is_empty());
        assert_eq!(out.errors.len(), 1);
    }

    /// A short is opened and covered on a margin account.
    #[test]
    fn short_round_trip() {
        let out = match_fills(
            vec![
                fill("s", datetime!(2025-03-01 10:00 UTC), false, 2.0, 100.0, 1.0),
                fill("b", datetime!(2025-03-03 10:00 UTC), true, 2.0, 90.0, 1.0),
            ],
            Method::Fifo,
            true,
            None,
        );
        assert!(out.disposals[0].short);
        assert!((out.disposals[0].gain - 20.0).abs() < 1e-9);
    }

    /// Wash sale: a loss with a repurchase 10 days later is deferred onto the new shares.
    #[test]
    fn wash_defers_onto_the_repurchase() {
        let w = WashRule { before_days: 30, after_days: 30, ..Default::default() };
        let out = match_fills(
            vec![
                fill("b1", datetime!(2025-01-02 10:00 UTC), true, 10.0, 100.0, 1.0),
                fill("s1", datetime!(2025-03-01 10:00 UTC), false, 10.0, 80.0, 1.0),
                fill("b2", datetime!(2025-03-11 10:00 UTC), true, 10.0, 85.0, 1.0),
                fill("s2", datetime!(2025-06-01 10:00 UTC), false, 10.0, 90.0, 1.0),
            ],
            Method::Fifo,
            false,
            Some(&w),
        );
        assert!((out.disposals[0].gain - 0.0).abs() < 1e-9, "the 200 loss is deferred");
        assert!((out.disposals[0].disallowed - 200.0).abs() < 1e-9);
        // Replacement cost 850 + 200 = 1050, sold 900: the loss comes back.
        assert!((out.disposals[1].gain + 150.0).abs() < 1e-9);
    }

    /// Months windows clamp to the month's length.
    #[test]
    fn add_months_clamps() {
        assert_eq!(add_months(time::macros::date!(2025 - 03 - 31), -1), time::macros::date!(2025 - 02 - 28));
        assert_eq!(add_months(time::macros::date!(2025 - 12 - 15), 2), time::macros::date!(2026 - 02 - 15));
    }

    /// HMRC HS284 shape: pool of 1000 at £4000, sell 500 on day 1, rebuy 300 ten days later.
    /// 300 match the rebuy (bed and breakfast), 200 come out of the pool.
    #[test]
    fn uk_same_day_then_thirty_days_then_pool() {
        let out = match_fills(
            vec![
                fill("b0", datetime!(2024-05-01 10:00 UTC), true, 1000.0, 4.0, 1.0),
                fill("s1", datetime!(2025-06-01 10:00 UTC), false, 500.0, 6.0, 1.0),
                fill("b1", datetime!(2025-06-01 15:00 UTC), true, 100.0, 5.5, 1.0),
                fill("b2", datetime!(2025-06-11 10:00 UTC), true, 300.0, 5.0, 1.0),
            ],
            Method::UkPool,
            false,
            None,
        );
        let rule = |r: &str| out.disposals.iter().find(|d| d.rule == r).unwrap();
        assert_eq!(rule("same_day").qty, 100.0);
        assert!((rule("same_day").cost - 550.0).abs() < 1e-9);
        assert_eq!(rule("bed_breakfast").qty, 300.0);
        assert!((rule("bed_breakfast").cost - 1500.0).abs() < 1e-9);
        assert_eq!(rule("s104").qty, 100.0);
        assert!((rule("s104").cost - 400.0).abs() < 1e-9);
        // The 30-day rebuy never enters the pool: 900 left at £4 each.
        assert!((out.open[0].qty - 900.0).abs() < 1e-9);
        assert!((out.open[0].basis - 3600.0).abs() < 1e-9);
    }

    /// BOFiP shape: 10 000 invested, a 3 000 sale when the portfolio is worth 15 000 consumes
    /// 2 000 of acquisition price (gain 1 000); a second 4 000 sale at 8 000 consumes 4 000.
    #[test]
    fn fr_crypto_global_portfolio() {
        let ev = |id: &str, at, acq, amount| CryptoEvent {
            id: id.into(),
            symbol: "BTC".into(),
            at,
            acquisition: acq,
            amount,
        };
        let events = vec![
            ev("a", datetime!(2024-01-10 10:00 UTC), true, 10_000.0),
            ev("c1", datetime!(2025-02-10 10:00 UTC), false, 3_000.0),
            ev("c2", datetime!(2025-09-10 10:00 UTC), false, 4_000.0),
        ];
        let values = HashMap::from([("c1".to_string(), 15_000.0), ("c2".to_string(), 8_000.0)]);
        let out = fr_global_portfolio(events, &values, 0.0, 2025);
        assert!((out.cessions[0].gain.unwrap() - 1_000.0).abs() < 1e-9);
        assert!((out.cessions[1].acquisition_total.unwrap() - 8_000.0).abs() < 1e-9);
        assert!((out.cessions[1].gain.unwrap() - 0.0).abs() < 1e-9);
        assert!((out.year_gain.unwrap() - 1_000.0).abs() < 1e-9);
        assert!((out.acquisition_left.unwrap() - 4_000.0).abs() < 1e-9);
        assert!(!out.exempt);
    }

    /// A missing global value blocks that sale and every later one.
    #[test]
    fn fr_crypto_missing_value_blocks() {
        let events = vec![
            CryptoEvent { id: "a".into(), symbol: "BTC".into(), at: datetime!(2025-01-01 10:00 UTC), acquisition: true, amount: 1000.0 },
            CryptoEvent { id: "c1".into(), symbol: "BTC".into(), at: datetime!(2025-02-01 10:00 UTC), acquisition: false, amount: 100.0 },
            CryptoEvent { id: "c2".into(), symbol: "BTC".into(), at: datetime!(2025-03-01 10:00 UTC), acquisition: false, amount: 100.0 },
        ];
        let values = HashMap::from([("c2".to_string(), 900.0)]);
        let out = fr_global_portfolio(events, &values, 0.0, 2025);
        assert!(out.cessions.iter().all(|c| c.gain.is_none()));
        assert!(out.year_gain.is_none());
        assert!(out.exempt, "200 of sales is under the 305 threshold");
    }
}
