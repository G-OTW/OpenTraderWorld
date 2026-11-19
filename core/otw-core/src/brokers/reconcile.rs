//! What OpenTraderWorld sent, against what the broker says (onboarding spec 6 and 7.2).
//!
//! - **Stored before sent.** Each leg of a ticket is written `pending` under its client id
//!   before the call leaves ([`claim`]); an id the account was already sent is refused, across
//!   restarts. The answer stores the broker's id ([`record`]).
//! - **Unknown is not rejected.** A leg whose answer was lost stays `unknown` and blocks new
//!   orders on its instrument until a lookup by client id settles it: found, the broker's
//!   state is adopted; "never seen" twice after the ack window, it was not received, and
//!   waits for the user to send it again ([`resend_ticket`]) or dismiss it; still
//!   unreachable past [`DEADLINE`], the user is told once.
//! - **The broker wins.** On start and on a cycle ([`spawn`]), every leg still followed is
//!   looked up and takes the broker's state; working orders OTW did not send are noted as
//!   external. The account's positions are not watched: the user trades freely outside
//!   OpenTraderWorld, and a robot checks its own instruments against its own orders. Every
//!   difference is written to the audit log (`reconcile`), and unsettled legs are notified.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use otw_connector_model::{Decimal, OrderStatus};
use otw_store::broker_orders::{self as legs, Leg, NewLeg};
use otw_store::brokers::BrokerRow;
use serde_json::{json, Value};
use time::OffsetDateTime;
use tokio::sync::Notify;
use uuid::Uuid;

use super::{Broker, Lookup, OrderTicket, OrderUnknown, Placed, StoredLeg};
use crate::AppState;

/// After start, before the first cycle: the gateway sessions and the live hub come up first.
const WARMUP: Duration = Duration::from_secs(20);
/// How often the cycle looks at which accounts are due.
const TICK: Duration = Duration::from_secs(15);
/// An account with orders out is reconciled this often; one with none, every [`IDLE`].
const BUSY: Duration = Duration::from_secs(60);
const IDLE: Duration = Duration::from_secs(600);
/// A "never seen" answer counts only once the broker had time to register the order.
const ACK_WINDOW: time::Duration = time::Duration::seconds(10);
/// An unknown leg still unsettled after this is notified.
const DEADLINE: time::Duration = time::Duration::minutes(5);
/// A leg written `pending` this long ago without an answer belongs to a call that died.
const PENDING_STALE: time::Duration = time::Duration::minutes(2);
/// The notification producer: orders are worked from the chart.
const NOTIFY_MODULE: &str = "histviz";
/// The halt an account-level position drift left until it was acknowledged. No longer raised:
/// a halt still open from before can be acknowledged.
pub const DRIFT: &str = "position_drift";

static POKE: Notify = Notify::const_new();
static POKED: LazyLock<Mutex<HashSet<Uuid>>> = LazyLock::new(Default::default);

/// Reconcile this account on the next tick: its broker said events were missed.
pub fn poke(account: Uuid) {
    POKED.lock().unwrap_or_else(|e| e.into_inner()).insert(account);
    POKE.notify_one();
}

/// Where a broker's status leaves a leg.
pub fn word(st: OrderStatus) -> &'static str {
    match st {
        OrderStatus::Filled => "filled",
        OrderStatus::Cancelled => "cancelled",
        OrderStatus::Rejected => "rejected",
        OrderStatus::Expired => "expired",
        OrderStatus::PendingNew | OrderStatus::New | OrderStatus::PartiallyFilled | OrderStatus::PendingCancel | OrderStatus::PendingReplace => "working",
    }
}

/// Before a ticket leaves: refuse it while an order on its instrument has no known outcome,
/// then store its legs `pending`. A client id the account was already sent is refused.
pub async fn claim(state: &AppState, account: &BrokerRow, ticket: &OrderTicket) -> Result<()> {
    if ticket.legs.iter().any(|l| l.key.trim().is_empty()) {
        return Err(anyhow!("an order leg has no key: each order needs the client id it is sent under"));
    }
    let waiting = legs::unknown_on(&state.pool, account.id, &ticket.ticker).await?;
    if !waiting.is_empty() {
        poke(account.id);
        return Err(anyhow!(
            "order {} on {} got no answer from the broker and is being looked up: new orders on {} wait until it is settled. Check it at the broker if this lasts.",
            waiting.join(", "),
            ticket.ticker,
            ticket.ticker
        ));
    }
    let requests: Vec<Value> = ticket.legs.iter().map(|l| serde_json::to_value(l).unwrap_or(Value::Null)).collect();
    let rows: Vec<NewLeg> = ticket
        .legs
        .iter()
        .zip(&requests)
        .map(|(l, request)| NewLeg {
            client_id: &l.key,
            ticker: &ticket.ticker,
            asset_type: &ticket.asset_type,
            action: "place",
            parent: l.parent.as_deref().filter(|p| !p.is_empty()),
            group: l.group.as_deref().filter(|g| !g.is_empty()),
            request,
        })
        .collect();
    duplicate(legs::claim(&state.pool, account.id, &account.broker, &rows).await?)
}

/// The ticket that sends legs the broker never received again: their requests under new
/// client ids (`<old>r`), entries first, each exit still on its entry. One instrument at a
/// time, and an exit never goes without its entry.
pub fn resend_ticket(legs: &[legs::Unsent]) -> Result<super::OrderTicket> {
    let first = legs.first().ok_or_else(|| anyhow!("no order to send again"))?;
    if legs.iter().any(|l| !l.ticker.eq_ignore_ascii_case(&first.ticker)) {
        return Err(anyhow!("orders on different instruments are sent again one instrument at a time"));
    }
    let ids: std::collections::HashSet<&str> = legs.iter().map(|l| l.client_id.as_str()).collect();
    let renamed = |k: &str| format!("{k}r");
    let mut out = Vec::with_capacity(legs.len());
    for l in legs {
        let mut leg: super::OrderLeg = serde_json::from_value(l.request.clone()).map_err(|e| anyhow!("order {} cannot be read back: {e}", l.client_id))?;
        if let Some(p) = leg.parent.as_deref() {
            if !ids.contains(p) {
                return Err(anyhow!("order {} is an exit of {p}: send them again together", l.client_id));
            }
            leg.parent = Some(renamed(p));
        }
        leg.key = renamed(&leg.key);
        leg.group = leg.group.as_deref().map(renamed);
        out.push(leg);
    }
    out.sort_by_key(|l| l.parent.is_some());
    Ok(super::OrderTicket {
        ticker: first.ticker.clone(),
        asset_type: first.asset_type.clone(),
        legs: out,
        provider: None,
        connector: None,
        reference: None,
        reference_at: None,
        // The user chose to send it again: the first try is not a duplicate of it.
        confirm_duplicate: true,
    })
}

/// [`claim`] for a close: one leg, on the position's id.
pub async fn claim_close(state: &AppState, account: &BrokerRow, native_id: &str, key: &str) -> Result<()> {
    let waiting = legs::unknown_on(&state.pool, account.id, native_id).await?;
    if !waiting.is_empty() {
        poke(account.id);
        return Err(anyhow!(
            "order {} on {native_id} got no answer from the broker and is being looked up: the close waits until it is settled. Check it at the broker if this lasts.",
            waiting.join(", ")
        ));
    }
    let request = json!({ "native_id": native_id, "key": key });
    let row = NewLeg { client_id: key, ticker: native_id, asset_type: "", action: "close", parent: None, group: None, request: &request };
    duplicate(legs::claim(&state.pool, account.id, &account.broker, std::slice::from_ref(&row)).await?)
}

/// After [`claim`] or [`claim_close`], before the call leaves: the venue client ids a broker
/// mints for `keys` (Bitfinex), stored with the legs so a lost answer can still be looked up
/// after a restart. Nothing for a broker that spells its ids from the keys. A mint or a store
/// that fails leaves the legs refused: nothing was sent.
pub async fn mint(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, ticker: &str, keys: &[String]) -> Result<()> {
    let refuse = |e: anyhow::Error| async move {
        let reason = format!("{e:#}");
        for k in keys {
            let _ = legs::set_status(&state.pool, account.id, k, "rejected", Some(&reason), None, None).await;
        }
        Err(e)
    };
    let minted = match connector.mint_client_ids(settings, ticker, keys).await {
        Ok(m) => m,
        Err(e) => return refuse(e).await,
    };
    for m in &minted {
        if let Err(e) = legs::set_minted(&state.pool, account.id, &m.key, &m.id, m.at_ms).await {
            return refuse(e.context("the minted client id could not be stored, so nothing was sent")).await;
        }
    }
    Ok(())
}

fn duplicate(seen: Option<String>) -> Result<()> {
    match seen {
        None => Ok(()),
        Some(id) => Err(anyhow!("client order id {id} was already sent from this account: a new order needs a new id, so nothing is placed twice")),
    }
}

/// Store the broker's answer to a ticket whose legs [`claim`] wrote. Accepted legs work under
/// the broker's id; an [`OrderUnknown`] leaves its legs unknown for the cycle to settle; any
/// other refusal rejects them, then looks them up once anyway, since a ticket refused half way
/// may have left a leg out (a spot market entry filled whose exits were refused).
pub async fn record(state: &AppState, account: &BrokerRow, keys: &[String], out: &Result<Vec<Placed>>) {
    let pool = &state.pool;
    let result = async {
        match out {
            Ok(placed) => {
                for p in placed {
                    legs::accepted(pool, account.id, &p.key, &p.id).await?;
                }
                // A leg the broker did not name back is looked up rather than assumed.
                for k in keys.iter().filter(|k| !placed.iter().any(|p| &p.key == *k)) {
                    legs::set_status(pool, account.id, k, "unknown", Some("not named in the broker's answer"), None, None).await?;
                }
            }
            Err(e) => {
                let unknown = e.downcast_ref::<OrderUnknown>().map(|u| u.keys.clone()).unwrap_or_default();
                let reason = format!("{e:#}");
                for k in keys {
                    let st = if unknown.contains(k) { "unknown" } else { "rejected" };
                    legs::set_status(pool, account.id, k, st, Some(&reason), None, None).await?;
                }
                let refused: Vec<String> = keys.iter().filter(|k| !unknown.contains(k)).cloned().collect();
                if !refused.is_empty() {
                    let (state, account) = (state.clone(), account.clone());
                    tokio::spawn(async move { verify(&state, &account, &refused).await });
                }
            }
        }
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = result {
        tracing::error!("broker orders: could not store the answer for “{}”: {e:#}", account.name);
    }
    if let Err(e) = out {
        if let Some(u) = e.downcast_ref::<OrderUnknown>() {
            if u.alert {
                notify(
                    state,
                    format!("Unmapped broker answer on {}", account.name),
                    format!("{}\n\nThe order is looked up by its client id and takes the broker's state. Report the code so it gets mapped.", u.message),
                );
            }
            poke(account.id);
        }
    }
}

/// Look refused legs up once: one the broker has after all takes its state, and says so.
async fn verify(state: &AppState, account: &BrokerRow, keys: &[String]) {
    tokio::time::sleep(Duration::from_secs(2)).await;
    let Ok((connector, settings)) = open_account(state, account).await else { return };
    for k in keys {
        let Ok(Some(leg)) = legs::get(&state.pool, account.id, k).await else { continue };
        if let Ok(Lookup::Found { id, status, filled_qty, .. }) = connector.lookup(&settings, &leg.ticker, &leg.asset_type, k).await {
            adopt(state, account, &leg, &id, status, filled_qty, "refused_leg_found").await;
        }
    }
}

async fn open_account(state: &AppState, account: &BrokerRow) -> Result<(Box<dyn Broker>, HashMap<String, String>)> {
    let connector = super::for_account(&state.pool, account)?;
    let settings = otw_store::brokers::load_creds(&state.pool, &state.cipher, account.id).await?;
    Ok((connector, settings))
}

/// One reconciliation finding, written to the audit log.
async fn log(state: &AppState, account: &BrokerRow, kind: &str, subject: Value, detail: Value) {
    let request = json!({ "kind": kind, "subject": subject });
    let entry = otw_store::order_log::NewEntry {
        account_id: account.id,
        account: &account.name,
        broker: &account.broker,
        action: "reconcile",
        request: &request,
        ok: true,
        response: &detail,
        latency_ms: 0,
        source: None,
    };
    if let Err(e) = otw_store::order_log::append(&state.pool, &entry).await {
        tracing::error!("broker order log: could not record a reconciliation on “{}”: {e:#}", account.name);
    }
}

/// Tell the user an account halted at its loss limit.
pub fn notify_halt(state: &AppState, account: &BrokerRow, detail: &str) {
    notify(
        state,
        format!("Trading halted on {}", account.name),
        format!("“{}” reached its loss limit ({detail}). New orders are refused until the halt ends or is lifted in Settings, Brokers; orders that reduce a position still go out.", account.name),
    );
}

fn notify(state: &AppState, title: String, details: String) {
    tracing::warn!("broker orders: {title}: {}", details.replace('\n', " | "));
    let (pool, cipher, http) = (state.pool.clone(), state.cipher.clone(), state.http.clone());
    tokio::spawn(async move {
        match otw_store::reminders::add_notification(&pool, &title, &details).await {
            Ok(n) => {
                crate::notif_send::dispatch(&pool, &cipher, &http, &n, NOTIFY_MODULE, None).await;
            }
            Err(e) => tracing::warn!("could not record the notice '{title}': {e:#}"),
        }
    });
}

/// Take the broker's state for a leg, and log the difference when there is one.
async fn adopt(state: &AppState, account: &BrokerRow, leg: &Leg, id: &str, status: OrderStatus, filled: Decimal, kind: &str) {
    let to = word(status);
    let filled_s = filled.normalize().to_string();
    let was_filled = leg.filled_qty.as_deref().and_then(|q| q.parse::<Decimal>().ok());
    let same = leg.status == to && leg.venue_id.as_deref() == Some(id) && was_filled.map(|q| q.normalize()) == Some(filled.normalize());
    if same {
        return;
    }
    if let Err(e) = legs::set_status(&state.pool, account.id, &leg.client_id, to, None, Some(id), Some(&filled_s)).await {
        tracing::error!("broker orders: could not update {} on “{}”: {e:#}", leg.client_id, account.name);
        return;
    }
    // A working leg that moved on is what a missed event looks like; the rest is settlement.
    let kind = match (leg.status.as_str(), to) {
        ("working", "filled") => "missed_fill",
        ("working", "cancelled" | "expired") => "missed_cancel",
        ("working", "working") if filled > was_filled.unwrap_or_default() => "missed_partial_fill",
        // Only its id or a first read of its size changed: nothing was missed.
        ("working", "working") => return,
        _ => kind,
    };
    log(state, account, kind, json!({ "client_id": leg.client_id, "id": id }), json!({ "was": leg.status, "now": to, "filled_qty": filled_s })).await;
}

/// Bring one leg in line with the broker.
async fn settle(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, leg: &Leg, now: OffsetDateTime) {
    let age = now - leg.created_at;
    match connector.lookup(settings, &leg.ticker, &leg.asset_type, &leg.client_id).await {
        Ok(Lookup::Found { id, status, filled_qty, .. }) => adopt(state, account, leg, &id, status, filled_qty, "settled_unknown").await,
        Ok(Lookup::NotFound) => {
            if age < ACK_WINDOW {
                return;
            }
            let n = legs::not_found(&state.pool, account.id, &leg.client_id).await.unwrap_or(0);
            if n < 2 {
                return;
            }
            // Never received, or (a working leg) forgotten by a broker that purges old orders.
            // One never received waits for the user, who sends it again or dismisses it.
            let (to, reason, kind) = if leg.status == "working" {
                ("cancelled", "the broker no longer knows this order", "missing_at_broker")
            } else {
                (legs::NOT_RECEIVED, "not received by the broker", "not_received")
            };
            if legs::set_status(&state.pool, account.id, &leg.client_id, to, Some(reason), None, None).await.is_ok() {
                log(state, account, kind, json!({ "client_id": leg.client_id }), json!({ "was": leg.status, "now": to })).await;
                if to == legs::NOT_RECEIVED {
                    notify(
                        state,
                        format!("Order {} on {} not received", leg.client_id, leg.ticker),
                        format!(
                            "“{}” never received order {} on {}: nothing is working at the broker. Send it again or dismiss it in the chart's order panel, Orders tab.",
                            account.name, leg.client_id, leg.ticker
                        ),
                    );
                }
            }
        }
        Err(e) => {
            if leg.status != "unknown" || age < DEADLINE {
                return;
            }
            if legs::mark_alerted(&state.pool, account.id, &leg.client_id).await.unwrap_or(false) {
                log(state, account, "unknown_past_deadline", json!({ "client_id": leg.client_id }), json!({ "error": format!("{e:#}") })).await;
                notify(
                    state,
                    format!("Order {} on {} not settled", leg.client_id, leg.ticker),
                    format!(
                        "OpenTraderWorld sent order {} on {} from “{}” and got no answer, and the broker could not be asked since ({e:#}).\n\nCheck the order at the broker. New orders on {} wait until it is settled.",
                        leg.client_id, leg.ticker, account.name, leg.ticker
                    ),
                );
            }
        }
    }
}

/// One account: every leg followed, the broker's working orders.
async fn reconcile(state: &AppState, account: &BrokerRow, external_seen: &mut HashSet<String>) -> Result<()> {
    let (connector, settings) = open_account(state, account).await?;
    let now = OffsetDateTime::now_utc();
    let open = legs::open(&state.pool, account.id).await?;
    for leg in open.iter().filter(|l| l.status != "pending" || now - l.updated_at > PENDING_STALE) {
        settle(state, account, connector.as_ref(), &settings, leg, now).await;
    }
    let http = super::client()?;
    if connector.capability().orders {
        let working = connector.orders(&http, &settings).await?;
        let ours: HashSet<String> = legs::open(&state.pool, account.id).await?.into_iter().filter_map(|l| l.venue_id).collect();
        for o in working.iter().filter(|o| !ours.contains(&o.id)) {
            if external_seen.insert(format!("{}:{}", account.id, o.id)) {
                log(state, account, "external_order", json!({ "id": o.id, "symbol": o.symbol }), json!({ "side": o.side, "type": o.order_type, "qty": o.qty })).await;
            }
        }
    }
    if let Err(e) = super::risk::watch_loss(state, account, connector.as_ref(), &settings).await {
        tracing::warn!("broker orders: loss limit of “{}”: {e:#}", account.name);
    }
    // The fills just settled are the ones the chart's journal books.
    crate::journal_import::chart::book_account(&state.pool, account, connector.as_ref(), &settings, &http).await;
    Ok(())
}

/// After a restart: legs left `pending` are unknown, and each broker takes back up what it
/// links itself.
async fn start(state: &AppState) -> Result<()> {
    let n = legs::pending_to_unknown(&state.pool).await?;
    if n > 0 {
        tracing::warn!("broker orders: {n} order(s) sent without an answer before the last stop are looked up");
    }
    for id in legs::accounts_open(&state.pool).await? {
        let Some(account) = otw_store::brokers::get(&state.pool, id).await? else { continue };
        let (connector, settings) = match open_account(state, &account).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("broker orders: “{}”: {e:#}", account.name);
                continue;
            }
        };
        let open = legs::open(&state.pool, id).await?;
        let minted: Vec<super::MintedId> = open
            .iter()
            .filter_map(|l| {
                let (vid, at) = (l.venue_client_id.clone()?, l.venue_client_at?);
                Some(super::MintedId { key: l.client_id.clone(), id: vid, at_ms: at, ticker: l.ticker.clone(), order_id: l.venue_id.clone() })
            })
            .collect();
        if !minted.is_empty() {
            if let Err(e) = connector.resume_client_ids(&settings, &minted).await {
                tracing::warn!("broker orders: “{}” could not take its minted client ids back up: {e:#}", account.name);
            }
        }
        let stored: Vec<StoredLeg> = open
            .into_iter()
            .filter(|l| l.status == "working")
            .map(|l| StoredLeg { key: l.client_id, id: l.venue_id, ticker: l.ticker, asset_type: l.asset_type, parent: l.parent, group: l.grp, status: l.status })
            .collect();
        if !stored.is_empty() {
            if let Err(e) = connector.resume(&settings, &stored).await {
                tracing::warn!("broker orders: “{}” could not take its linked orders back up: {e:#}", account.name);
            }
        }
    }
    Ok(())
}

/// The reconciliation cycle, on start and then per account: every [`BUSY`] while it has orders
/// out, every [`IDLE`] otherwise, and at once when poked.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        if let Err(e) = start(&state).await {
            tracing::warn!("broker orders: {e:#}");
        }
        let mut last: HashMap<Uuid, Instant> = HashMap::new();
        let mut external_seen: HashSet<String> = HashSet::new();
        loop {
            if let Err(e) = cycle(&state, &mut last, &mut external_seen).await {
                tracing::warn!("broker orders: {e:#}");
            }
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = POKE.notified() => {}
            }
        }
    });
}

async fn cycle(state: &AppState, last: &mut HashMap<Uuid, Instant>, external_seen: &mut HashSet<String>) -> Result<()> {
    let busy: HashSet<Uuid> = legs::accounts_open(&state.pool).await?.into_iter().collect();
    let poked: HashSet<Uuid> = std::mem::take(&mut *POKED.lock().unwrap_or_else(|e| e.into_inner()));
    for account in otw_store::brokers::list(&state.pool).await? {
        if !account.trades() && !busy.contains(&account.id) {
            continue;
        }
        let every = if busy.contains(&account.id) { BUSY } else { IDLE };
        let due = poked.contains(&account.id) || last.get(&account.id).is_none_or(|t| t.elapsed() >= every);
        if !due {
            continue;
        }
        last.insert(account.id, Instant::now());
        if let Err(e) = reconcile(state, &account, external_seen).await {
            tracing::warn!("broker orders: reconciling “{}”: {e:#}", account.name);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn unsent(id: &str, parent: Option<&str>, group: Option<&str>, side: &str) -> legs::Unsent {
        legs::Unsent {
            client_id: id.into(),
            ticker: "BTCUSDT".into(),
            asset_type: "crypto".into(),
            parent: parent.map(Into::into),
            grp: group.map(Into::into),
            request: serde_json::json!({ "key": id, "side": side, "type": "limit", "qty": "0.00124", "limit": "80500", "tif": "gtc", "parent": parent, "group": group }),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn orders_never_received_go_again_under_new_ids_their_exits_on_their_entry() {
        let legs = [unsent("k1b", Some("k1"), Some("g1"), "sell"), unsent("k1", None, None, "buy"), unsent("k1c", Some("k1"), Some("g1"), "sell")];
        let t = resend_ticket(&legs).unwrap();
        assert_eq!(t.ticker, "BTCUSDT");
        assert!(t.confirm_duplicate, "the first try is not a duplicate of the one the user sends again");
        let keys: Vec<_> = t.legs.iter().map(|l| (l.key.as_str(), l.parent.as_deref(), l.group.as_deref())).collect();
        assert_eq!(keys, [("k1r", None, None), ("k1br", Some("k1r"), Some("g1r")), ("k1cr", Some("k1r"), Some("g1r"))]);
        let alone = resend_ticket(&[unsent("k1b", Some("k1"), Some("g1"), "sell")]).unwrap_err().to_string();
        assert!(alone.contains("send them again together"), "{alone}");
    }

    #[test]
    fn a_broker_status_maps_to_where_the_leg_stands() {
        assert_eq!(word(OrderStatus::PartiallyFilled), "working");
        assert_eq!(word(OrderStatus::Filled), "filled");
        assert_eq!(word(OrderStatus::Expired), "expired");
    }
}
