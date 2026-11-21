//! Compile diagnostics and runtime errors. Messages are codes plus arguments; the UI translates.

use serde::{Deserialize, Serialize};

/// A source position: 1-based line and column, length, all counted in chars (not bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagCode {
    Syntax,
    Indentation,
    VersionMissing,
    VersionUnsupported,
    NotIndicator,
    /// args: [feature, phase that brings it ("0" = not planned)]
    Unsupported,
    UnknownName,
    ArgCount,
    ArgName,
    TypeMismatch,
    AssignUndeclared,
    /// `:=` on a global from inside a function (Pine refuses it). args: [name]
    AssignGlobal,
    Redeclared,
    /// A literal argument outside its allowed range. args: [parameter, maximum]
    OutOfRange,
    /// Expression or block nesting too deep. args: [limit]
    TooDeep,
    /// Warning. args: [feature, occurrences]
    SkippedVisual,
    /// Warning. args: [function name]
    SkippedUnused,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub span: Span,
    pub args: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: DiagCode, span: Span, args: Vec<String>) -> Self {
        Self { code, severity: Severity::Error, span, args }
    }
    pub fn warning(code: DiagCode, span: Span, args: Vec<String>) -> Self {
        Self { code, severity: Severity::Warning, span, args }
    }
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RtCode {
    /// args: [field]
    SyminfoUnknown,
    /// args: [input key, value]
    InputOutOfRange,
    /// args: [input key]
    InputType,
    LoopLimit,
    OpsBudget,
    Timeout,
    /// args: [offset]
    HistoryTooDeep,
    /// args: [message]
    RuntimeErrorCall,
    /// A string longer than `Limits::max_string_len`.
    StringTooLong,
    /// Too many call-site instances or state for one run.
    MemoryLimit,
    /// A bar column whose length differs from `close`. args: [column]
    BadBars,
    /// The script reads volume and the host has none (bricks, providers without volume).
    VolumeUnknown,
    /// An array index outside the array. args: [index, size]
    ArrayIndex,
    /// An array function on an `na` array.
    ArrayNa,
    /// An array function that needs an element on an empty array. args: [function]
    ArrayEmpty,
    /// A field of an `na` object. args: [field]
    ObjectNa,
    /// A table cell outside its table. args: [column, row, columns, rows]
    TableCell,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeError {
    pub code: RtCode,
    pub span: Span,
    pub bar: usize,
    pub args: Vec<String>,
}
