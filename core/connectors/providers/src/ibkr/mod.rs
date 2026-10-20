//! Interactive Brokers. Two doors, two adapters:
//!   - [`flex`]: the Flex Web Service, the account's past (fills and open positions of a
//!     saved Activity Flex Query), over HTTP with a report token.
//!   - [`gateway`]: the local IB Gateway or TWS, the market (history, bid/ask, search, live),
//!     over the TWS socket API.
//!
//! Endpoint-by-endpoint mapping in PROVIDER.md.

pub mod flex;
pub mod gateway;
