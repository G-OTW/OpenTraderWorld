//! The adapter contract and the shared plumbing every provider reuses.
//!
//! A provider implements [`Mapper`]: pure translation, bytes in and canonical events out, with
//! no I/O, no clock and no secrets. The [`Driver`] owns everything else (HTTP, signing, rate
//! limiting, timeouts), so the same mapper runs against the venue, a mock server or a
//! recorded frame without changing a line.

pub mod clock;
pub mod driver;
pub mod enums;
pub mod http;
pub mod limiter;
pub mod mapper;
pub mod native;
pub mod parse;
pub mod signer;
pub mod symbols;

pub use clock::{Clock, SystemClock};
pub use driver::{Driver, DriverConfig, Observer};
pub use limiter::{Limiter, NoLimit};
pub use mapper::{MapCtx, Mapper};
pub use native::{ConnKey, HttpRequest, Method, NativeRequest, RawFrame};
pub use parse::{Dec, OptDec};
pub use signer::{OAuthRefresh, Signer, SignableRequest, TokenBody, TokenRequest};
pub use symbols::InstrumentCache;

pub use otw_connector_model as model;
