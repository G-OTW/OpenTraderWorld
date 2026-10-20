//! Asking Interactive Brokers which instrument a ticker names, before any bar is requested.
//!
//! A contract IB cannot pin down is either refused or, worse, served from another listing:
//! the same futures root trades on several exchanges, in several currencies, with a different
//! multiplier on each. So a partial contract goes through `reqContractDetails` first, and
//! only exactly one answer is used. A wrong ticker is named at once, with the candidates
//! written the way the form accepts them; IB's own spelling (`MNQU6`) is accepted.
//!
//! Cached per gateway address for the life of the process: a contract definition does not
//! change, and every reader would otherwise pay for the same question.

use std::collections::HashMap;

use ibapi::contracts::Contract;
use otw_connector_model::ConnectorError;
use tokio::sync::{Mutex, OnceCell};

use super::contract;
use super::session::Session;

/// Resolved contracts by `address|asset_type|ticker`.
static CACHE: OnceCell<Mutex<HashMap<String, Contract>>> = OnceCell::const_new();

async fn cache() -> &'static Mutex<HashMap<String, Contract>> {
    CACHE.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

/// How many candidates an ambiguity error prints before it stops helping.
const MAX_LISTED: usize = 8;

fn key(session: &Session, ticker: &str, asset_type: &str) -> String {
    format!("{}|{asset_type}|{}", session.addr(), ticker.trim().to_uppercase())
}

/// The contract this ticker names, asking the gateway when the ticker alone does not pin one.
pub async fn contract_for(session: &Session, ticker: &str, asset_type: &str) -> Result<Contract, ConnectorError> {
    let k = key(session, ticker, asset_type);
    if let Some(hit) = cache().await.lock().await.get(&k) {
        return Ok(hit.clone());
    }
    let partial = contract::build(ticker, asset_type)?;
    let resolved = if contract::needs_resolution(&partial) { lookup(session, ticker, partial).await? } else { partial };
    cache().await.lock().await.insert(k, resolved.clone());
    Ok(resolved)
}

/// Record a contract a listing already resolved, so its download does not ask again.
pub async fn remember(session: &Session, ticker: &str, asset_type: &str, c: Contract) {
    cache().await.lock().await.insert(key(session, ticker, asset_type), c);
}

/// One `reqContractDetails` round trip, reduced to a single instrument or an error naming
/// what to choose between.
async fn lookup(session: &Session, ticker: &str, partial: Contract) -> Result<Contract, ConnectorError> {
    let what = format!("{ticker} contract lookup");
    let details = session
        .call(&what, move |client| {
            let partial = partial.clone();
            async move { client.contract_details(&partial).await }
        })
        .await?;
    // A listing appears once per trading class; identity is the contract id.
    let mut found: Vec<Contract> = details.into_iter().map(|d| d.contract).collect();
    found.sort_by(|a, b| (&a.exchange.0, &a.last_trade_date_or_contract_month, a.contract_id).cmp(&(&b.exchange.0, &b.last_trade_date_or_contract_month, b.contract_id)));
    found.dedup_by_key(|c| c.contract_id);
    match found.len() {
        0 => Err(ConnectorError::not_found(format!(
            "Interactive Brokers has no contract matching {ticker:?}. Check the spelling, and for a future write the month (ES.202512) or paste the local symbol TWS shows (ESZ5)."
        ))),
        1 => {
            let mut c = found.remove(0);
            // History of a contract past its last trading day is refused unless asked for.
            c.include_expired = c.security_type == ibapi::contracts::SecurityType::Future;
            Ok(c)
        }
        n => {
            let listed: Vec<String> = found.iter().take(MAX_LISTED).map(contract::label).collect();
            let more = n.saturating_sub(listed.len());
            let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
            Err(ConnectorError::invalid(format!("{ticker:?} names {n} Interactive Brokers contracts. Ask for one of them: {}{tail}", listed.join(", "))))
        }
    }
}
