//! The paper broker's state: cash moved by the user, orders, and the fills they produced.
//!
//! Not the backtest's paper sessions ([`crate::paper`]): this is a broker account the user
//! trades by hand from a chart, run by the app instead of a venue. A position is never stored:
//! it is the fold of the account's fills on one instrument ([`fold`]), so it cannot disagree
//! with the execution history the journal reads.
//!
//! Prices, quantities, fees, rates and money are [`Decimal`] (NUMERIC columns), as a venue
//! books them: a fill sums to flat exactly and a price stays on its tick. They are written to
//! JSON as numbers, the way the page reads them.

#![deny(clippy::float_arithmetic)]

use anyhow::Context;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

/// Statuses of an order that is still alive: an exit `held` until its entry fills, or
/// `working` on the market.
pub const OPEN: &[&str] = &["held", "working"];

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CashMove {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub kind: String,
    #[serde(serialize_with = "number")]
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Order {
    pub id: Uuid,
    pub account_id: Uuid,
    pub key: String,
    pub connector_id: Uuid,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub currency: String,
    #[serde(serialize_with = "number")]
    pub multiplier: Decimal,
    #[serde(serialize_with = "number_opt")]
    pub tick: Option<Decimal>,
    pub side: String,
    pub order_type: String,
    #[serde(serialize_with = "number")]
    pub qty: Decimal,
    #[serde(serialize_with = "number_opt")]
    pub limit_price: Option<Decimal>,
    #[serde(serialize_with = "number_opt")]
    pub stop_price: Option<Decimal>,
    pub tif: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub parent_id: Option<Uuid>,
    pub oca_group: Option<String>,
    pub status: String,
    pub triggered: bool,
    pub reason: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub placed_at: OffsetDateTime,
    /// It may only shrink the position: capped to what is held at its fill, cancelled when
    /// there is nothing to reduce.
    pub reduce_only: bool,
    /// A limit that only rests: refused at placement when it would fill at once.
    pub post_only: bool,
    /// A trailing stop's distance, one of the two.
    #[serde(serialize_with = "number_opt")]
    pub trail_amount: Option<Decimal>,
    #[serde(serialize_with = "number_opt")]
    pub trail_pct: Option<Decimal>,
    /// A trailing stop's best price since it started.
    #[serde(serialize_with = "number_opt")]
    pub trail_best: Option<Decimal>,
}

impl Order {
    pub fn buy(&self) -> bool {
        self.side == "buy"
    }
}

const ORDER_COLS: &str = "id, account_id, key, connector_id, provider, asset_type, ticker, currency, multiplier, \
     tick, side, order_type, qty, limit_price, stop_price, tif, expires_at, parent_id, oca_group, status, \
     triggered, reason, placed_at, reduce_only, post_only, trail_amount, trail_pct, trail_best";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Fill {
    pub id: Uuid,
    pub account_id: Uuid,
    pub order_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub connector_id: Uuid,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub side: String,
    #[serde(serialize_with = "number")]
    pub qty: Decimal,
    #[serde(serialize_with = "number")]
    pub price: Decimal,
    #[serde(serialize_with = "number")]
    pub fee: Decimal,
    pub currency: String,
    #[serde(serialize_with = "number")]
    pub multiplier: Decimal,
    #[serde(serialize_with = "number")]
    pub realized: Decimal,
    #[serde(serialize_with = "number")]
    pub fx: Decimal,
}

const FILL_COLS: &str = "id, account_id, order_id, at, connector_id, provider, asset_type, ticker, side, qty, \
     price, fee, currency, multiplier, realized, fx";

/// A decimal written to JSON as a number, as the page reads it.
fn number<S: serde::Serializer>(v: &Decimal, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(f64::try_from(*v).map_err(serde::ser::Error::custom)?)
}

fn number_opt<S: serde::Serializer>(v: &Option<Decimal>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(v) => number(v, s),
        None => s.serialize_none(),
    }
}

// ── Positions ────────────────────────────────────────────────────────────────

/// One instrument's position, folded from its fills in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Held {
    /// Signed: positive long, negative short.
    pub qty: Decimal,
    /// Weighted average cost of what is open. 0 when flat.
    pub avg: Decimal,
    /// When the open position started (the fill that opened it from flat).
    pub opened_at: Option<OffsetDateTime>,
    /// Commission paid on the fills that built what is open, instrument currency.
    pub entry_fee: Decimal,
}

impl Held {
    /// Apply one fill and say what it realized (instrument currency, before fees): a fill
    /// that reduces the position closes at the weighted average cost, and one that goes
    /// through zero opens the rest on the other side at its own price.
    pub fn apply(&mut self, buy: bool, qty: Decimal, price: Decimal, fee: Decimal, multiplier: Decimal, at: OffsetDateTime) -> Decimal {
        let signed = if buy { qty } else { -qty };
        if self.qty.is_zero() || self.qty.is_sign_positive() == signed.is_sign_positive() {
            let total = self.qty.abs() + qty;
            self.avg = if total > Decimal::ZERO { (self.avg * self.qty.abs() + price * qty) / total } else { Decimal::ZERO };
            if self.qty.is_zero() {
                self.opened_at = Some(at);
                self.entry_fee = Decimal::ZERO;
            }
            self.entry_fee += fee;
            self.qty += signed;
            return Decimal::ZERO;
        }
        let closing = qty.min(self.qty.abs());
        let long = self.qty > Decimal::ZERO;
        let realized = (if long { price - self.avg } else { self.avg - price }) * closing * multiplier;
        let left = self.qty.abs() - closing;
        self.entry_fee = self.entry_fee * left / self.qty.abs();
        self.qty += signed;
        if self.qty.is_zero() {
            *self = Held::default();
        } else if qty > closing {
            // Through zero: the remainder opens the other side.
            self.avg = price;
            self.opened_at = Some(at);
            self.entry_fee = fee * (qty - closing) / qty;
        }
        realized
    }

    pub fn flat(&self) -> bool {
        self.qty.is_zero()
    }
}

/// The position these fills leave, in the order given.
pub fn fold<'a>(fills: impl IntoIterator<Item = &'a Fill>) -> Held {
    let mut h = Held::default();
    for f in fills {
        h.apply(f.side == "buy", f.qty, f.price, f.fee, f.multiplier, f.at);
    }
    h
}

// ── Cash ─────────────────────────────────────────────────────────────────────

pub async fn cash_moves(pool: &PgPool, account: Uuid) -> anyhow::Result<Vec<CashMove>> {
    Ok(sqlx::query_as::<_, CashMove>(
        "SELECT id, at, kind, amount FROM paper_cash WHERE account_id = $1 ORDER BY at, id",
    )
    .bind(account)
    .fetch_all(pool)
    .await?)
}

/// Record a deposit or a withdrawal, in the account's currency.
pub async fn book_cash(pool: &PgPool, account: Uuid, kind: &str, amount: Decimal) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO paper_cash (account_id, kind, amount) VALUES ($1, $2, $3)")
        .bind(account)
        .bind(kind)
        .bind(amount)
        .execute(pool)
        .await
        .context("recording a paper cash move")?;
    Ok(())
}

/// The decimal a float reads as (`0.1` is 0.1, not its binary neighbour).
pub fn decimal_of(v: f64) -> anyhow::Result<Decimal> {
    if !v.is_finite() {
        anyhow::bail!("{v} is not a number");
    }
    format!("{v}").parse::<Decimal>().map_err(|_| anyhow::anyhow!("{v} is out of range"))
}

/// Start over: every order, fill and cash move goes, and the account holds `amount`.
pub async fn reset(pool: &PgPool, account: Uuid, amount: Decimal) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    for table in ["paper_fills", "paper_orders", "paper_cash"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table} WHERE account_id = $1")))
            .bind(account)
            .execute(&mut *tx)
            .await?;
    }
    if amount > Decimal::ZERO {
        sqlx::query("INSERT INTO paper_cash (account_id, kind, amount) VALUES ($1, 'deposit', $2)")
            .bind(account)
            .bind(amount)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ── Orders ───────────────────────────────────────────────────────────────────

/// An order as it is written. Its status is `held` when it waits on a parent.
pub struct NewOrder {
    pub id: Uuid,
    pub key: String,
    pub connector_id: Uuid,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub currency: String,
    pub multiplier: Decimal,
    pub tick: Option<Decimal>,
    pub side: String,
    pub order_type: String,
    pub qty: Decimal,
    pub limit_price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub tif: String,
    pub expires_at: Option<OffsetDateTime>,
    pub parent_id: Option<Uuid>,
    pub oca_group: Option<String>,
    pub reduce_only: bool,
    pub post_only: bool,
    pub trail_amount: Option<Decimal>,
    pub trail_pct: Option<Decimal>,
}

/// Write a ticket's orders together: all of them or none.
pub async fn insert_orders(pool: &PgPool, account: Uuid, orders: &[NewOrder]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    for o in orders {
        sqlx::query(
            "INSERT INTO paper_orders (id, account_id, key, connector_id, provider, asset_type, ticker, currency, \
             multiplier, tick, side, order_type, qty, limit_price, stop_price, tif, expires_at, parent_id, \
             oca_group, status, reduce_only, post_only, trail_amount, trail_pct) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24)",
        )
        .bind(o.id)
        .bind(account)
        .bind(&o.key)
        .bind(o.connector_id)
        .bind(&o.provider)
        .bind(&o.asset_type)
        .bind(&o.ticker)
        .bind(&o.currency)
        .bind(o.multiplier)
        .bind(o.tick)
        .bind(&o.side)
        .bind(&o.order_type)
        .bind(o.qty)
        .bind(o.limit_price)
        .bind(o.stop_price)
        .bind(&o.tif)
        .bind(o.expires_at)
        .bind(o.parent_id)
        .bind(&o.oca_group)
        .bind(if o.parent_id.is_some() { "held" } else { "working" })
        .bind(o.reduce_only)
        .bind(o.post_only)
        .bind(o.trail_amount)
        .bind(o.trail_pct)
        .execute(&mut *tx)
        .await
        .context("writing a paper order")?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn order(pool: &PgPool, account: Uuid, id: Uuid) -> anyhow::Result<Option<Order>> {
    let sql = format!("SELECT {ORDER_COLS} FROM paper_orders WHERE account_id = $1 AND id = $2");
    Ok(sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql))
        .bind(account)
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// The account's order a chart sent with this client key (the newest, should it have sent the
/// key twice), with what has filled of it.
pub async fn by_key(pool: &PgPool, account: Uuid, key: &str) -> anyhow::Result<Option<(Order, Decimal)>> {
    let sql = format!("SELECT {ORDER_COLS} FROM paper_orders WHERE account_id = $1 AND key = $2 ORDER BY placed_at DESC, id LIMIT 1");
    let Some(o) = sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql)).bind(account).bind(key).fetch_optional(pool).await? else {
        return Ok(None);
    };
    let (filled,): (Option<Decimal>,) = sqlx::query_as("SELECT SUM(qty) FROM paper_fills WHERE order_id = $1").bind(o.id).fetch_one(pool).await?;
    Ok(Some((o, filled.unwrap_or_default())))
}

/// The account's live orders, oldest first.
pub async fn open_orders(pool: &PgPool, account: Uuid) -> anyhow::Result<Vec<Order>> {
    let sql = format!(
        "SELECT {ORDER_COLS} FROM paper_orders WHERE account_id = $1 AND status IN ('held', 'working') \
         ORDER BY placed_at, id"
    );
    Ok(sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql)).bind(account).fetch_all(pool).await?)
}

/// The account's orders that stopped working (filled, cancelled, rejected, expired), newest
/// first: where a rejection's reason is read.
pub async fn closed_orders(pool: &PgPool, account: Uuid, limit: i64) -> anyhow::Result<Vec<Order>> {
    let sql = format!(
        "SELECT {ORDER_COLS} FROM paper_orders WHERE account_id = $1 AND status NOT IN ('held', 'working') \
         ORDER BY updated_at DESC, id LIMIT $2"
    );
    Ok(sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql)).bind(account).bind(limit).fetch_all(pool).await?)
}

/// Every working order of every paper account: what the engine watches.
pub async fn working(pool: &PgPool) -> anyhow::Result<Vec<Order>> {
    let sql = format!("SELECT {ORDER_COLS} FROM paper_orders WHERE status = 'working' ORDER BY placed_at, id");
    Ok(sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

/// The orders a stop of an order ends with it: its exits, which have nothing left to close.
async fn end_children(tx: &mut Transaction<'_, Postgres>, parent: Uuid) -> anyhow::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "UPDATE paper_orders SET status = 'cancelled', reason = 'its entry stopped working', updated_at = now() \
         WHERE parent_id = $1 AND status IN ('held', 'working') RETURNING id",
    )
    .bind(parent)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Stop a live order (`cancelled`, `rejected`, `expired`) with the exits waiting on it. Answers
/// every id that stopped, the order first; empty when it was no longer live.
pub async fn stop(pool: &PgPool, account: Uuid, id: Uuid, status: &str, reason: Option<&str>) -> anyhow::Result<Vec<Uuid>> {
    let mut tx = pool.begin().await?;
    let out = stop_in(&mut tx, account, id, status, reason).await?;
    tx.commit().await?;
    Ok(out)
}

async fn stop_in(tx: &mut Transaction<'_, Postgres>, account: Uuid, id: Uuid, status: &str, reason: Option<&str>) -> anyhow::Result<Vec<Uuid>> {
    let done = sqlx::query(
        "UPDATE paper_orders SET status = $3, reason = $4, updated_at = now() \
         WHERE account_id = $1 AND id = $2 AND status IN ('held', 'working')",
    )
    .bind(account)
    .bind(id)
    .bind(status)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    if done.rows_affected() == 0 {
        return Ok(Vec::new());
    }
    let mut out = vec![id];
    out.extend(end_children(tx, id).await?);
    Ok(out)
}

/// A stop-limit whose stop was reached: from now on it rests as its limit.
pub async fn trigger(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE paper_orders SET triggered = TRUE, updated_at = now() WHERE id = $1 AND status = 'working'")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A trailing stop's best price since it started, kept as the market moves it on.
pub async fn set_trail_best(pool: &PgPool, id: Uuid, best: Decimal) -> anyhow::Result<()> {
    sqlx::query("UPDATE paper_orders SET trail_best = $2, updated_at = now() WHERE id = $1 AND status = 'working'")
        .bind(id)
        .bind(best)
        .execute(pool)
        .await?;
    Ok(())
}

/// A change to a live order's prices, size or time in force. `None` leaves a field alone.
pub struct Amend {
    pub limit: Option<Decimal>,
    pub stop: Option<Decimal>,
    pub qty: Option<Decimal>,
    pub tif: Option<String>,
    pub expires_at: Option<Option<OffsetDateTime>>,
}

pub async fn amend(pool: &PgPool, account: Uuid, id: Uuid, a: &Amend) -> anyhow::Result<bool> {
    let (set_expiry, expiry) = match a.expires_at {
        Some(e) => (true, e),
        None => (false, None),
    };
    let res = sqlx::query(
        "UPDATE paper_orders SET \
           limit_price = COALESCE($3, limit_price), stop_price = COALESCE($4, stop_price), \
           qty = COALESCE($5, qty), tif = COALESCE($6, tif), \
           expires_at = CASE WHEN $7 THEN $8 ELSE expires_at END, updated_at = now() \
         WHERE account_id = $1 AND id = $2 AND status IN ('held', 'working')",
    )
    .bind(account)
    .bind(id)
    .bind(a.limit)
    .bind(a.stop)
    .bind(a.qty)
    .bind(&a.tif)
    .bind(set_expiry)
    .bind(expiry)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Live orders whose time is up (a day order past its day, a good-till-date past its date),
/// marked expired with their exits. Answers (account, ids) per order that expired.
pub async fn expire_due(pool: &PgPool, now: OffsetDateTime) -> anyhow::Result<Vec<(Uuid, Vec<Uuid>)>> {
    let due: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT account_id, id FROM paper_orders \
         WHERE status IN ('held', 'working') AND expires_at IS NOT NULL AND expires_at <= $1",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for (account, id) in due {
        let ids = stop(pool, account, id, "expired", Some("its time in force ran out")).await?;
        if !ids.is_empty() {
            out.push((account, ids));
        }
    }
    Ok(out)
}

// ── Fills ────────────────────────────────────────────────────────────────────

/// What filling an order changed besides the order itself.
#[derive(Debug, Default)]
pub struct Filled {
    pub fill: Option<Fill>,
    /// A reduce-only order cancelled instead, with the reason: nothing left to reduce.
    pub cancelled_why: Option<&'static str>,
    /// Exits that start working now their entry filled.
    pub activated: Vec<Order>,
    /// Orders of the same one-cancels-other group, cancelled by this fill.
    pub cancelled: Vec<Uuid>,
}

/// Fill a working order whole at `price`, with `fee` (instrument currency) and `fx` (instrument
/// currency to account currency): the fill is written with the result it realized, the order
/// marked filled, its exits set working and the rest of its group cancelled, in one
/// transaction. Nothing when the order stopped working in the meantime.
pub async fn fill(pool: &PgPool, id: Uuid, price: Decimal, fee: Decimal, fx: Decimal, at: OffsetDateTime) -> anyhow::Result<Filled> {
    let mut tx = pool.begin().await?;
    let sql = format!("SELECT {ORDER_COLS} FROM paper_orders WHERE id = $1 AND status = 'working' FOR UPDATE");
    let Some(o) = sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql)).bind(id).fetch_optional(&mut *tx).await? else {
        return Ok(Filled::default());
    };
    let sql = format!(
        "SELECT {FILL_COLS} FROM paper_fills WHERE account_id = $1 AND provider = $2 AND asset_type = $3 \
         AND ticker = $4 ORDER BY at, id"
    );
    let prior = sqlx::query_as::<_, Fill>(sqlx::AssertSqlSafe(sql))
        .bind(o.account_id)
        .bind(&o.provider)
        .bind(&o.asset_type)
        .bind(&o.ticker)
        .fetch_all(&mut *tx)
        .await?;
    let mut held = fold(&prior);
    // Reduce-only: never more than what is held on the other side, and nothing when the
    // position is flat or on the order's own side.
    let mut qty = o.qty;
    let mut fee = fee;
    if o.reduce_only {
        let against = if o.buy() { -held.qty } else { held.qty };
        if against <= Decimal::ZERO {
            let why = "reduce-only: there was no position to reduce";
            let ids = stop_in(&mut tx, o.account_id, o.id, "cancelled", Some(why)).await?;
            tx.commit().await?;
            return Ok(Filled { cancelled_why: Some(why), cancelled: ids, ..Default::default() });
        }
        if against < qty {
            fee = fee * against / qty;
            qty = against;
            sqlx::query("UPDATE paper_orders SET qty = $2 WHERE id = $1").bind(o.id).bind(qty).execute(&mut *tx).await?;
        }
    }
    let realized = held.apply(o.buy(), qty, price, fee, o.multiplier, at);
    let f = Fill {
        id: Uuid::new_v4(),
        account_id: o.account_id,
        order_id: o.id,
        at,
        connector_id: o.connector_id,
        provider: o.provider.clone(),
        asset_type: o.asset_type.clone(),
        ticker: o.ticker.clone(),
        side: o.side.clone(),
        qty,
        price,
        fee,
        currency: o.currency.clone(),
        multiplier: o.multiplier,
        realized,
        fx,
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO paper_fills ({FILL_COLS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)"
    )))
    .bind(f.id)
    .bind(f.account_id)
    .bind(f.order_id)
    .bind(f.at)
    .bind(f.connector_id)
    .bind(&f.provider)
    .bind(&f.asset_type)
    .bind(&f.ticker)
    .bind(&f.side)
    .bind(f.qty)
    .bind(f.price)
    .bind(f.fee)
    .bind(&f.currency)
    .bind(f.multiplier)
    .bind(f.realized)
    .bind(f.fx)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE paper_orders SET status = 'filled', updated_at = now() WHERE id = $1")
        .bind(o.id)
        .execute(&mut *tx)
        .await?;
    let sql = format!(
        "UPDATE paper_orders SET status = 'working', updated_at = now() \
         WHERE parent_id = $1 AND status = 'held' RETURNING {ORDER_COLS}"
    );
    let activated = sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(sql)).bind(o.id).fetch_all(&mut *tx).await?;
    let mut cancelled = Vec::new();
    if let Some(group) = &o.oca_group {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "UPDATE paper_orders SET status = 'cancelled', reason = 'another order of its group filled', \
             updated_at = now() \
             WHERE account_id = $1 AND oca_group = $2 AND id <> $3 AND status IN ('held', 'working') RETURNING id",
        )
        .bind(o.account_id)
        .bind(group)
        .bind(o.id)
        .fetch_all(&mut *tx)
        .await?;
        for (gone,) in rows {
            cancelled.push(gone);
            cancelled.extend(end_children(&mut tx, gone).await?);
        }
    }
    tx.commit().await?;
    Ok(Filled { fill: Some(f), activated, cancelled, cancelled_why: None })
}

/// The account's fills over a window, oldest first.
pub async fn fills(pool: &PgPool, account: Uuid, from: OffsetDateTime, to: OffsetDateTime) -> anyhow::Result<Vec<Fill>> {
    let sql = format!("SELECT {FILL_COLS} FROM paper_fills WHERE account_id = $1 AND at >= $2 AND at <= $3 ORDER BY at, id");
    Ok(sqlx::query_as::<_, Fill>(sqlx::AssertSqlSafe(sql))
        .bind(account)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?)
}

/// Every fill of the account, oldest first: what its positions and cash fold from.
pub async fn all_fills(pool: &PgPool, account: Uuid) -> anyhow::Result<Vec<Fill>> {
    let sql = format!("SELECT {FILL_COLS} FROM paper_fills WHERE account_id = $1 ORDER BY at, id");
    Ok(sqlx::query_as::<_, Fill>(sqlx::AssertSqlSafe(sql)).bind(account).fetch_all(pool).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(v: &str) -> Decimal {
        v.parse().unwrap()
    }

    fn t(s: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000 + s).unwrap()
    }

    #[test]
    fn adding_to_a_position_averages_its_cost_and_reducing_it_realizes_at_that_cost() {
        let mut h = Held::default();
        assert_eq!(h.apply(true, d("10"), d("100"), d("1"), d("1"), t(0)), d("0"));
        assert_eq!(h.apply(true, d("10"), d("110"), d("1"), d("1"), t(1)), d("0"));
        assert_eq!((h.qty, h.avg, h.opened_at), (d("20"), d("105"), Some(t(0))));
        // Half out at 120: 10 x (120 - 105).
        assert_eq!(h.apply(false, d("10"), d("120"), d("1"), d("1"), t(2)), d("150"));
        assert_eq!((h.qty, h.avg), (d("10"), d("105")));
        assert_eq!(h.entry_fee, d("1"), "half the entry fee stays with half the position");
        assert_eq!(h.apply(false, d("10"), d("100"), d("1"), d("1"), t(3)), d("-50"));
        assert!(h.flat());
        assert_eq!(h, Held::default());
    }

    #[test]
    fn a_fill_through_zero_closes_one_side_and_opens_the_other_at_its_price() {
        let mut h = Held::default();
        h.apply(true, d("2"), d("50"), d("0"), d("10"), t(0));
        // Sell 5 at 55: 2 close (+5 x 2 x 10 = 100), 3 open short at 55.
        assert_eq!(h.apply(false, d("5"), d("55"), d("5"), d("10"), t(1)), d("100"));
        assert_eq!((h.qty, h.avg, h.opened_at), (d("-3"), d("55"), Some(t(1))));
        assert_eq!(h.entry_fee, d("3"));
        // Buy back at 50: a short earns the drop.
        assert_eq!(h.apply(true, d("3"), d("50"), d("0"), d("10"), t(2)), d("150"));
        assert!(h.flat());
    }

    #[test]
    fn a_tenth_reads_as_a_tenth_and_three_of_them_close_flat() {
        assert_eq!(decimal_of(0.1).unwrap(), d("0.1"));
        assert_eq!(decimal_of(5000.25).unwrap(), d("5000.25"));
        assert!(decimal_of(f64::NAN).is_err());
        let mut h = Held::default();
        for _ in 0..3 {
            h.apply(true, d("0.1"), d("100"), d("0"), d("1"), t(0));
        }
        assert_eq!(h.qty, d("0.3"));
        h.apply(false, d("0.3"), d("101"), d("0"), d("1"), t(1));
        assert_eq!(h, Held::default());
    }
}
