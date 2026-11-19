//! Broker account assets: an account's equity, read with its own credentials.
//!
//! The figure is the account's net liquidation value as the broker states it
//! (`Broker::account`, the same summary the broker pages read), not a sum of holdings priced
//! here: a holdings list carries no prices, and pricing it would be a second opinion on a
//! number the broker already gives.

use anyhow::{bail, Result};

use crate::brokers::AccountSummary;

/// The module id an account must be granted to.
pub const MODULE: &str = "wealth";

/// The equity of every account the connection reaches, with its currency. A summary without
/// an equity is an error, never a zero: the account exists and holds something unknown.
pub fn equity_of(broker: &str, summaries: &[AccountSummary]) -> Result<Vec<(f64, String)>> {
    if summaries.is_empty() {
        bail!("{broker} returned no account");
    }
    let mut out = Vec::with_capacity(summaries.len());
    for s in summaries {
        let Some(e) = s.equity.filter(|e| e.is_finite()) else {
            bail!("{broker} does not state this account's equity");
        };
        out.push((e, money_of(&s.currency)));
    }
    Ok(out)
}

/// A crypto venue states its money in a dollar stablecoin. It is read as dollars, the same
/// convention the Portfolio Tracker's exchange quotes use, so it can be converted at all.
pub fn money_of(currency: &str) -> String {
    match currency.trim().to_ascii_uppercase().as_str() {
        "USDT" | "USDC" | "BUSD" | "FDUSD" | "TUSD" => "USD".to_string(),
        "" => "USD".to_string(),
        c => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(currency: &str, equity: Option<f64>) -> AccountSummary {
        AccountSummary { currency: currency.into(), equity, ..Default::default() }
    }

    #[test]
    fn equity_is_read_with_its_currency() {
        let v = equity_of("Paper", &[summary("EUR", Some(12_500.0))]).unwrap();
        assert_eq!(v, vec![(12_500.0, "EUR".to_string())]);
        // Two sub-accounts are both kept, each in its own money.
        let v = equity_of("IBKR", &[summary("USD", Some(1.0)), summary("CHF", Some(2.0))]).unwrap();
        assert_eq!(v.len(), 2);
        // A stablecoin is read as dollars.
        assert_eq!(equity_of("Binance", &[summary("USDT", Some(5.0))]).unwrap()[0].1, "USD");
    }

    #[test]
    fn an_unstated_equity_is_an_error_not_a_zero() {
        let e = equity_of("Binance", &[summary("USDT", None)]).unwrap_err();
        assert!(e.to_string().contains("does not state"));
        assert!(equity_of("X", &[]).is_err());
    }
}
