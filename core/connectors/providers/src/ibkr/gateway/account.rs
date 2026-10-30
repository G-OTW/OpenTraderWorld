//! An IB account as the socket sees it: what it holds, valued at IB's own market price, and
//! the fills of the last days that opened it.
//!
//! The portfolio is IB's account update stream (`reqAccountUpdates`), read to its end once
//! per account and dropped: it carries the market price and the unrealized result a bare
//! position list does not. When a position opened is found by walking its contract's fills
//! back from the newest until they add up to what is held ([`entry`]); IB serves the API
//! seven days of fills at most, so an older position has no entry, not a guessed one.

use std::collections::HashMap;

use ibapi::accounts::{AccountPortfolioValue, AccountUpdate};
use ibapi::orders::{ExecutionData, ExecutionFilter, Executions};
use otw_connector_model::{ConnectorError, Decimal, Timestamp};

use super::session::Session;

/// How far back IB's API answers fills.
const FILL_DAYS: i32 = 7;

/// One fill, reduced to what an entry needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    /// IB's execution id.
    pub id: String,
    /// The order IB filled (its permanent id, 0 when IB names none).
    pub order_id: i64,
    pub contract_id: i32,
    /// The contract as IB reported it with the execution.
    pub contract: ibapi::contracts::Contract,
    /// The price this execution traded at.
    pub price: f64,
    pub account: String,
    /// Signed: a sale is negative.
    pub qty: Decimal,
    pub at: Timestamp,
    /// Commission as a positive cost, once IB reported it.
    pub fee: Option<Decimal>,
    pub fee_currency: String,
}

/// When a position opened and what opening it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub at: Timestamp,
    pub fee: Option<Decimal>,
    pub fee_currency: String,
}

/// IB writes an execution time as `yyyymmdd hh:mm:ss` followed by a zone name
/// (`US/Eastern`), or as `yyyymmdd-hh:mm:ss` in UTC. Anything else is no time.
pub fn exec_time(raw: &str) -> Option<Timestamp> {
    use time::macros::format_description;
    let raw = raw.trim();
    if let Some((d, t)) = raw.split_once('-') {
        let date = time::Date::parse(d, format_description!("[year][month][day]")).ok()?;
        let tod = time::Time::parse(t, format_description!("[hour]:[minute]:[second]")).ok()?;
        return Timestamp::from_offset_datetime(time::PrimitiveDateTime::new(date, tod).assume_utc());
    }
    let mut parts = raw.split_whitespace();
    let date = time::Date::parse(parts.next()?, format_description!("[year][month][day]")).ok()?;
    let tod = time::Time::parse(parts.next()?, format_description!("[hour]:[minute]:[second]")).ok()?;
    let local = time::PrimitiveDateTime::new(date, tod);
    match parts.next() {
        None => None,
        Some(zone) => {
            use time_tz::PrimitiveDateTimeExt;
            let tz = time_tz::timezones::get_by_name(zone)?;
            Timestamp::from_offset_datetime(local.assume_timezone(tz).take_first()?)
        }
    }
}

/// The fills that built a position of `held` (signed), newest first in `fills` (one contract,
/// one account): walked back until they add up to it. The entry is the oldest of them. None
/// when the fills on hand do not reach it (the position is older than IB's window).
pub fn entry(fills: &[Fill], held: Decimal) -> Option<Entry> {
    if held.is_zero() {
        return None;
    }
    let mut sum = Decimal::ZERO;
    let mut fee = Some(Decimal::ZERO);
    let mut fee_currency = String::new();
    for f in fills {
        sum += f.qty;
        fee = match (fee, f.fee) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        if fee_currency.is_empty() {
            fee_currency = f.fee_currency.clone();
        }
        // Reached exactly: these fills are the position. Crossing zero means an earlier
        // position was flipped, whose fills say nothing of when this one began.
        if sum == held {
            return Some(Entry { at: f.at, fee, fee_currency });
        }
        if !sum.is_zero() && sum.is_sign_negative() != held.is_sign_negative() {
            return None;
        }
    }
    None
}

/// The portfolio of every account the gateway is signed in to.
pub async fn portfolio(s: &Session) -> Result<Vec<AccountPortfolioValue>, ConnectorError> {
    let accounts = s.ask("accounts", |client| async move { client.managed_accounts().await }).await?;
    let mut out = Vec::new();
    for account in accounts.into_iter().filter(|a| !a.trim().is_empty()) {
        let id = ibapi::accounts::types::AccountId(account.clone());
        let rows = s
            .ask("portfolio", move |client| {
                let id = id.clone();
                async move {
                    let mut sub = client.account_updates(&id).await?;
                    let mut rows = Vec::new();
                    while let Some(item) = sub.next().await {
                        match item? {
                            AccountUpdate::PortfolioValue(mut p) => {
                                if p.account.is_none() {
                                    p.account = Some(id.0.clone());
                                }
                                rows.push(p);
                            }
                            AccountUpdate::End => break,
                            _ => {}
                        }
                    }
                    Ok(rows)
                }
            })
            .await?;
        out.extend(rows);
    }
    Ok(out)
}

/// One account figure as IB's account summary states it (`NetLiquidation`, `AvailableFunds`…).
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub account: String,
    pub tag: String,
    pub value: String,
    pub currency: String,
}

/// What the summary is asked for: the money of a margin account, and the ledger in the base
/// currency for the results.
const SUMMARY_TAGS: &[&str] = &[
    "NetLiquidation",
    "TotalCashValue",
    "InitMarginReq",
    "MaintMarginReq",
    "AvailableFunds",
    "ExcessLiquidity",
    "BuyingPower",
    "GrossPositionValue",
    "$LEDGER",
];

/// The money of every account the gateway is signed in to (`reqAccountSummary`), read to its
/// end once and dropped. Not the account update stream: that one is IB's to give one account
/// at a time, and the portfolio read already uses it.
pub async fn summary(s: &Session) -> Result<Vec<Figure>, ConnectorError> {
    s.ask("account summary", |client| async move {
        let group = ibapi::accounts::types::AccountGroup("All".to_string());
        let mut sub = client.account_summary(&group, SUMMARY_TAGS).await?;
        let mut out = Vec::new();
        while let Some(item) = sub.next().await {
            match item? {
                ibapi::accounts::AccountSummaryResult::Summary(f) => {
                    out.push(Figure { account: f.account, tag: f.tag, value: f.value, currency: f.currency })
                }
                ibapi::accounts::AccountSummaryResult::End => break,
            }
        }
        Ok(out)
    })
    .await
}

/// The fills IB still serves (seven days), newest first, with their commissions.
pub async fn fills(s: &Session) -> Result<Vec<Fill>, ConnectorError> {
    let (execs, fees) = s
        .ask("fills", |client| async move {
            let filter = ExecutionFilter { last_n_days: FILL_DAYS, ..Default::default() };
            let mut sub = client.executions(filter).await?;
            let mut execs = Vec::new();
            let mut fees: HashMap<String, (f64, String)> = HashMap::new();
            while let Some(item) = sub.next().await {
                match item? {
                    Executions::ExecutionData(d) => execs.push(d),
                    Executions::CommissionReport(c) => {
                        fees.insert(c.execution_id.clone(), (c.commission, c.currency.clone()));
                    }
                    Executions::Notice(_) => {}
                }
            }
            Ok((execs, fees))
        })
        .await?;
    Ok(fills_of(execs, &fees))
}

/// IB's executions as fills, newest first, each with the commission IB reported for its
/// execution id, whichever came first. A repeated execution counts once and a correction
/// replaces the fill it corrects ([`super::orders::standing`]).
pub fn fills_of(execs: Vec<ExecutionData>, fees: &HashMap<String, (f64, String)>) -> Vec<Fill> {
    let mut out: Vec<Fill> = super::orders::standing(execs, |d| d.execution.execution_id.as_str())
        .into_iter()
        .filter_map(|d| {
            let at = exec_time(&d.execution.time).or_else(|| {
                tracing::warn!("ibkr: unreadable fill time {:?}", d.execution.time);
                None
            })?;
            let sell = d.execution.side.eq_ignore_ascii_case("SLD") || d.execution.side.eq_ignore_ascii_case("SELL");
            let fee = fees.get(&d.execution.execution_id);
            let shares = super::mapper::dec(d.execution.shares).ok()?;
            Some(Fill {
                id: d.execution.execution_id.clone(),
                order_id: i64::from(d.execution.perm_id),
                contract_id: d.contract.contract_id,
                contract: d.contract.clone(),
                price: d.execution.price,
                account: d.execution.account_number.clone(),
                qty: if sell { -shares } else { shares },
                at,
                // IB sends an unset commission as f64::MAX.
                fee: fee.map(|f| f.0).filter(|c| c.is_finite() && *c < 1e300).and_then(|c| super::mapper::dec(c).ok()),
                fee_currency: fee.map(|f| f.1.clone()).unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.at));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn fill(qty: &str, secs: i64, fee: Option<&str>) -> Fill {
        Fill { id: String::new(), order_id: 0, contract_id: 1, contract: Default::default(), price: 0.0, account: "DU1".into(), qty: d(qty), at: Timestamp::from_secs(secs).unwrap(), fee: fee.map(d), fee_currency: "USD".into() }
    }

    fn exec(id: &str, shares: f64, at: &str) -> ExecutionData {
        ExecutionData {
            contract: ibapi::contracts::Contract { contract_id: 1, ..Default::default() },
            execution: ibapi::orders::Execution { execution_id: id.into(), shares, side: "BOT".into(), time: at.into(), account_number: "DU1".into(), ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn a_fill_counts_once_takes_its_correction_and_its_late_fee() {
        // T24 and T25: a repeat of an execution, a correction of another, and a commission
        // report that came after (keyed by the corrected id).
        let execs = vec![
            exec("0001.aa.01.01", 1.0, "20261021-17:40:12"),
            exec("0001.aa.01.01", 1.0, "20261021-17:40:12"),
            exec("0001.ab.01.01", 3.0, "20261021-17:41:12"),
            exec("0001.ab.01.02", 2.0, "20261021-17:41:12"),
        ];
        let fees: HashMap<String, (f64, String)> = [("0001.ab.01.02".to_string(), (0.62, "USD".to_string()))].into();
        let f = fills_of(execs, &fees);
        assert_eq!(f.iter().map(|x| x.qty).collect::<Vec<_>>(), [d("2"), d("1")]);
        assert_eq!((f[0].fee, f[0].fee_currency.as_str()), (Some(d("0.62")), "USD"));
        assert_eq!(f[1].fee, None, "a commission not reported yet is unknown, not zero");
    }

    #[test]
    fn both_ib_time_spellings_are_read() {
        assert_eq!(exec_time("20261021-17:40:12"), Timestamp::from_secs(1_792_604_412));
        // 13:40:12 in New York in October is 17:40:12 UTC.
        assert_eq!(exec_time("20261021 13:40:12 US/Eastern"), Timestamp::from_secs(1_792_604_412));
        assert_eq!(exec_time("20261021 13:40:12"), None);
        assert_eq!(exec_time("yesterday"), None);
    }

    #[test]
    fn an_entry_is_the_oldest_fill_that_builds_the_position() {
        // Newest first: +1 at 300, +1 at 200 build a long 2; the sale before is another trade.
        let f = [fill("1", 300, Some("0.62")), fill("1", 200, Some("0.62")), fill("-1", 100, Some("0.62"))];
        let e = entry(&f, d("2")).unwrap();
        assert_eq!(e.at, Timestamp::from_secs(200).unwrap());
        assert_eq!(e.fee, Some(d("1.24")));
        // Long 1: the newest buy alone.
        assert_eq!(entry(&f, d("1")).unwrap().at, Timestamp::from_secs(300).unwrap());
        // More held than the fills on hand: older than the window, no entry.
        assert_eq!(entry(&f[..1], d("2")), None);
        // A commission IB never reported makes the fee unknown, not zero.
        assert_eq!(entry(&[fill("1", 300, None)], d("1")).unwrap().fee, None);
        // A short.
        assert_eq!(entry(&[fill("-2", 50, Some("1"))], d("-2")).unwrap().at, Timestamp::from_secs(50).unwrap());
    }
}
