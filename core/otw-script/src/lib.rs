//! Pine v5/v6 compatible indicator scripts: compile source, run it bar by bar over OHLCV.
//! No I/O: hosts (chart via WASM, backtest) hand in bars and read plots back.

pub mod ast;
pub mod check;
pub mod compile;
pub mod diag;
pub mod host;
pub mod input;
pub mod lexer;
pub mod output;
pub mod parser;
pub mod registry;
pub mod session;
pub mod stdlib;
pub mod vm;

pub use compile::Program;

use diag::{Diagnostic, RuntimeError};
use input::RunInput;
use output::RunOutput;

/// Parse, check and compile a script. `Err` holds every error (and the warnings).
pub fn compile(source: &str) -> Result<Program, Vec<Diagnostic>> {
    let script = parser::parse(source)?;
    let checked = check::check(&script);
    if checked.has_errors() {
        return Err(checked.diags);
    }
    compile::compile(&script, checked)
}

impl Program {
    /// Run over the host's bars, oldest to newest.
    pub fn run(&self, input: &RunInput) -> Result<RunOutput, RuntimeError> {
        vm::run(self, input)
    }
}
