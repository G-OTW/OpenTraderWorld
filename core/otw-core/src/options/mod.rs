//! Options: an underlying's chain with the columns derived from it, multi-leg strategies
//! read across spot, and saved strategies (`otw_store::options`).
//!
//! The inputs are the listed contracts, a price per contract and the underlying's price,
//! read from an Interactive Brokers, a Massive or an Alpaca connector granted to the module;
//! the rate and the dividend yield are the user's. Everything else is derived here.

mod alpaca;
pub mod cache;
pub mod chain;
pub mod dividends;
pub mod margin;
pub mod metrics;
pub mod pricing;
pub mod strategy;

/// Shares one equity, ETF or index option contract stands for.
pub const MULTIPLIER: f64 = 100.0;
/// Expiries one chain call reads at most.
pub const MAX_EXPIRIES: usize = 8;
/// Contracts one chain call reads at most.
pub const MAX_CONTRACTS: usize = 400;
