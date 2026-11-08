//! CSV export of assets and their revisions.

use otw_store::wealth::{Asset, Revision};

/// One CSV field. Quoted when it holds a separator, a quote or a line break; a text cell
/// that a spreadsheet would run as a formula (`=`, `+`, `-`, `@`, tab) is prefixed with a
/// quote mark so opening the file never executes it.
pub fn field(s: &str, text: bool) -> String {
    let guarded = if text && s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn num(v: Option<f64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

pub const HEADER: &str =
    "asset_id,asset,type,category,owner,currency,side,valued_at,value,price,quantity,source,note";

/// Every asset, one line per revision, oldest first; an asset never valued gets one line
/// with the revision columns empty, so the file lists the whole book.
pub fn csv(assets: &[Asset], revisions: &[Revision]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for a in assets {
        let lead = [
            a.id.to_string(),
            field(&a.name, true),
            field(&a.asset_type, true),
            field(a.category.as_deref().unwrap_or(""), true),
            field(a.owner.as_deref().unwrap_or(""), true),
            a.currency.clone(),
            if a.sign < 0 { "owed".into() } else { "owned".into() },
        ]
        .join(",");
        let mut any = false;
        for r in revisions.iter().filter(|r| r.asset_id == a.id) {
            any = true;
            let source = r.fields.get("source").and_then(|v| v.as_str()).unwrap_or("manual");
            let line = [
                lead.clone(),
                r.valued_at.to_string(),
                r.value.to_string(),
                num(r.price),
                num(r.quantity),
                field(source, true),
                field(r.note.as_deref().unwrap_or(""), true),
            ]
            .join(",");
            out.push_str(&line);
            out.push('\n');
        }
        if !any {
            out.push_str(&lead);
            out.push_str(",,,,,,\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escaping() {
        assert_eq!(field("plain", true), "plain");
        assert_eq!(field("a,b", true), "\"a,b\"");
        assert_eq!(field("say \"hi\"", true), "\"say \"\"hi\"\"\"");
        assert_eq!(field("two\nlines", true), "\"two\nlines\"");
        // A formula is defused, then quoted if it also needs quoting.
        assert_eq!(field("=SUM(A1)", true), "'=SUM(A1)");
        assert_eq!(field("=1,2", true), "\"'=1,2\"");
        // Numbers are not text: a negative value stays a number.
        assert_eq!(field("-12.5", false), "-12.5");
    }

    #[test]
    fn every_asset_appears_even_unvalued() {
        let a = Asset {
            id: uuid::Uuid::nil(),
            template_id: None,
            name: "Flat, Paris".into(),
            asset_type: "house".into(),
            currency: "EUR".into(),
            category: None,
            sign: 1,
            portfolio_id: None,
            review_days: 0,
            loan: None,
            paired_liability_id: None,
            drift_pct_year: None,
            broker_account_id: None,
            chain: None,
            address: None,
            owner: Some("Ana".into()),
            latest_value: None,
            latest_at: None,
            revision_count: 0,
        };
        let out = csv(&[a], &[]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].split(',').count(), HEADER.split(',').count() + 1); // the quoted comma
        assert!(lines[1].contains("\"Flat, Paris\""));
    }
}
