//! Closed vocabularies for enum-like request fields.
//!
//! A string field whose valid values live in a SQL match or a CHECK constraint fails
//! silently: `asset_class=Stock` is a valid query that matches no row, so the caller gets a
//! 200 with nothing in it and cannot tell a typo from an empty book. [`vocab!`] turns such a
//! field into an enum that reads its values case-insensitively (trimmed), accepts the
//! synonyms listed for it, and refuses anything else with the full list:
//!
//! ```text
//! unknown asset_class "Stocks"; valid: stock, option, future, forex (aliases: equity = stock)
//! ```
//!
//! The same values are published as a JSON Schema `enum` (so the MCP catalog and
//! `mcp::validate` show them) and by `GET /api/vocab/{field}` through [`all`].
//!
//! ```ignore
//! crate::util::vocab::vocab! {
//!     /// What a journal trade is.
//!     pub JournalAssetClass("asset_class") {
//!         Stock = "stock",
//!         Forex = "forex",
//!     }
//!     aliases { "equity" => Stock, "fx" => Forex }
//! }
//! ```
//!
//! Register every vocab in [`all`] by hand: one line, `Name::entry()`.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fmt;

/// One registered vocabulary, as `GET /api/vocab` serves it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Entry {
    pub field: &'static str,
    pub values: &'static [&'static str],
    /// `(alias, canonical value)`.
    pub aliases: &'static [(&'static str, &'static str)],
}

/// Every vocabulary the API enforces. Hand-maintained: add `Name::entry()` next to each
/// [`vocab!`] use. A test keeps the field names unique.
pub fn all() -> Vec<Entry> {
    vec![
        crate::search_api::SearchScope::entry(),
        crate::journal_api::JournalAssetClass::entry(),
        crate::journal_api::TradeSide::entry(),
        crate::paper_api::PaperEvent::entry(),
        crate::portfolios::analytics::window::WindowName::entry(),
        crate::automator_api::RunStatus::entry(),
        crate::quant_api::SeasonalityMetric::entry(),
        crate::quant_api::OptionStyle::entry(),
        crate::mailbox_api::MailCategory::entry(),
    ]
}

/// Look one vocabulary up by its field name (case-insensitive).
pub fn find(field: &str) -> Option<Entry> {
    let field = field.trim();
    all().into_iter().find(|e| e.field.eq_ignore_ascii_case(field))
}

/// The refusal of a value outside the vocabulary. Its message is the whole contract: the
/// field, what was sent, and every value (and alias) that would have worked.
// Built only by `vocab!` expansions: dead until the first vocab is declared outside tests.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownValue {
    pub field: &'static str,
    pub got: String,
    pub values: &'static [&'static str],
    pub aliases: &'static [(&'static str, &'static str)],
}

impl fmt::Display for UnknownValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "unknown {} {:?}; valid: {}", self.field, self.got, self.values.join(", "))?;
        if !self.aliases.is_empty() {
            let list: Vec<String> = self.aliases.iter().map(|(a, c)| format!("{a} = {c}")).collect();
            write!(f, " (aliases: {})", list.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownValue {}

/// The JSON Schema of a vocabulary: a string `enum` of the canonical values. Aliases ride in
/// `x-aliases` (alias to canonical) so `mcp::validate` accepts them without the schema
/// advertising two spellings of one value.
#[allow(dead_code)]
pub fn schema(values: &[&str], aliases: &[(&str, &str)]) -> schemars::Schema {
    let mut v = json!({ "type": "string", "enum": values });
    if !aliases.is_empty() {
        let map: Map<String, Value> =
            aliases.iter().map(|(a, c)| (a.to_string(), Value::from(*c))).collect();
        v["x-aliases"] = Value::Object(map);
    }
    schemars::Schema::try_from(v).expect("an object is a schema")
}

/// Declare a closed vocabulary. See the module docs for the syntax; the string in
/// parentheses is the field name used in error messages and by `GET /api/vocab/{field}`.
///
/// Generates the enum (`Debug, Clone, Copy, PartialEq, Eq, Hash`) plus `FIELD`, `VALUES`,
/// `ALIASES`, `ALL`, `as_str`, `parse`, `entry`, and impls of `Display`, `FromStr`,
/// `Serialize` (canonical string), `Deserialize` (trimmed, case-insensitive, aliases) and
/// `JsonSchema` (inline string `enum`).
#[allow(unused_macros)]
macro_rules! vocab {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident ( $field:literal ) {
            $( $(#[$vmeta:meta])* $variant:ident = $value:literal ),+ $(,)?
        }
        $( aliases { $( $alias:literal => $target:ident ),* $(,)? } )?
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        #[allow(dead_code)]
        impl $name {
            /// Field name used in error messages and by `GET /api/vocab/{field}`.
            pub const FIELD: &'static str = $field;
            /// Canonical values, in declaration order.
            pub const VALUES: &'static [&'static str] = &[$($value),+];
            /// `(alias, canonical value)`.
            pub const ALIASES: &'static [(&'static str, &'static str)] =
                &[$($( ($alias, $name::$target.as_str()) ),*)?];
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The canonical string.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $value ),+
                }
            }

            /// Trimmed, case-insensitive, aliases accepted.
            pub fn parse(raw: &str) -> ::std::result::Result<Self, $crate::util::vocab::UnknownValue> {
                let s = raw.trim();
                $( if s.eq_ignore_ascii_case($value) { return Ok($name::$variant); } )+
                $($( if s.eq_ignore_ascii_case($alias) { return Ok($name::$target); } )*)?
                Err($crate::util::vocab::UnknownValue {
                    field: $field,
                    got: raw.to_string(),
                    values: Self::VALUES,
                    aliases: Self::ALIASES,
                })
            }

            /// The registry line for [`crate::util::vocab::all`].
            pub const fn entry() -> $crate::util::vocab::Entry {
                $crate::util::vocab::Entry { field: $field, values: Self::VALUES, aliases: Self::ALIASES }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::util::vocab::UnknownValue;
            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                Self::parse(s)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> ::std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> ::std::result::Result<Self, D::Error> {
                struct V;
                impl<'de> ::serde::de::Visitor<'de> for V {
                    type Value = $name;
                    fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        write!(f, "{}: one of {}", $field, $name::VALUES.join(", "))
                    }
                    fn visit_str<E: ::serde::de::Error>(self, v: &str) -> ::std::result::Result<$name, E> {
                        $name::parse(v).map_err(E::custom)
                    }
                }
                d.deserialize_str(V)
            }
        }

        impl ::schemars::JsonSchema for $name {
            fn inline_schema() -> bool {
                true
            }
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                ::std::borrow::Cow::Borrowed(stringify!($name))
            }
            fn json_schema(_: &mut ::schemars::SchemaGenerator) -> ::schemars::Schema {
                $crate::util::vocab::schema(Self::VALUES, Self::ALIASES)
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use vocab;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    vocab! {
        /// Test-only vocabulary.
        pub Color("color") {
            Red = "red",
            Green = "green",
            LightBlue = "light_blue",
        }
        aliases { "crimson" => Red, "sky" => LightBlue }
    }

    vocab! {
        Plain("plain") { One = "one", Two = "two" }
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Q {
        color: Option<Color>,
        plain: Plain,
    }

    #[test]
    fn valid_alias_and_wrong_case_are_accepted() {
        assert_eq!("green".parse::<Color>(), Ok(Color::Green));
        assert_eq!(" RED ".parse::<Color>(), Ok(Color::Red));
        assert_eq!("Crimson".parse::<Color>(), Ok(Color::Red));
        assert_eq!(serde_json::from_str::<Color>("\"SKY\"").unwrap(), Color::LightBlue);
        assert_eq!(serde_json::from_str::<Plain>("\"Two\"").unwrap(), Plain::Two);
    }

    #[test]
    fn unknown_is_refused_with_the_list() {
        let e = "Reds".parse::<Color>().unwrap_err().to_string();
        assert_eq!(
            e,
            r#"unknown color "Reds"; valid: red, green, light_blue (aliases: crimson = red, sky = light_blue)"#
        );
        let e = serde_json::from_str::<Plain>("\"three\"").unwrap_err().to_string();
        assert!(e.starts_with(r#"unknown plain "three"; valid: one, two"#), "{e}");
        assert!(!e.contains("aliases"), "{e}");
    }

    #[test]
    fn it_reads_from_a_query_string() {
        use axum::extract::Query;
        let uri: axum::http::Uri = "/x?color=Sky&plain=one".parse().unwrap();
        let Query(q) = Query::<Q>::try_from_uri(&uri).unwrap();
        assert_eq!(q.color, Some(Color::LightBlue));
        assert_eq!(q.plain, Plain::One);
        let uri: axum::http::Uri = "/x?plain=nine".parse().unwrap();
        let e = Query::<Q>::try_from_uri(&uri).err().unwrap().body_text();
        assert!(e.contains(r#"unknown plain "nine"; valid: one, two"#), "{e}");
    }

    #[test]
    fn it_serializes_and_displays_the_canonical_value() {
        assert_eq!(serde_json::to_string(&Color::LightBlue).unwrap(), "\"light_blue\"");
        assert_eq!(Color::Red.to_string(), "red");
        assert_eq!(Color::VALUES, &["red", "green", "light_blue"]);
        assert_eq!(Color::ALIASES, &[("crimson", "red"), ("sky", "light_blue")]);
        assert_eq!(Color::ALL.len(), 3);
        assert!(Plain::ALIASES.is_empty());
        assert_eq!(Color::entry().field, "color");
    }

    #[test]
    fn the_schema_has_the_enum_inline() {
        let s = crate::mcp::catalog::schema::<Q>();
        let plain = &s["properties"]["plain"];
        assert_eq!(plain["enum"], json!(["one", "two"]), "{s}");
        assert_eq!(plain["type"], "string");
        let color = &s["properties"]["color"];
        let values: Vec<&str> = color["enum"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(values, ["red", "green", "light_blue"], "{s}");
        assert_eq!(color["x-aliases"]["crimson"], "red", "{s}");
        assert!(s.get("definitions").is_none(), "inlined, never a $ref: {s}");
    }

    #[test]
    fn registered_field_names_are_unique() {
        let mut seen = HashSet::new();
        for e in all() {
            assert!(seen.insert(e.field.to_ascii_lowercase()), "{} registered twice", e.field);
            assert!(!e.values.is_empty(), "{} has no values", e.field);
            for (alias, target) in e.aliases {
                assert!(e.values.contains(target), "{}: alias {alias} targets {target}", e.field);
            }
        }
    }
}
