//! Asking Interactive Brokers which instrument a ticker names, before any bar is requested.
//!
//! Every other provider takes a symbol and answers. IB takes a **contract**, and a contract
//! it cannot pin down is either refused or, worse, silently served from another listing: the
//! same futures root trades on several exchanges, in several currencies, with a different
//! multiplier on each. So a partial contract is resolved through `reqContractDetails` first,
//! and the download only starts once exactly one instrument answers.
//!
//! Two things follow from doing it here rather than letting the first chunk fail:
//!
//! - **A wrong ticker is named at once**, with the candidates written the way the form
//!   accepts them, instead of a job that queues, waits its turn and dies on error 200.
//! - **IB's own spelling is accepted.** `MNQU6` is what TWS shows and what a user copies;
//!   it goes out as the contract's `local_symbol` and comes back fully qualified.
//!
//! The lookup is cached per gateway address for the life of the process. A contract
//! definition does not change, and the download worker, the chart preview and the symbol
//! search would otherwise each pay for the same question.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use ibapi::contracts::Contract;
use tokio::sync::{Mutex, OnceCell};

use super::contract;
use super::session::Session;

/// Resolved contracts by `address|asset_type|ticker`.
static CACHE: OnceCell<Mutex<HashMap<String, Contract>>> = OnceCell::const_new();

async fn cache() -> &'static Mutex<HashMap<String, Contract>> {
    CACHE.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

/// How many candidates an ambiguity error is willing to print before it stops helping.
const MAX_LISTED: usize = 8;

/// The contract to trade this ticker against, asking the gateway when the ticker alone does
/// not pin one down.
pub async fn contract_for(
    session: &Session,
    ticker: &str,
    asset_type: &str,
) -> Result<Contract> {
    let key = format!("{}|{asset_type}|{}", session.addr(), ticker.trim().to_uppercase());
    if let Some(hit) = cache().await.lock().await.get(&key) {
        return Ok(hit.clone());
    }
    let partial = contract::build(ticker, asset_type)?;
    let resolved = if contract::needs_resolution(&partial) {
        lookup(session, ticker, partial).await?
    } else {
        partial
    };
    cache().await.lock().await.insert(key, resolved.clone());
    Ok(resolved)
}

/// One `reqContractDetails` round trip, reduced to a single instrument or an error that
/// names what to choose between.
async fn lookup(session: &Session, ticker: &str, partial: Contract) -> Result<Contract> {
    let what = format!("{ticker} contract lookup");
    let details = session
        .call(&what, move |client| {
            let partial = partial.clone();
            async move { client.contract_details(&partial).await }
        })
        .await?;

    // IB answers per listing, and a listing appears once per trading class, so the same
    // instrument can come back several times. Identity is the contract id.
    let mut found: Vec<Contract> = details.into_iter().map(|d| d.contract).collect();
    found.sort_by(|a, b| {
        (&a.exchange.0, &a.last_trade_date_or_contract_month, a.contract_id).cmp(&(
            &b.exchange.0,
            &b.last_trade_date_or_contract_month,
            b.contract_id,
        ))
    });
    found.dedup_by_key(|c| c.contract_id);

    match found.len() {
        0 => Err(anyhow!(
            "Interactive Brokers has no contract matching {ticker:?}. Check the spelling, and \
             for a future write the month (ES.202512) or paste the local symbol TWS shows \
             (ESZ5)."
        )),
        1 => {
            let mut c = found.remove(0);
            // The resolved contract answers by id from here on, which is the one field no
            // other listing shares. `include_expired` is ours, not IB's: history of a
            // contract past its last trading day is refused unless the request says so.
            c.include_expired = c.security_type == ibapi::contracts::SecurityType::Future;
            Ok(c)
        }
        n => {
            let listed: Vec<String> = found.iter().take(MAX_LISTED).map(contract::label).collect();
            let more = n.saturating_sub(listed.len());
            let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
            Err(anyhow!(
                "{ticker:?} names {n} Interactive Brokers contracts. Ask for one of them: {}{tail}",
                listed.join(", ")
            ))
        }
    }
}
