//! Book a chart instrument's trades into the journal picked on its order ticket.
//!
//! The ticket links an instrument to a journal for a period (`otw_store::journal_chart`).
//! Each reconciliation cycle of a trading account (`brokers::reconcile`) reads the account's
//! fills on every linked instrument and folds them into positions exactly as a broker sync
//! does ([`fold_execs`]): a position lands open at its entry and is refreshed in place at each
//! exit, whoever sent the exit (the ticket, the pane's buttons, a bracket, the broker itself).
//!
//! - **A position belongs to the period it opened in.** Picking another journal ends the
//!   period: a position open then finishes in the old journal, the next one goes to the new.
//! - **What the account held before the link is never booked.** It is seeded (what is held
//!   less the fills since the link), so its closes are not read as new positions.
//! - **Same identity as a broker sync** (the account and the opening fill): a journal also
//!   synced from the broker files the position once.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use otw_store::brokers::BrokerRow;
use otw_store::journal_chart::{self as store, Book};
use otw_store::{journal_fx, journal_import as import_store};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use super::broker::{fold_execs, position_hash, still_open, Rules, Seed};
use crate::brokers::{Broker, ExecQuery, Execution};
use crate::brokers_api::ChartSymbol;

/// The broker module the chart trades under.
const CHART_MODULE: &str = "histviz";

/// Book the account's fills on every linked instrument. A failure is kept on the instrument's
/// book and logged; the next cycle tries again.
pub async fn book_account(pool: &PgPool, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, http: &reqwest::Client) {
    if !account.allows(CHART_MODULE) || !connector.chart_fills() {
        return;
    }
    if let Err(e) = follow_links(pool, account, connector, settings, http).await {
        tracing::warn!("chart journal: following the links on “{}”: {e:#}", account.name);
    }
    let books = match store::books(pool, account.id).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("chart journal: “{}”: {e:#}", account.name);
            return;
        }
    };
    for mut book in books {
        let err = book_one(pool, account, connector, settings, http, &mut book).await.err().map(|e| format!("{e:#}"));
        if let Some(e) = &err {
            tracing::warn!("chart journal: booking {} on “{}”: {e}", book.ticker, account.name);
        }
        if let Err(e) = store::save_book(pool, &book, err.as_deref()).await {
            tracing::warn!("chart journal: “{}”: {e:#}", account.name);
        }
    }
}

/// Start following, on this account, the instruments linked since it last looked.
async fn follow_links(pool: &PgPool, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, http: &reqwest::Client) -> Result<()> {
    let cap = connector.capability();
    let mut held = None;
    for (ticker, asset_type, since) in store::open_links(pool).await? {
        if store::has_book(pool, account.id, &ticker, &asset_type).await? {
            continue;
        }
        let symbol = ChartSymbol::of(connector, settings, &ticker, &asset_type).await;
        let mut seeds = Vec::new();
        // A cash account's positions are balances, not trades: a sell of a holding bought
        // before is an orphan the fold leaves out on its own.
        if cap.positions && !cap.spot_only {
            if held.is_none() {
                held = Some(connector.positions(http, settings).await?);
            }
            let mine: Vec<_> = held.iter().flatten().filter(|p| symbol.matches(&p.symbol)).collect();
            let now_qty: f64 = mine.iter().map(|p| if p.side == "short" { -p.qty.abs() } else { p.qty.abs() }).sum();
            if let (Some(p), true) = (mine.first(), now_qty.abs() > 1e-12) {
                let q = ExecQuery { from: since, to: OffsetDateTime::now_utc(), symbols: vec![symbol.symbol.clone()] };
                let net: f64 = matching(connector, settings, http, &q, &symbol).await?.iter().map(signed).sum();
                let before = now_qty - net;
                if before.abs() > 1e-9 {
                    seeds.push(Seed {
                        id: Uuid::new_v4(),
                        symbol: p.symbol.clone(),
                        side: if before < 0.0 { "short".into() } else { "long".into() },
                        qty: before.abs(),
                        price: p.avg_price.or(p.market_price).unwrap_or(0.0),
                        entry_at: since - time::Duration::milliseconds(1),
                        cutoff: since,
                        currency: p.currency.clone(),
                        asset_class: p.asset_class.clone(),
                        multiplier: p.multiplier,
                    });
                }
            }
        }
        store::add_book(pool, account.id, &ticker, &asset_type, since, &serde_json::to_value(&seeds)?).await?;
    }
    Ok(())
}

/// Fold the instrument's fills since the oldest position still open and write each position
/// into the journal of the period it opened in. `book` is changed only once it all went in.
async fn book_one(pool: &PgPool, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, http: &reqwest::Client, book: &mut Book) -> Result<()> {
    let links = store::links(pool, &book.ticker, &book.asset_type).await?;
    let Some(first) = links.first() else { return Ok(()) };
    let ended = links.iter().map(|l| l.until).collect::<Option<Vec<_>>>().and_then(|u| u.into_iter().max());
    // No period open and flat since the last one ended: nothing left to book.
    if ended.is_some_and(|u| book.resume_from >= u) {
        return Ok(());
    }
    let symbol = ChartSymbol::of(connector, settings, &book.ticker, &book.asset_type).await;
    crate::brokers::check_window(connector.capability(), book.resume_from)?;
    let q = ExecQuery { from: book.resume_from, to: OffsetDateTime::now_utc(), symbols: vec![symbol.symbol.clone()] };
    let mut execs = matching(connector, settings, http, &q, &symbol).await?;
    let mark = mark_of(&execs);
    if mark == book.mark {
        return Ok(());
    }
    let mut seeds: Vec<Seed> = serde_json::from_value(book.seeds.clone()).unwrap_or_default();
    // One spelling for the fold: a seed read off a position and the fills must group together.
    if let Some(canon) = execs.last().map(|e| e.symbol.clone()) {
        execs.iter_mut().for_each(|e| e.symbol.clone_from(&canon));
        seeds.iter_mut().for_each(|s| s.symbol.clone_from(&canon));
    }
    let last_at = execs.iter().map(|e| e.at).max();
    let rules = Rules { seeds: seeds.clone(), ..Default::default() };
    let folded = fold_execs(pool, execs, &q, first.category_id, None, None, connector.capability().spot_only, &rules).await;

    let mut batches: HashMap<Uuid, Uuid> = serde_json::from_value(book.batches.clone()).unwrap_or_default();
    let mut touched = HashSet::new();
    let mut open_since: Option<OffsetDateTime> = None;
    for mut t in folded.trades {
        let opener = t.source_rows.first().and_then(|i| folded.ids.get(*i)).cloned().unwrap_or_else(|| t.row_hash.clone());
        let seed = opener.strip_prefix("seed:");
        if still_open(&t.input) {
            let since = match seed {
                Some(id) => seeds.iter().find(|s| s.id.to_string() == id).map(|s| s.cutoff),
                None => t.input.entry_at,
            };
            open_since = [open_since, since].into_iter().flatten().min();
        }
        if seed.is_some() {
            continue;
        }
        let Some(link) = t.input.entry_at.and_then(|at| links.iter().find(|l| l.covers(at))) else { continue };
        t.input.category_id = link.category_id;
        let batch = batch_for(pool, account, &book.ticker, link.category_id, &mut batches).await?;
        import_store::sync_imported_trade(pool, &t.input, batch, &position_hash(account.id, &opener)).await?;
        touched.insert(batch);
    }
    for b in touched {
        store::count_batch(pool, b).await?;
    }
    for (date, quote) in &folded.fx_misses {
        journal_fx::mark_pending(pool, *date, quote, "needed to convert a broker fee").await.ok();
    }

    // The next read starts at the oldest position still open, or just past the last fill.
    book.resume_from = open_since.or(last_at.map(|a| a + time::Duration::microseconds(1))).unwrap_or(book.resume_from);
    seeds.retain(|s| s.cutoff >= book.resume_from);
    book.seeds = serde_json::to_value(&seeds)?;
    book.batches = serde_json::to_value(&batches)?;
    book.mark = if book.resume_from == q.from { mark } else { String::new() };
    Ok(())
}

/// The account's fills on the instrument over the query's window.
async fn matching(connector: &dyn Broker, settings: &HashMap<String, String>, http: &reqwest::Client, q: &ExecQuery, symbol: &ChartSymbol) -> Result<Vec<Execution>> {
    Ok(connector.executions(http, settings, q).await?.into_iter().filter(|e| symbol.matches(&e.symbol)).collect())
}

/// The batch the journal's chart trades from this account go to, made again if the user
/// reverted or forgot it.
async fn batch_for(pool: &PgPool, account: &BrokerRow, ticker: &str, category_id: Uuid, batches: &mut HashMap<Uuid, Uuid>) -> Result<Uuid> {
    if let Some(id) = batches.get(&category_id) {
        if store::batch_exists(pool, *id).await? {
            return Ok(*id);
        }
    }
    let id = import_store::add_broker_batch(pool, account.id, &format!("{} · {ticker}", account.name), category_id, 0).await?;
    batches.insert(category_id, id);
    Ok(id)
}

/// A buy adds, a sell takes away.
fn signed(e: &Execution) -> f64 {
    if e.side.eq_ignore_ascii_case("sell") {
        -e.qty.abs()
    } else {
        e.qty.abs()
    }
}

/// What a window of fills was, to tell an unchanged one.
fn mark_of(execs: &[Execution]) -> String {
    let mut h = Sha256::new();
    for e in execs {
        h.update(format!("{}|{}|{}\n", e.id, e.qty, e.price).as_bytes());
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::brokers::{BrokerCapability, Position};

    /// An account whose fills and positions the test writes.
    struct Fake {
        execs: Mutex<Vec<Execution>>,
        held: Mutex<Vec<Position>>,
    }

    #[async_trait::async_trait]
    impl Broker for Fake {
        fn capability(&self) -> &'static BrokerCapability {
            &crate::brokers::paper::PAPER
        }
        async fn executions(&self, _: &reqwest::Client, _: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
            Ok(self.execs.lock().unwrap().iter().filter(|e| e.at >= q.from).cloned().collect())
        }
        async fn positions(&self, _: &reqwest::Client, _: &HashMap<String, String>) -> Result<Vec<Position>> {
            Ok(self.held.lock().unwrap().clone())
        }
    }

    fn fill(id: &str, symbol: &str, side: &str, qty: f64, price: f64, at: OffsetDateTime) -> Execution {
        Execution {
            id: id.into(),
            order_id: String::new(),
            at,
            symbol: symbol.into(),
            side: side.into(),
            qty,
            price,
            fee: 0.0,
            fee_currency: "USD".into(),
            currency: "USD".into(),
            asset_class: "stock".into(),
            multiplier: 1.0,
            venue: String::new(),
            account: String::new(),
        }
    }

    fn long(symbol: &str, qty: f64, price: f64) -> Position {
        Position {
            symbol: symbol.into(),
            side: "long".into(),
            qty,
            avg_price: Some(price),
            currency: "USD".into(),
            asset_class: "stock".into(),
            multiplier: 1.0,
            venue: String::new(),
            account: String::new(),
            native_id: None,
            market_price: None,
            unrealized_pnl: None,
            opened_at: None,
            entry_fee: None,
            fee_currency: String::new(),
            leverage: None,
            margin_mode: None,
        }
    }

    /// (journal, entry legs, exit legs, still open) per booked trade of the ticker, by entry.
    async fn booked(pool: &PgPool, ticker: &str) -> Vec<(Uuid, i32, i32, bool)> {
        sqlx::query_as(
            "SELECT category_id, jsonb_array_length(entries), jsonb_array_length(exits), exit_at IS NULL \
             FROM journal_trades WHERE ticker = $1 ORDER BY entry_at",
        )
        .bind(ticker)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    /// The held position is left out, the position opened under the first journal finishes
    /// there after the switch, whoever closed it, and the next one goes to the second.
    #[tokio::test]
    async fn each_position_lands_in_the_journal_it_opened_under() {
        let Ok(url) = std::env::var("OTW_TEST_DATABASE_URL") else { return };
        let pool = otw_store::connect_and_migrate(&url).await.unwrap();
        let run = Uuid::new_v4().simple().to_string();
        let ticker = format!("T{}", run[..8].to_uppercase());
        let j1 = otw_store::journal::add_category(&pool, &format!("chart a {run}"), None).await.unwrap().id;
        let j2 = otw_store::journal::add_category(&pool, &format!("chart b {run}"), None).await.unwrap().id;
        let account = otw_store::brokers::create(&pool, "paper", &format!("chart {run}"), &["*".into(), "trading".into()], &serde_json::json!({}))
            .await
            .unwrap();
        let http = reqwest::Client::new();
        let none = HashMap::new();
        let ms = time::Duration::milliseconds;

        // 2 held before the link; after it, those 2 are sold and 1 is bought.
        assert!(store::set(&pool, &ticker, "stock", Some(j1)).await.unwrap());
        let since = store::links(&pool, &ticker, "stock").await.unwrap()[0].since;
        let fake = Fake {
            execs: Mutex::new(vec![fill("1", &ticker, "sell", 2.0, 100.0, since + ms(1)), fill("2", &ticker, "buy", 1.0, 110.0, since + ms(2))]),
            held: Mutex::new(vec![long(&ticker, 1.0, 110.0)]),
        };
        book_account(&pool, &account, &fake, &none, &http).await;
        assert_eq!(booked(&pool, &ticker).await, vec![(j1, 1, 0, true)], "the held position is not booked, nor its sale as a short");

        // Switch journals; the open position is closed (from anywhere) and a new one traded.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(store::set(&pool, &ticker, "stock", Some(j2)).await.unwrap());
        let switched = store::links(&pool, &ticker, "stock").await.unwrap()[1].since;
        fake.execs.lock().unwrap().extend([
            fill("3", &ticker, "sell", 1.0, 120.0, switched + ms(1)),
            fill("4", &ticker, "buy", 3.0, 121.0, switched + ms(2)),
            fill("5", &ticker, "sell", 3.0, 125.0, switched + ms(3)),
        ]);
        fake.held.lock().unwrap().clear();
        book_account(&pool, &account, &fake, &none, &http).await;
        assert_eq!(booked(&pool, &ticker).await, vec![(j1, 1, 1, false), (j2, 1, 1, false)]);

        // Unchanged fills write nothing new; no journal: the next position is not booked.
        book_account(&pool, &account, &fake, &none, &http).await;
        assert!(store::set(&pool, &ticker, "stock", None).await.unwrap());
        let off = OffsetDateTime::now_utc() + ms(5);
        fake.execs.lock().unwrap().extend([fill("6", &ticker, "buy", 1.0, 130.0, off), fill("7", &ticker, "sell", 1.0, 131.0, off + ms(1))]);
        book_account(&pool, &account, &fake, &none, &http).await;
        assert_eq!(booked(&pool, &ticker).await.len(), 2);
    }
}
