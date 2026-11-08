//! Errors an adapter reports. The kind decides the engine's policy (retry, stop, fail one
//! request); the message is for the user and names the fix when there is one.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Network hiccup or a 5xx: retry with backoff.
    Transient,
    /// 429/418 or a venue's own throttle code: wait `retry_after`, shrink the limiter.
    RateLimited,
    /// Credentials refused: stop the connector until the user fixes them.
    Auth,
    /// Credentials fine, this data or this call is not allowed (plan, entitlement).
    Permission,
    /// We built a request the venue rejects: a bug signal.
    InvalidRequest,
    /// The adapter does not declare this capability.
    Unsupported,
    /// Unknown symbol or empty resource.
    NotFound,
    /// The venue is down (repeated 5xx, maintenance).
    ProviderDown,
    /// A frame we cannot parse.
    Decode,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct ConnectorError {
    pub kind: ErrorKind,
    pub provider_code: Option<String>,
    pub message: String,
    /// How long the venue asked us to wait, in milliseconds.
    pub retry_after_ms: Option<u64>,
    /// Pointer to the recorded raw payload, when recording is on.
    pub raw_ref: Option<String>,
}

impl ConnectorError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        ConnectorError { kind, provider_code: None, message: message.into(), retry_after_ms: None, raw_ref: None }
    }
    pub fn decode(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Decode, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequest, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.provider_code = Some(code.into());
        self
    }
    pub fn with_retry_after_ms(mut self, ms: u64) -> Self {
        self.retry_after_ms = Some(ms);
        self
    }
    /// Whether the engine may try the same request again.
    pub fn is_retryable(&self) -> bool {
        matches!(self.kind, ErrorKind::Transient | ErrorKind::RateLimited | ErrorKind::ProviderDown)
    }
}
