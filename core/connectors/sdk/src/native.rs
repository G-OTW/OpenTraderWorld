//! What a mapper asks the transport to do, and what the transport hands back.

use bytes::Bytes;
use otw_connector_model::CorrId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

/// One socket of a connector. A venue that splits markets over several sockets uses several.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnKey(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Stable name of the endpoint (`klines`), used for rate limits, metrics and decode.
    pub endpoint_id: &'static str,
    pub method: Method,
    /// Which base URL this goes to (`rest`, `fapi`). The driver resolves it, which is what
    /// lets a test point the whole adapter at a mock server.
    pub base: &'static str,
    /// Path below the base, starting with `/`.
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
    /// Needs the account's signature. The mapper never sees the secret.
    pub signed: bool,
    /// Rate-limit weight the venue charges for this call.
    pub weight: u32,
    pub corr_id: CorrId,
}

impl HttpRequest {
    pub fn get(endpoint_id: &'static str, base: &'static str, path: impl Into<String>, corr_id: CorrId) -> Self {
        HttpRequest {
            endpoint_id,
            method: Method::Get,
            base,
            path: path.into(),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
            signed: false,
            weight: 1,
            corr_id,
        }
    }
    pub fn query(mut self, k: &str, v: impl ToString) -> Self {
        self.query.push((k.to_string(), v.to_string()));
        self
    }
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
    pub fn signed(mut self) -> Self {
        self.signed = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeRequest {
    Http(HttpRequest),
    /// Open a socket on `base` (resolved like an HTTP base, so a test can redirect it) +
    /// `path`. Query parameters, if any, are part of `path`.
    WsOpen { conn_key: ConnKey, base: &'static str, path: String },
    /// Open a socket at an address the venue handed out for this connection (KuCoin's
    /// bullet: a host and a one-time token, answered by [`crate::Mapper::socket_bootstrap`]).
    WsOpenUrl { conn_key: ConnKey, url: String },
    WsSend { conn_key: ConnKey, payload: Bytes, corr_id: Option<CorrId> },
    /// A frame the venue authenticates per message: the transport has the signer complete it
    /// ([`crate::Signer::sign_frame`]) before it is sent.
    WsSigned { conn_key: ConnKey, payload: Bytes, corr_id: Option<CorrId> },
    WsClose { conn_key: ConnKey },
    /// The venue wants the socket's login frame now. The transport asks the signer for it
    /// ([`crate::Signer::ws_login`]), so the mapper never writes a credential.
    WsLogin { conn_key: ConnKey },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Remote,
    Local,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawFrame {
    Http {
        corr_id: CorrId,
        endpoint_id: &'static str,
        status: u16,
        /// Lower-cased header names.
        headers: Vec<(String, String)>,
        body: Bytes,
    },
    Ws { conn_key: ConnKey, payload: Bytes },
    Closed { conn_key: ConnKey, reason: CloseReason },
    Timeout { corr_id: CorrId, endpoint_id: &'static str },
}

impl RawFrame {
    pub fn header(&self, name: &str) -> Option<&str> {
        match self {
            RawFrame::Http { headers, .. } => {
                headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
            }
            _ => None,
        }
    }
}
