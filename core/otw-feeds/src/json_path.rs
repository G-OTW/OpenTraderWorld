//! Minimal dotted JSON path lookup: `data.articles` or `meta.0.title`.
//!
//! Segments separated by '.'. A numeric segment indexes into an array; any other
//! segment is an object key. Returns `None` if any step is missing.

use serde_json::Value;

pub fn get_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = match cur {
            Value::Object(map) => map.get(seg)?,
            Value::Array(arr) => {
                let idx: usize = seg.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_object_and_array() {
        let v = json!({ "data": { "articles": [{ "title": "Hi" }, { "title": "Yo" }] } });
        assert_eq!(get_path(&v, "data.articles.1.title").unwrap(), "Yo");
        assert_eq!(get_path(&v, "data.articles").unwrap().as_array().unwrap().len(), 2);
        assert!(get_path(&v, "data.missing").is_none());
    }

    #[test]
    fn empty_path_returns_root() {
        let v = json!([1, 2, 3]);
        assert_eq!(get_path(&v, "").unwrap(), &v);
    }
}
