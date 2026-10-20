//! The instrument cache a mapper reads from: native symbol to spec, display symbol to native.
//! Built from the venue's own list, so a symbol is looked up, never reshaped by string surgery.

use std::collections::HashMap;

use otw_connector_model::{InstrumentId, InstrumentSpec};

#[derive(Debug, Clone, Default)]
pub struct InstrumentCache {
    by_native: HashMap<InstrumentId, InstrumentSpec>,
    by_symbol: HashMap<String, InstrumentId>,
}

impl InstrumentCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, spec: InstrumentSpec) {
        self.by_symbol.insert(spec.symbol.0.to_ascii_uppercase(), spec.id.clone());
        self.by_native.insert(spec.id.clone(), spec);
    }

    pub fn get(&self, id: &InstrumentId) -> Option<&InstrumentSpec> {
        self.by_native.get(id)
    }

    /// Display form (`BTC/USDT`) to the venue's id. Case-insensitive.
    pub fn by_symbol(&self, symbol: &str) -> Option<&InstrumentId> {
        self.by_symbol.get(&symbol.to_ascii_uppercase())
    }

    pub fn len(&self) -> usize {
        self.by_native.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_native.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &InstrumentSpec> {
        self.by_native.values()
    }

    /// The round-trip property every adapter must hold: for every loaded instrument, display
    /// symbol to id and back lands on the same instrument. Returns the ones that do not.
    pub fn round_trip_failures(&self) -> Vec<InstrumentId> {
        self.by_native
            .values()
            .filter(|s| self.by_symbol(&s.symbol.0) != Some(&s.id))
            .map(|s| s.id.clone())
            .collect()
    }
}
