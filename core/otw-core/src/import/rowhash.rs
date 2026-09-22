//! The identity of a source row, so re-importing a file adds nothing.
//!
//! The raw cells are the identity — plus how many identical rows came before, so a book
//! holding two genuinely identical lines keeps both while a second run of the same file
//! is a no-op. An order/transaction id, when the file carries one, is used instead: the
//! same order re-exported with an extra column is still the same order.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

#[derive(Default)]
pub struct Hasher {
    seen: HashMap<String, usize>,
}

impl Hasher {
    pub fn new() -> Self {
        Self { seen: HashMap::new() }
    }

    /// Hash one row, keyed on its external id when it has one.
    pub fn row(&mut self, cells: &[String], external_id: Option<&str>) -> String {
        let base = match external_id {
            Some(id) if !id.trim().is_empty() => format!("id:{}", id.trim()),
            _ => cells.join("\u{1}"),
        };
        self.finish(base)
    }

    /// Hash a group of rows folded into one object (a position built from its fills).
    pub fn group(&mut self, keys: &[String]) -> String {
        self.finish(keys.join("\u{2}"))
    }

    fn finish(&mut self, base: String) -> String {
        let n = self.seen.entry(base.clone()).or_insert(0);
        *n += 1;
        let mut h = Sha256::new();
        h.update(format!("{base}#{n}").as_bytes());
        format!("{:x}", h.finalize())
    }
}
