//! Native enum values we have no name for. Each mapper maps its enums with an exhaustive
//! `match` and sends the rest here: the value is kept, and the miss is logged once per call.

use otw_connector_model::Mapped;

pub fn unknown<T>(provider: &str, what: &str, raw: &str) -> Mapped<T> {
    tracing::warn!(provider, what, raw, "unknown native enum value");
    Mapped::Unknown(raw.to_string())
}
