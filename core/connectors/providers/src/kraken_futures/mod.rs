//! Kraken Futures (Kraken Derivatives, `futures.kraken.com`): the account read, the contract
//! list it needs, and orders on the multi-collateral contracts (`trade.rs`). A platform of its
//! own, apart from Kraken spot: its own host, its own keys, its own signature, its own demo
//! environment (`demo-futures.kraken.com`).
//! Endpoint-by-endpoint mapping in PROVIDER.md.
//!
//! A contract's size is not always counted in the coin. The list's `type` says how:
//!   - `flexible_futures` (`PF_` perpetuals, `FF_` dated): linear, multi-collateral, sized in
//!     the base coin (`contractSize` 1, steps of `10^-contractValueTradePrecision`);
//!   - `futures_inverse` (`PI_`, `FI_`): sized in USD contracts and settled in the coin.
//!
//! The account panel reads coins, so an inverse row is turned into coins at its own price,
//! and its fills are not imported (their profit is counted in the coin). The list is read
//! once an hour; a contract that is no longer listed (matured) is looked up in the list of
//! expired ones, read only when a row names such a contract.

mod account;
mod errors;
mod signer;
mod trade;
mod trade_errors;

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use otw_connector_model::{
    AssetClass, Capabilities, CapsError, Command, ConnectorError, ContractType, CorrId, Decimal, Envelope, ErrorKind, Event, InstrumentId,
    InstrumentSpec, Price, Qty, Timestamp,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;

pub const VENUE: &str = "KRAKEN_FUTURES";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://futures.kraken.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Order entry, account and contract list.
const API: &str = "/derivatives/api/v3";
/// The account's history (executions).
const HISTORY: &str = "/api/history/v3";
/// How long the contract lists the session keeps are trusted before a reload.
const LIST_TTL_NS: i64 = 3_600_000_000_000;

#[derive(Clone)]
pub struct KrakenFutures {
    caps: Capabilities,
}

impl KrakenFutures {
    pub fn new() -> Result<Self, CapsError> {
        Ok(KrakenFutures { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// How a contract's size is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sizing {
    /// In the base coin, `contract_size` coins a contract.
    Linear,
    /// In contracts of `contract_size` quote units (USD), settled in the coin.
    Inverse,
    /// A type this adapter does not read (`futures_vanilla`, `options`): its word.
    Other,
}

/// One contract as the session keeps it.
#[derive(Debug, Clone)]
pub(crate) struct Contract {
    pub sizing: Sizing,
    pub kind: String,
    pub base: String,
    pub quote: String,
    pub contract_size: Decimal,
    pub dated: bool,
}

impl Contract {
    pub fn class(&self) -> AssetClass {
        match self.sizing {
            Sizing::Linear | Sizing::Inverse if self.dated => AssetClass::Future,
            Sizing::Linear | Sizing::Inverse => AssetClass::Perp,
            Sizing::Other => AssetClass::Other,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentRow {
    symbol: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    contract_size: OptDec,
    #[serde(default)]
    contract_value_trade_precision: Option<i64>,
    #[serde(default)]
    last_trading_time: Option<String>,
    #[serde(default)]
    suspended: Option<bool>,
    #[serde(default)]
    post_only: Option<bool>,
    #[serde(default)]
    is_expired: Option<bool>,
    #[serde(default)]
    tradeable: Option<bool>,
    #[serde(default)]
    max_position_size: OptDec,
}

#[derive(Deserialize)]
struct InstrumentsPage {
    #[serde(default)]
    instruments: Vec<InstrumentRow>,
}

impl InstrumentRow {
    fn contract(&self) -> Contract {
        let kind = self.kind.clone().unwrap_or_default();
        let sizing = match kind.as_str() {
            "flexible_futures" => Sizing::Linear,
            "futures_inverse" => Sizing::Inverse,
            _ => Sizing::Other,
        };
        Contract {
            sizing,
            kind,
            base: self.base.clone().unwrap_or_default().to_ascii_uppercase(),
            quote: self.quote.clone().unwrap_or_default().to_ascii_uppercase(),
            contract_size: self.contract_size.0.filter(|c| !c.is_zero()).unwrap_or(Decimal::ONE),
            dated: self.last_trading_time.as_deref().is_some_and(|t| !t.is_empty()),
        }
    }

    /// The step a size moves by, in contracts: `10^-precision` (a negative precision is a
    /// step above one).
    fn lot(&self) -> Option<Decimal> {
        let p = self.contract_value_trade_precision?;
        if p >= 0 {
            Decimal::try_from_i128_with_scale(1, u32::try_from(p).ok()?).ok()
        } else {
            let e = u32::try_from(p.checked_neg()?).ok()?;
            10i64.checked_pow(e).map(Decimal::from)
        }
    }

    fn spec(&self) -> Result<InstrumentSpec, ConnectorError> {
        let c = self.contract();
        let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &self.symbol.to_ascii_uppercase()), &c.base, &c.quote);
        spec.symbol = otw_connector_model::CanonicalSymbol(self.symbol.to_ascii_uppercase());
        spec.asset_class = c.class();
        spec.contract = if c.dated { ContractType::Future } else { ContractType::Perp };
        spec.tick_size = self.tick_size.0.map(Price);
        spec.lot_size = self.lot().map(Qty);
        spec.rules.max_qty = self.max_position_size.0;
        spec.expiry = self.last_trading_time.as_deref().filter(|t| !t.is_empty()).map(ts_rfc3339).transpose()?;
        spec.active = !self.suspended.unwrap_or(false) && !self.is_expired.unwrap_or(false) && self.tradeable.unwrap_or(true);
        match c.sizing {
            Sizing::Linear => {
                spec.multiplier = c.contract_size;
                spec.note = Some(if self.post_only.unwrap_or(false) { "multi-collateral, post-only".into() } else { "multi-collateral".into() });
            }
            Sizing::Inverse => {
                // Sized in USD contracts: a contract's coins depend on the price.
                spec.inverse = true;
                spec.multiplier = Decimal::ONE;
                spec.note = Some(format!("inverse: one contract is {} {}, settled in {}", c.contract_size.normalize(), c.quote, c.base));
            }
            Sizing::Other => spec.note = Some(c.kind.clone()),
        }
        Ok(spec)
    }
}

/// Contract lists by symbol (upper case), with when each was read.
type Contracts = Arc<HashMap<String, Contract>>;

enum Pending {
    Instruments,
    /// The current list, read for the account rows' contracts.
    Contracts,
    /// The expired list, read because a row named a contract the current one lacks.
    Expired,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    current: Option<(Timestamp, Contracts)>,
    expired: Option<(Timestamp, Contracts)>,
    /// Marks read for a positions request, by its id.
    marks: HashMap<CorrId, HashMap<String, Decimal>>,
    /// An answer held back until the expired list is read, by its request id.
    stash: HashMap<CorrId, (account::Pending, Bytes)>,
    /// Requests to send next for a request id (the expired list).
    follow: HashMap<CorrId, Vec<NativeRequest>>,
}

impl Session {
    fn fresh(slot: &Option<(Timestamp, Contracts)>, now: Timestamp) -> bool {
        slot.as_ref().is_some_and(|(at, _)| now.nanos().saturating_sub(at.nanos()) < LIST_TTL_NS)
    }

    /// The contract a symbol names: the current list first, then the expired one.
    pub(crate) fn contract(&self, symbol: &str) -> Option<&Contract> {
        let s = symbol.to_ascii_uppercase();
        self.current.as_ref().and_then(|(_, m)| m.get(&s)).or_else(|| self.expired.as_ref().and_then(|(_, m)| m.get(&s)))
    }

    fn expired_read(&self, now: Timestamp) -> bool {
        Self::fresh(&self.expired, now)
    }

    /// The current list request, when the session's is missing or stale.
    fn need_contracts(&mut self, req_id: CorrId, now: Timestamp) -> Option<NativeRequest> {
        if Self::fresh(&self.current, now) {
            return None;
        }
        self.pending.insert((req_id, "contracts"), Pending::Contracts);
        Some(NativeRequest::Http(HttpRequest::get("contracts", REST, format!("{API}/instruments"), req_id)))
    }

    /// Hold `body` back and ask for the expired list first: `p`'s rows name a contract the
    /// current list lacks.
    fn defer(&mut self, req_id: CorrId, p: account::Pending, body: &[u8]) {
        self.stash.insert(req_id, (p, Bytes::copy_from_slice(body)));
        self.pending.insert((req_id, "expired"), Pending::Expired);
        let h = HttpRequest::get("expired", REST, format!("{API}/instruments"), req_id).query("expired", "true");
        self.follow.entry(req_id).or_default().push(NativeRequest::Http(h));
    }
}

fn contracts_of(rows: &[InstrumentRow]) -> Contracts {
    Arc::new(rows.iter().map(|r| (r.symbol.to_ascii_uppercase(), r.contract())).collect())
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kraken_futures {what}: {e}"))
}

impl KrakenFutures {
    fn account_declared(&self, kind: otw_connector_model::AccountKind) -> bool {
        use otw_connector_model::AccountKind as K;
        let a = &self.caps.account;
        match kind {
            K::Balances => a.balances,
            K::Positions => a.positions,
            K::Fills => a.fills_history,
            K::OpenOrders => a.open_orders,
            K::Summary => a.summary,
        }
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: InstrumentsPage = serde_json::from_slice(body).map_err(|e| decode_err("instruments", e))?;
        s.current = Some((ctx.received_ts, contracts_of(&page.instruments)));
        let mut out = Vec::with_capacity(page.instruments.len() + 1);
        for r in &page.instruments {
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(r.spec()?) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for KrakenFutures {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // As Kraken writes it on every row (`PF_XBTUSD`, `FI_XBTUSD_260925`); nothing is
        // reshaped but the case.
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':')) {
            return Err(ConnectorError::not_found(format!(
                "Kraken Futures: bad symbol `{symbol}`, write the contract as Kraken lists it (PF_XBTUSD, PF_ETHUSD)"
            )));
        }
        Ok(InstrumentId::new(VENUE, &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::LoadInstruments { req_id } => {
                s.pending.insert((*req_id, "instruments"), Pending::Instruments);
                Ok(vec![NativeRequest::Http(HttpRequest::get("instruments", REST, format!("{API}/instruments"), *req_id))])
            }
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, s, ctx),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("Kraken Futures: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(errors::from_frame(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } => return Err(ConnectorError::unsupported("Kraken Futures: no socket is declared")),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let mut pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        // The history API hands the next page's token in a header too ("Opaque token from the
        // `Next-Continuation-Token` header"): kept with the page.
        if let Pending::Account(account::Pending::Fills { header, .. }) = &mut pending {
            *header = frame.header("Next-Continuation-Token").map(str::to_string);
        }
        if let Some(e) = errors::from_frame(frame) {
            // Marks are a convenience for positions: without them a position has no mark.
            if matches!(pending, Pending::Account(account::Pending::Marks)) && e.kind != ErrorKind::RateLimited {
                return Ok(vec![]);
            }
            return Err(e);
        }
        match pending {
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::Contracts => {
                let page: InstrumentsPage = serde_json::from_slice(body).map_err(|e| decode_err("instruments", e))?;
                s.current = Some((ctx.received_ts, contracts_of(&page.instruments)));
                Ok(vec![])
            }
            Pending::Expired => {
                let page: InstrumentsPage = serde_json::from_slice(body).map_err(|e| decode_err("expired instruments", e))?;
                s.expired = Some((ctx.received_ts, contracts_of(&page.instruments)));
                match s.stash.remove(&corr) {
                    Some((p, held)) => account::decode(p, corr, &held, s, ctx),
                    None => Ok(vec![]),
                }
            }
            Pending::Account(p) => account::decode(p, corr, body, s, ctx),
        }
    }

    fn follow_ups(&self, corr_id: CorrId, s: &mut Session) -> Vec<NativeRequest> {
        s.follow.remove(&corr_id).unwrap_or_default()
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
        s.marks.remove(&corr_id);
        s.stash.remove(&corr_id);
        s.follow.remove(&corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| SecretString::from(v.to_string()))
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
        };
        Ok(Some(Arc::new(signer::KrakenFuturesSigner::new(get("api_key")?, get("api_secret")?)?)))
    }
}

#[cfg(test)]
mod tests;
