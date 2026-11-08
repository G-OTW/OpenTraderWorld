//! The error matrix every HTTP history adapter is run through: the answers a venue can give
//! (refusals by status, throttling with `Retry-After`, an outage, a hung connection, a
//! garbled or empty body), served by a mock venue and read through the real driver, so the
//! classification the engine acts on is the one under test, not a hand-built frame.
//!
//! An adapter's own file calls one `cases::*` function per test, so a case that reveals a
//! bug can be quarantined on its own (`#[ignore = "bug: ..."]`) without hiding the others.
//! The venue-specific answers (nominal, unknown symbol, a missing or null field, a gap) are
//! written beside the adapter with [`Probe::serve`].

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use otw_connector_model::{Candle, ConnectorError, CorrId, DataType, ErrorKind, HistoryItems, InstrumentId, TimeRange, Timestamp};
use otw_connector_sdk::{Driver, DriverConfig, Limiter, Mapper, Signer};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::ManualClock;

/// A limiter that grants everything and remembers every back-off the driver asked for.
#[derive(Default)]
pub struct RecordingLimiter(Mutex<Vec<u64>>);

impl RecordingLimiter {
    pub fn penalties(&self) -> Vec<u64> {
        self.0.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

// Written out by hand (it is what `#[async_trait]` expands to) so the testkit needs no macro
// crate of its own.
impl Limiter for RecordingLimiter {
    fn acquire<'a, 'b, 'c>(&'a self, _endpoint_id: &'b str, _weight: u32) -> Pin<Box<dyn Future<Output = ()> + Send + 'c>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        Box::pin(async {})
    }
    fn penalize(&self, retry_after_ms: u64) {
        if let Ok(mut v) = self.0.lock() {
            v.push(retry_after_ms);
        }
    }
}

/// What one history request against the mock venue came back with.
pub struct Outcome {
    pub result: Result<Vec<HistoryItems>, ConnectorError>,
    /// Back-offs the driver handed the limiter.
    pub penalties: Vec<u64>,
    /// Requests the venue received.
    pub requests: usize,
}

impl Outcome {
    pub fn err(&self) -> &ConnectorError {
        match &self.result {
            Err(e) => e,
            Ok(items) => panic!("expected an error, got {} chunk(s): {items:?}", items.len()),
        }
    }
    pub fn kind(&self) -> ErrorKind {
        self.err().kind
    }
    /// Every candle served, in order, failing on an error.
    pub fn candles(&self) -> Vec<Candle> {
        match &self.result {
            Ok(items) => items
                .iter()
                .flat_map(|i| match i {
                    HistoryItems::Candles(c) => c.clone(),
                    HistoryItems::Trades(_) => Vec::new(),
                })
                .collect(),
            Err(e) => panic!("expected candles, got {e}"),
        }
    }
}

/// One history request, ready to be answered any way a test likes.
pub struct Probe<M: Mapper> {
    make: Box<dyn Fn() -> M + Send + Sync>,
    secrets: HashMap<String, String>,
    pub instrument: InstrumentId,
    pub data_type: DataType,
    pub range: TimeRange,
    pub now: Timestamp,
}

impl<M: Mapper> Probe<M> {
    pub fn new(make: impl Fn() -> M + Send + Sync + 'static, instrument: InstrumentId, data_type: DataType, range: TimeRange, now: Timestamp) -> Self {
        Probe { make: Box::new(make), secrets: HashMap::new(), instrument, data_type, range, now }
    }

    /// Placeholder credentials for a keyed venue: the mock checks nothing, the driver only
    /// needs a signer to send a signed call.
    pub fn secrets(mut self, pairs: &[(&str, &str)]) -> Self {
        self.secrets = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self
    }

    pub async fn serve(&self, answer: ResponseTemplate) -> Outcome {
        self.serve_with(answer, Duration::from_secs(5)).await
    }

    /// Every request, whatever its path, gets `answer`.
    pub async fn serve_with(&self, answer: ResponseTemplate, timeout: Duration) -> Outcome {
        let server = MockServer::start().await;
        Mock::given(any()).respond_with(answer).mount(&server).await;
        let mapper = (self.make)();
        let signer: Option<Arc<dyn Signer>> = if self.secrets.is_empty() {
            None
        } else {
            mapper.signer(&self.secrets).expect("placeholder secrets build a signer")
        };
        let mut cfg = DriverConfig::new("test");
        for (k, _) in mapper.base_urls() {
            cfg = cfg.base(k, &server.uri());
        }
        cfg.timeout = timeout;
        // A venue that keeps answering a cursor must not spin a test for 10 000 pages.
        cfg.max_pages = 50;
        let limiter = Arc::new(RecordingLimiter::default());
        let mut drv = Driver::new(mapper, cfg, reqwest::Client::new())
            .with_clock(ManualClock::at(self.now))
            .with_limiter(limiter.clone());
        if let Some(s) = signer {
            drv = drv.with_signer(s);
        }
        let result = drv.history(CorrId(1), &self.instrument, &self.data_type, self.range).await;
        let requests = server.received_requests().await.map(|r| r.len()).unwrap_or(0);
        Outcome { result, penalties: limiter.penalties(), requests }
    }
}

/// The generic cases. Each asserts what the engine needs to decide its policy: the kind
/// (retry, stop, fail the request) and, for a throttle, how long to wait.
pub mod cases {
    use super::*;

    fn text(status: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_string(body.to_string())
    }

    fn expect<M: Mapper>(p: &Probe<M>, o: &Outcome, want: ErrorKind, case: &str) {
        let e = o.err();
        assert_eq!(e.kind, want, "{case} on {}: {e}", p.instrument);
        assert!(!e.message.trim().is_empty(), "{case}: an error names what happened");
    }

    /// A rejected request (400, plain body) is the request's fault: not retried.
    pub async fn bad_request_is_invalid<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(400, "Bad Request")).await;
        expect(p, &o, ErrorKind::InvalidRequest, "400");
    }

    /// Credentials refused: the connector stops until the user fixes them.
    pub async fn unauthorized_is_auth<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(401, "Unauthorized")).await;
        expect(p, &o, ErrorKind::Auth, "401");
    }

    /// Credentials fine, entitlement missing.
    pub async fn forbidden_is_permission<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(403, "Forbidden")).await;
        expect(p, &o, ErrorKind::Permission, "403");
    }

    /// A throttle carries the venue's wait, and the driver backs the limiter off by it.
    pub async fn rate_limit_carries_retry_after<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(429, "Too Many Requests").insert_header("Retry-After", "7")).await;
        expect(p, &o, ErrorKind::RateLimited, "429");
        assert_eq!(o.err().retry_after_ms, Some(7_000), "429: the wait the venue asked for is kept");
        assert_eq!(o.penalties, vec![7_000], "429: the limiter is told to back off");
        assert_eq!(o.requests, 1, "429: the driver does not retry on its own");
    }

    /// An outage with a plain body: retry with backoff.
    pub async fn server_error_is_transient<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(500, "Internal Server Error")).await;
        expect(p, &o, ErrorKind::Transient, "500");
        assert!(o.err().is_retryable());
    }

    /// An outage answered by a gateway in JSON (`{"message":"..."}`, the shape API gateways
    /// and CDNs write): the status decides, never the body's lack of data.
    pub async fn gateway_json_outage_is_transient<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(503, r#"{"message":"Service Unavailable"}"#).insert_header("content-type", "application/json")).await;
        expect(p, &o, ErrorKind::Transient, "503 with a JSON body");
    }

    /// No answer in time.
    pub async fn timeout_is_transient<M: Mapper>(p: &Probe<M>) {
        let slow = text(200, "[]").set_delay(Duration::from_secs(3));
        let o = p.serve_with(slow, Duration::from_millis(150)).await;
        expect(p, &o, ErrorKind::Transient, "timeout");
    }

    /// A body cut mid-way: a decode error, never a partial history.
    pub async fn malformed_json_is_decode<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(200, r#"{"data":[[1700000000,"1.0","#)).await;
        expect(p, &o, ErrorKind::Decode, "malformed JSON");
    }

    /// A 200 with nothing in it is not an answer.
    pub async fn empty_body_is_decode<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(200, "")).await;
        expect(p, &o, ErrorKind::Decode, "empty body");
    }

    /// A 200 carrying an HTML page (a captive portal, a CDN challenge): not data.
    pub async fn html_page_is_decode<M: Mapper>(p: &Probe<M>) {
        let o = p.serve(text(200, "<!DOCTYPE html><html><head><title>Just a moment...</title></head></html>")).await;
        expect(p, &o, ErrorKind::Decode, "HTML page");
    }
}

/// One `#[tokio::test]` per named case, each against a fresh probe. A case can carry its own
/// attributes, so a bug it reveals is quarantined alone:
///
/// ```text
/// history_error_matrix! { probe();
///     bad_request_is_invalid,
///     #[ignore = "bug: a 503 with a JSON body reads as an unknown symbol"]
///     gateway_json_outage_is_transient,
/// }
/// ```
#[macro_export]
macro_rules! history_error_matrix {
    ($probe:expr; $( $(#[$m:meta])* $case:ident ),* $(,)?) => {
        $(
            $(#[$m])*
            #[tokio::test]
            async fn $case() {
                $crate::matrix::cases::$case(&$probe).await
            }
        )*
    };
}
