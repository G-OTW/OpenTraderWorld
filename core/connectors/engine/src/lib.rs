//! Everything that is the same for every venue, written once: the planner that turns a
//! request plus declared capabilities into steps, and the stages those steps run (candle
//! folding, validation, rate limiting).
//!
//! The engine never imports a provider. It reads `Capabilities` and canonical events only.

pub mod candles;
pub mod limiter;
pub mod planner;
pub mod validate;

pub use planner::{plan, CostEstimate, Plan, Prefs, Request, Step, Unsupported};
