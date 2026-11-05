//! Runs a [`Mapper`] against a real transport: resolves base URLs, signs, rate-limits, sends
//! with a timeout, and feeds the answer back to `decode`. Request/response only; sockets are
//! driven by the live adapter.
//!
//! The driver does not retry. A failure comes back as the `ConnectorError` the mapper
//! classified, and the caller's policy (the engine, or OTW's download worker with its own
//! backoff) decides what to do with it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use otw_connector_model::{
    Command, ConnectorError, ConnectorId, CorrId, DataType, Envelope, ErrorKind, Event, HistoryItems, InstrumentId,
    InstrumentSpec, TimeRange,
};

use crate::clock::{Clock, SystemClock};
use crate::limiter::{Limiter, NoLimit};
use crate::mapper::{MapCtx, Mapper};
use crate::native::{HttpRequest, Method, NativeRequest, RawFrame};
use crate::signer::{encode_query, SignableRequest, Signer};
use crate::symbols::InstrumentCache;

/// Called after every HTTP exchange; OTW plugs its API-rate dashboard in here.
pub trait Observer: Send + Sync {
    fn on_http(&self, url: &str, status: Option<u16>, retry_after: Option<&str>);
}

#[derive(Debug, Clone)]
pub struct DriverConfig {
    pub connector: ConnectorId,
    pub timeout: Duration,
    /// Base URL overrides by key (a sandbox, or a mock server in tests).
    pub base_overrides: HashMap<String, String>,
    /// Hard stop on a paged history, so a venue that keeps handing back a cursor cannot loop.
    pub max_pages: usize,
}

impl DriverConfig {
    pub fn new(connector: &str) -> Self {
        DriverConfig {
            connector: ConnectorId(connector.to_string()),
            timeout: Duration::from_secs(20),
            base_overrides: HashMap::new(),
            max_pages: 10_000,
        }
    }
    pub fn base(mut self, key: &str, url: &str) -> Self {
        self.base_overrides.insert(key.to_string(), url.trim_end_matches('/').to_string());
        self
    }
}

pub struct Driver<M: Mapper> {
    mapper: M,
    session: Mutex<M::Session>,
    instruments: RwLock<InstrumentCache>,
    cfg: DriverConfig,
    http: reqwest::Client,
    signer: Option<Arc<dyn Signer>>,
    /// Venue clock minus ours, in nanoseconds (see [`Driver::sync_time`]).
    offset_ns: std::sync::atomic::AtomicI64,
    limiter: Arc<dyn Limiter>,
    clock: Arc<dyn Clock>,
    observer: Option<Arc<dyn Observer>>,
}

fn poisoned() -> ConnectorError {
    ConnectorError::new(ErrorKind::Unknown, "adapter state lock poisoned")
}

impl<M: Mapper> Driver<M> {
    pub fn new(mapper: M, cfg: DriverConfig, http: reqwest::Client) -> Self {
        Driver {
            mapper,
            session: Mutex::new(M::Session::default()),
            instruments: RwLock::new(InstrumentCache::new()),
            cfg,
            http,
            signer: None,
            offset_ns: std::sync::atomic::AtomicI64::new(0),
            limiter: Arc::new(NoLimit),
            clock: Arc::new(SystemClock),
            observer: None,
        }
    }
    pub fn with_signer(mut self, s: Arc<dyn Signer>) -> Self {
        self.signer = Some(s);
        self
    }
    pub fn with_limiter(mut self, l: Arc<dyn Limiter>) -> Self {
        self.limiter = l;
        self
    }
    pub fn with_clock(mut self, c: Arc<dyn Clock>) -> Self {
        self.clock = c;
        self
    }
    pub fn with_observer(mut self, o: Arc<dyn Observer>) -> Self {
        self.observer = Some(o);
        self
    }

    pub fn mapper(&self) -> &M {
        &self.mapper
    }

    pub fn connector(&self) -> &ConnectorId {
        &self.cfg.connector
    }

    pub fn instruments(&self) -> Result<InstrumentCache, ConnectorError> {
        self.instruments.read().map(|g| g.clone()).map_err(|_| poisoned())
    }

    pub fn set_instruments(&self, cache: InstrumentCache) -> Result<(), ConnectorError> {
        *self.instruments.write().map_err(|_| poisoned())? = cache;
        Ok(())
    }

    fn with_session<R>(
        &self,
        f: impl FnOnce(&M, &mut M::Session, &MapCtx<'_>) -> Result<R, ConnectorError>,
    ) -> Result<R, ConnectorError> {
        let inst = self.instruments.read().map_err(|_| poisoned())?;
        let ctx = MapCtx { received_ts: self.clock.now(), instruments: &inst, connector: &self.cfg.connector };
        let mut s = self.session.lock().map_err(|_| poisoned())?;
        f(&self.mapper, &mut s, &ctx)
    }

    pub fn base_url(&self, key: &str) -> Result<String, ConnectorError> {
        if let Some(u) = self.cfg.base_overrides.get(key) {
            return Ok(u.clone());
        }
        self.mapper
            .base_urls()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, u)| u.trim_end_matches('/').to_string())
            .ok_or_else(|| ConnectorError::invalid(format!("no base URL `{key}`")))
    }

    /// Send one HTTP request and hand back the frame. A transport failure that never reached
    /// the venue is `Transient`; a timeout is a `Timeout` frame the mapper can classify.
    pub async fn send(&self, req: HttpRequest) -> Result<RawFrame, ConnectorError> {
        let signer = self.signer.clone();
        self.send_as(req, signer.as_deref()).await
    }

    /// The venue's clock as best known: ours plus the offset the last `sync_time` measured.
    pub fn venue_now(&self) -> otw_connector_model::Timestamp {
        self.clock.now().plus_nanos(self.offset_ns.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// [`Driver::send`] signed with these credentials rather than the driver's own: one
    /// shared driver serves every account on a venue.
    /// Exchange the signer's long-lived credential for an access token at the sign-in host.
    async fn mint(&self, signer: &dyn Signer, t: crate::signer::TokenRequest) -> Result<(), ConnectorError> {
        let url = format!("{}{}", self.base_url(t.base)?, t.path);
        self.limiter.acquire("token", 1).await;
        let (kind, body) = match t.body {
            crate::signer::TokenBody::Form(f) => ("application/x-www-form-urlencoded", encode_query(&f)),
            crate::signer::TokenBody::Json(j) => ("application/json", j),
        };
        let method = match t.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Delete => reqwest::Method::DELETE,
        };
        let mut rb = self.http.request(method, &url).timeout(self.cfg.timeout).header("Content-Type", kind);
        for (k, v) in &t.headers {
            rb = rb.header(k, v);
        }
        let resp = rb.body(body).send().await.map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("token: {}", e.without_url())))?;
        let status = resp.status().as_u16();
        if let Some(o) = &self.observer {
            o.on_http(&url, Some(status), None);
        }
        let headers: Vec<(String, String)> =
            resp.headers().iter().filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_ascii_lowercase(), v.to_string()))).collect();
        let body = resp.bytes().await.map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("token: reading body: {}", e.without_url())))?;
        signer.accept_token(status, &headers, &body, self.venue_now())
    }

    /// Mint what the signer still needs, step by step, before a signed call or a socket
    /// that authenticates with the session.
    pub async fn ensure_token(&self, signer: &dyn Signer) -> Result<(), ConnectorError> {
        // A sign-in is at most a few steps; a signer that keeps asking is a bug, not a loop.
        for _ in 0..4 {
            match signer.token_request(self.venue_now()) {
                Some(t) => self.mint(signer, t).await?,
                None => return Ok(()),
            }
        }
        Err(ConnectorError::new(ErrorKind::Auth, "the sign-in did not settle after four steps"))
    }

    pub async fn send_as(&self, req: HttpRequest, signer: Option<&dyn Signer>) -> Result<RawFrame, ConnectorError> {
        let base = self.base_url(req.base)?;
        let mut signable = SignableRequest {
            method: req.method,
            path: req.path.clone(),
            query: req.query.clone(),
            headers: req.headers.clone(),
            body: req.body.clone(),
        };
        if req.signed {
            let signer = signer.ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "this call needs credentials and none are set"))?;
            self.ensure_token(signer).await?;
            signer.sign(&mut signable, self.venue_now())?;
        }
        self.limiter.acquire(req.endpoint_id, req.weight).await;

        let mut url = format!("{base}{}", signable.path);
        if !signable.query.is_empty() {
            url.push('?');
            url.push_str(&encode_query(&signable.query));
        }
        let method = match signable.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Delete => reqwest::Method::DELETE,
        };
        let mut rb = self.http.request(method, &url).timeout(self.cfg.timeout);
        for (k, v) in &signable.headers {
            rb = rb.header(k, v);
        }
        if let Some(b) = signable.body {
            rb = rb.body(b);
        }
        match rb.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let headers: Vec<(String, String)> = resp
                    .headers()
                    .iter()
                    .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_ascii_lowercase(), v.to_string())))
                    .collect();
                if let Some(o) = &self.observer {
                    let ra = headers.iter().find(|(k, _)| k == "retry-after").map(|(_, v)| v.as_str());
                    o.on_http(&url, Some(status), ra);
                }
                if status == 401 && req.signed {
                    if let Some(s) = signer {
                        s.forget_token();
                    }
                }
                if status == 429 || status == 418 {
                    if let Some(ms) = headers
                        .iter()
                        .find(|(k, _)| k == "retry-after")
                        .and_then(|(_, v)| v.trim().parse::<u64>().ok())
                        .map(|s| s.saturating_mul(1000))
                    {
                        self.limiter.penalize(ms);
                    }
                }
                let body = resp
                    .bytes()
                    .await
                    .map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("{}: reading body: {}", req.endpoint_id, e.without_url())))?;
                Ok(RawFrame::Http { corr_id: req.corr_id, endpoint_id: req.endpoint_id, status, headers, body })
            }
            Err(e) => {
                if let Some(o) = &self.observer {
                    o.on_http(&url, None, None);
                }
                if e.is_timeout() {
                    Ok(RawFrame::Timeout { corr_id: req.corr_id, endpoint_id: req.endpoint_id })
                } else {
                    Err(ConnectorError::new(ErrorKind::Transient, format!("{}: {}", req.endpoint_id, e.without_url())))
                }
            }
        }
    }

    /// Encode a command, send what it needs, decode the answers.
    pub async fn execute(&self, cmd: &Command) -> Result<Vec<Envelope>, ConnectorError> {
        let signer = self.signer.clone();
        self.execute_as(cmd, signer.as_deref()).await
    }

    /// [`Driver::execute`] with these credentials.
    pub async fn execute_as(&self, cmd: &Command, signer: Option<&dyn Signer>) -> Result<Vec<Envelope>, ConnectorError> {
        let mut queue: std::collections::VecDeque<NativeRequest> = self.with_session(|m, s, ctx| m.encode(cmd, s, ctx))?.into();
        let mut out = Vec::new();
        while let Some(r) = queue.pop_front() {
            match r {
                NativeRequest::Http(h) => {
                    let corr = h.corr_id;
                    let frame = match self.send_as(h, signer).await {
                        Ok(f) => f,
                        Err(e) => {
                            self.with_session(|m, s, _| {
                                m.abandon(corr, s);
                                Ok(())
                            })?;
                            return Err(e);
                        }
                    };
                    let evs = match self.with_session(|m, s, ctx| m.decode(&frame, s, ctx)) {
                        Ok(evs) => evs,
                        Err(e) => {
                            self.with_session(|m, s, _| {
                                m.abandon(corr, s);
                                Ok(())
                            })?;
                            return Err(e);
                        }
                    };
                    out.extend(evs);
                    // What this answer says to ask next, asked before anything queued after it.
                    let next = self.with_session(|m, s, _| Ok(m.follow_ups(corr, s)))?;
                    for n in next.into_iter().rev() {
                        queue.push_front(n);
                    }
                }
                NativeRequest::WsOpen { .. } | NativeRequest::WsOpenUrl { .. } | NativeRequest::WsSend { .. } | NativeRequest::WsSigned { .. } | NativeRequest::WsClose { .. } | NativeRequest::WsLogin { .. } => {
                    return Err(ConnectorError::unsupported("socket requests go through the live adapter"));
                }
            }
        }
        Ok(out)
    }

    /// A whole history range, following the venue's cursor until it says done. Chunks come
    /// back in the order the venue served them.
    pub async fn history(
        &self,
        req_id: CorrId,
        instrument: &InstrumentId,
        data_type: &DataType,
        range: TimeRange,
    ) -> Result<Vec<HistoryItems>, ConnectorError> {
        let signer = self.signer.clone();
        self.history_as(req_id, instrument, data_type, range, signer.as_deref()).await
    }

    /// [`Driver::history`] with these credentials (a keyed venue).
    pub async fn history_as(
        &self,
        req_id: CorrId,
        instrument: &InstrumentId,
        data_type: &DataType,
        range: TimeRange,
        signer: Option<&dyn Signer>,
    ) -> Result<Vec<HistoryItems>, ConnectorError> {
        let mut cursor: Option<String> = None;
        let mut out = Vec::new();
        for _ in 0..self.cfg.max_pages {
            let cmd = Command::RequestHistory {
                req_id,
                instrument: instrument.clone(),
                data_type: data_type.clone(),
                range,
                cursor: cursor.clone(),
            };
            let mut next = None;
            let mut done = false;
            for env in self.execute_as(&cmd, signer).await? {
                match env.event {
                    Event::HistoryChunk { items, next_cursor, done: d } => {
                        out.push(items);
                        next = next_cursor;
                        done = d;
                    }
                    Event::Error(e) => return Err(e),
                    _ => {}
                }
            }
            match next {
                Some(c) if !done => {
                    if cursor.as_deref() == Some(c.as_str()) {
                        return Err(ConnectorError::new(ErrorKind::Decode, format!("cursor `{c}` did not advance")));
                    }
                    cursor = Some(c);
                }
                _ => {
                    // A venue that pages backwards hands the newest page first: chunks come
                    // back in time order whatever the walk's direction.
                    out.sort_by_key(|items| match items {
                        HistoryItems::Candles(c) => c.first().map(|c| c.open_ts),
                        HistoryItems::Trades(_) => None,
                    });
                    return Ok(out);
                }
            }
        }
        Err(ConnectorError::invalid(format!("history did not finish within {} pages", self.cfg.max_pages)))
    }

    /// Measure the venue's clock, so a signature carries its time rather than ours: a host
    /// a few seconds off is otherwise refused with an error nobody reads as a clock problem.
    /// `Ok(false)` when the venue has no clock endpoint.
    pub async fn sync_time(&self, req_id: CorrId) -> Result<bool, ConnectorError> {
        let evs = match self.execute_as(&Command::SyncTime { req_id }, None).await {
            Ok(evs) => evs,
            Err(e) if e.kind == ErrorKind::Unsupported => return Ok(false),
            Err(e) => return Err(e),
        };
        for env in evs {
            match env.event {
                Event::ServerTime(t) => {
                    let offset = t.nanos().saturating_sub(env.received_ts.nanos());
                    self.offset_ns.store(offset, std::sync::atomic::Ordering::Relaxed);
                    return Ok(true);
                }
                Event::Error(e) => return Err(e),
                _ => {}
            }
        }
        Err(ConnectorError::decode("the venue's clock answer carried no time"))
    }

    /// A whole account query, following the cursor like a history.
    pub async fn account(
        &self,
        req_id: CorrId,
        kind: otw_connector_model::AccountKind,
        symbols: &[InstrumentId],
        range: Option<TimeRange>,
        account: Option<&str>,
        signer: Option<&dyn Signer>,
    ) -> Result<Vec<otw_connector_model::AccountItems>, ConnectorError> {
        let mut cursor: Option<String> = None;
        let mut out = Vec::new();
        for _ in 0..self.cfg.max_pages {
            let cmd = Command::QueryAccount { req_id, kind, symbols: symbols.to_vec(), range, cursor: cursor.clone(), account: account.map(str::to_string) };
            let mut next = None;
            let mut done = true;
            for env in self.execute_as(&cmd, signer).await? {
                match env.event {
                    Event::AccountChunk { items, next_cursor, done: d } => {
                        out.push(items);
                        next = next_cursor;
                        done = d;
                    }
                    Event::Error(e) => return Err(e),
                    _ => {}
                }
            }
            match next {
                Some(c) if !done => {
                    if cursor.as_deref() == Some(c.as_str()) {
                        return Err(ConnectorError::new(ErrorKind::Decode, format!("cursor `{c}` did not advance")));
                    }
                    cursor = Some(c);
                }
                _ => return Ok(out),
            }
        }
        Err(ConnectorError::invalid(format!("account query did not finish within {} pages", self.cfg.max_pages)))
    }

    /// Last price of every listed instrument.
    pub async fn board(&self, req_id: CorrId) -> Result<Vec<otw_connector_model::LastPrice>, ConnectorError> {
        for env in self.execute_as(&Command::RequestBoard { req_id }, None).await? {
            match env.event {
                Event::Board(b) => return Ok(b),
                Event::Error(e) => return Err(e),
                _ => {}
            }
        }
        Err(ConnectorError::decode("the price board answer carried no prices"))
    }

    /// What these instruments are, asked one by one; the answers join the cache.
    pub async fn resolve_instruments(&self, req_id: CorrId, ids: &[InstrumentId], signer: Option<&dyn Signer>) -> Result<Vec<InstrumentSpec>, ConnectorError> {
        let evs = self.execute_as(&Command::ResolveInstruments { req_id, ids: ids.to_vec() }, signer).await?;
        let mut specs = Vec::new();
        for env in evs {
            match env.event {
                Event::Instrument { spec } => specs.push(*spec),
                Event::Error(e) => return Err(e),
                _ => {}
            }
        }
        let mut cache = self.instruments()?;
        for s in &specs {
            cache.insert(s.clone());
        }
        self.set_instruments(cache)?;
        Ok(specs)
    }

    /// The bid/ask in force at `at` (first at or after it, or last before it).
    pub async fn quote_at(
        &self,
        req_id: CorrId,
        instrument: &InstrumentId,
        at: otw_connector_model::Timestamp,
        before: bool,
        signer: Option<&dyn Signer>,
    ) -> Result<Option<otw_connector_model::Quote>, ConnectorError> {
        let cmd = Command::RequestQuoteAt { req_id, instrument: instrument.clone(), at, before };
        for env in self.execute_as(&cmd, signer).await? {
            match env.event {
                Event::Quote(q) => return Ok(Some(q)),
                Event::Error(e) => return Err(e),
                _ => {}
            }
        }
        Ok(None)
    }

    /// Load the venue's instrument list into the cache and return it.
    pub async fn load_instruments(&self, req_id: CorrId) -> Result<Vec<InstrumentSpec>, ConnectorError> {
        let signer = self.signer.clone();
        self.load_instruments_as(req_id, signer.as_deref()).await
    }

    /// [`Driver::load_instruments`] with these credentials (a venue whose list is keyed).
    pub async fn load_instruments_as(&self, req_id: CorrId, signer: Option<&dyn Signer>) -> Result<Vec<InstrumentSpec>, ConnectorError> {
        let evs = self.execute_as(&Command::LoadInstruments { req_id }, signer).await?;
        let mut specs = Vec::new();
        for env in evs {
            match env.event {
                Event::Instrument { spec } => specs.push(*spec),
                Event::Error(e) => return Err(e),
                _ => {}
            }
        }
        let mut cache = InstrumentCache::new();
        for s in &specs {
            cache.insert(s.clone());
        }
        self.set_instruments(cache)?;
        Ok(specs)
    }
}
