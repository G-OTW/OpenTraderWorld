//! Reading somebody else's export, whatever module it ends up in.
//!
//! Every import in the app faces the same three problems before the domain even starts:
//! the file is a grid in an unknown dialect (`parse`), its columns are named in an
//! unknown language (`dict`), and which column is which has to be *proposed* rather than
//! demanded (`detect`). None of that is journal- or portfolio-specific, so it lives here
//! and each module brings only its own target set and its own row → object builder.
//!
//! What stays in the module: the mapping document, the builder, and the writes.

pub mod detect;
pub mod dict;
pub mod parse;
pub mod rowhash;
