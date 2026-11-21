//! otw-core's import modules, compiled from their own sources: otw-core is a binary, so
//! there is no library to depend on. Only `parquet` is stood in for (see below).

#![allow(dead_code)]

#[path = "../../../otw-core/src/import/detect.rs"]
pub mod detect;
#[path = "../../../otw-core/src/import/dict.rs"]
pub mod dict;
#[path = "../../../otw-core/src/import/parse.rs"]
pub mod parse;

/// Parquet has its own binary reader (and pulls the histdata module in): only the text path
/// is fuzzed here.
pub mod parquet {
    pub fn looks_parquet(_filename: &str, _bytes: &[u8]) -> bool {
        false
    }
    pub fn read_grid(_bytes: &[u8]) -> anyhow::Result<super::parse::Grid> {
        anyhow::bail!("parquet is not fuzzed by this target")
    }
}
