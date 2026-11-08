//! Lenient boolean query params (`1/0/yes/no/on/off/true/false`, any case).
//!
//! Defined in `otw_store::lenient_bool` because the mailbox message filter lives in the
//! store crate; re-exported here so handlers write `crate::util::lenient_bool::...`.
//!
//! ```ignore
//! #[serde(default, deserialize_with = "crate::util::lenient_bool::deserialize_lenient_bool")]
//! #[schemars(with = "bool")]
//! full: bool,
//! ```

#[allow(unused_imports)]
pub use otw_store::lenient_bool::{
    accepted_words, deserialize_lenient_bool, deserialize_lenient_opt_bool, parse_lenient_bool,
    FALSE_WORDS, TRUE_WORDS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Query;
    use serde::Deserialize;

    #[derive(Deserialize, schemars::JsonSchema, Debug)]
    struct Q {
        #[serde(default, deserialize_with = "deserialize_lenient_bool")]
        #[schemars(with = "bool")]
        flag: bool,
        #[serde(default, deserialize_with = "deserialize_lenient_opt_bool")]
        #[schemars(with = "Option<bool>")]
        opt: Option<bool>,
    }

    fn query(s: &str) -> Result<Q, String> {
        let uri: axum::http::Uri = format!("/x?{s}").parse().unwrap();
        Query::<Q>::try_from_uri(&uri).map(|Query(q)| q).map_err(|e| e.body_text())
    }

    #[test]
    fn every_spelling_is_read_from_a_query_string() {
        for (raw, want) in [
            ("true", true), ("TRUE", true), ("1", true), ("yes", true), ("On", true),
            ("false", false), ("0", false), ("No", false), ("off", false), ("%20yes%20", true),
        ] {
            assert_eq!(query(&format!("flag={raw}")).unwrap().flag, want, "{raw}");
            assert_eq!(query(&format!("opt={raw}")).unwrap().opt, Some(want), "{raw}");
        }
    }

    #[test]
    fn absent_and_empty_values() {
        let q = query("").unwrap();
        assert!(!q.flag);
        assert_eq!(q.opt, None);
        assert_eq!(query("opt=").unwrap().opt, None);
        assert!(query("flag=").is_err());
    }

    #[test]
    fn anything_else_is_refused_with_the_accepted_list() {
        let e = query("flag=maybe").unwrap_err();
        assert!(e.contains("true, false, 1, 0, yes, no, on, off"), "{e}");
        assert!(query("opt=2").is_err());
    }

    #[test]
    fn json_booleans_and_numbers_are_read_too() {
        let q: Q = serde_json::from_str(r#"{"flag": 1, "opt": false}"#).unwrap();
        assert!(q.flag);
        assert_eq!(q.opt, Some(false));
        let q: Q = serde_json::from_str(r#"{"flag": "yes", "opt": null}"#).unwrap();
        assert!(q.flag);
        assert_eq!(q.opt, None);
    }

    #[test]
    fn the_schema_still_says_boolean() {
        let s = crate::mcp::catalog::schema::<Q>();
        assert_eq!(s["properties"]["flag"]["type"], "boolean", "{s}");
        assert_eq!(s["properties"]["opt"]["type"], serde_json::json!(["boolean", "null"]), "{s}");
    }
}
