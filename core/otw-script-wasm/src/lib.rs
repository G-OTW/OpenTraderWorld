//! Browser build of the script engine: a thin wasm-bindgen layer over `otw_script::host`, so
//! the chart computes exactly what a native host computes (same glue, same `libm` math).

use otw_script::host::{self, BarCols};
use otw_script::input::Limits;
use wasm_bindgen::prelude::*;

/// Wasm has no clock: the instruction budget is the only stop.
fn limits() -> Limits {
    Limits { timeout_ms: None, ..Limits::default() }
}

/// Compile JSON (`otw_script::host::compile_json`).
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    host::compile_json(source)
}

/// Every builtin as a JSON array of `BuiltinDoc`, for the editor.
#[wasm_bindgen]
pub fn registry() -> String {
    serde_json_string(&otw_script::registry::docs())
}

fn serde_json_string<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

/// A compiled script, kept by the worker across runs.
#[wasm_bindgen]
pub struct Script {
    program: otw_script::Program,
}

#[wasm_bindgen]
impl Script {
    /// Throws the compile JSON (`{"ok":false,"diagnostics":[…]}`) when the source has errors.
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<Script, String> {
        otw_script::compile(source).map(|program| Script { program }).map_err(|_| host::compile_json(source))
    }

    /// Run JSON (`otw_script::host::run_json`). `volume` empty = the chart has none.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&self, time: &[f64], open: &[f64], high: &[f64], low: &[f64], close: &[f64], volume: &[f64], ctx: &str) -> String {
        host::run_json(&self.program, BarCols { time, open, high, low, close, volume }, ctx, limits())
    }
}

/// A live script (`otw_script::host::HostSession`), kept by the worker per chart instance:
/// opened once, then advanced with the bars from the last committed one on each update.
#[wasm_bindgen]
pub struct Session {
    inner: host::HostSession,
}

#[wasm_bindgen]
impl Session {
    /// Throws the answer to show (compile diagnostics or a run refusal) as a JSON string.
    #[wasm_bindgen(constructor)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(source: &str, time: &[f64], open: &[f64], high: &[f64], low: &[f64], close: &[f64], volume: &[f64], ctx: &str, forming: bool) -> Result<Session, String> {
        host::HostSession::open(source, BarCols { time, open, high, low, close, volume }, ctx, forming, limits()).map(|inner| Session { inner })
    }

    /// Output JSON with per-bar columns from bar `from` on.
    pub fn output(&self, from: usize) -> String {
        self.inner.output_json(from)
    }

    /// Run the bars from `committed()` on; the last as forming when `forming`.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(&mut self, time: &[f64], open: &[f64], high: &[f64], low: &[f64], close: &[f64], volume: &[f64], forming: bool) -> String {
        self.inner.advance(BarCols { time, open, high, low, close, volume }, forming)
    }

    pub fn committed(&self) -> usize {
        self.inner.committed()
    }
}
