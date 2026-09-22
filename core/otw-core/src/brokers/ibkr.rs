//! Interactive Brokers: account history over the **Flex Web Service**.
//!
//! The TWS/IB Gateway socket the market-data connector speaks (`histdata::ibkr`) cannot
//! answer this question: `reqExecutions` returns the current session's fills and nothing
//! older, so "import my trades for March" has no socket form. The Flex Web Service does:
//! the user saves an Activity Flex Query in Account Management, and two HTTP calls turn it
//! into an XML statement: `SendRequest` asks for it, `GetStatement` collects it once IBKR
//! has generated it.
//!
//! The consequence the UI has to state: **the period belongs to the query**, not to us. A
//! Flex query carries its own date range (Last 365 Calendar Days, Year to Date, a custom
//! window) and the API takes no dates at all, so the window asked for here filters what the
//! statement returned. `window_from_broker` says so, and the modal repeats it.
//!
//! The socket connector stays the right tool for the present (positions, working orders,
//! today's fills) and is where the live half will be built; this one owns the past.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use super::{Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, Position};

const SEND: &str = "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService/SendRequest";
/// How long IBKR is given to generate the statement, and how often it is asked for.
const POLL_TRIES: usize = 12;
const POLL_DELAY_SECS: u64 = 5;

pub struct IbkrFlex;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "query_id",
        label: "Flex query id",
        placeholder: "1234567",
        kind: "text",
        required: true,
        help: "Account Management → Performance & Reports → Flex Queries. Create an \
               Activity query including Trades (level of detail: Execution) and note its id.",
    },
    ConfigField {
        name: "tz_offset",
        label: "Statement time offset (minutes)",
        placeholder: "0",
        kind: "number",
        required: false,
        help: "A Flex statement stamps its times in the query's own time zone and never \
               says which one. Set the offset of that zone so the fills land on the right \
               hour (-300 for New York in winter, 60 for Paris).",
    },
];

static CAP: BrokerCapability = BrokerCapability {
    broker: "ibkr_flex",
    label: "Interactive Brokers (Flex)",
    website: "https://www.interactivebrokers.com",
    docs_url: "https://www.ibkrguides.com/reportingreference/reportguide/activity%20flex%20query%20reference.htm",
    rate_limit: "One statement per query every few minutes; IBKR generates it on demand \
                 and refuses a second request while one is still building.",
    key_note: "A Flex Web Service token (Account Management → Settings → Flex Web Service). \
               The token only reads reports: it cannot trade, and it expires (IBKR mails a \
               reminder before it does).",
    required_secrets: &["flex_token"],
    config_fields: FIELDS,
    asset_classes: &["stock", "etf", "future", "option", "forex", "crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: true,
    history_days: 365,
    spot_only: false,
    positions: true,
    orders: false,
    holdings: true,
    quotes: false,
    testable: true,
};

#[async_trait::async_trait]
impl Broker for IbkrFlex {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let xml = statement(client, secrets).await?;
        let rows = elements(&xml, "Trade");
        let account = attr_of_first(&xml, "FlexStatement", "accountId").unwrap_or_default();
        let from = attr_of_first(&xml, "FlexStatement", "fromDate").unwrap_or_default();
        let to = attr_of_first(&xml, "FlexStatement", "toDate").unwrap_or_default();
        Ok(format!(
            "statement for {account} covering {from} → {to}, {} trade row(s)",
            rows.len()
        ))
    }

    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        let offset = tz_offset(secrets);
        let xml = statement(client, secrets).await?;
        let rows = execution_level(elements(&xml, "Trade"));
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut out = Vec::new();
        for r in rows {
            // A cancellation carries the original trade's id with a reversed quantity;
            // both sides are kept, so the position folds back to where it started.
            let qty = num(&r, "quantity").abs();
            let price = num(&r, "tradePrice");
            if qty == 0.0 {
                continue;
            }
            let category = get(&r, "assetCategory");
            let symbol = symbol_of(&r, &category);
            if !wanted.is_empty() && !wanted.contains(&symbol.to_uppercase()) {
                continue;
            }
            let Some(at) = stamp(&r, offset) else {
                return Err(anyhow!(
                    "a trade row carries no readable date ({:?}). Set the Flex query's date \
                     format to yyyyMMdd and its time format to HHmmss",
                    get(&r, "dateTime")
                ));
            };
            if at < q.from || at > q.to {
                continue;
            }
            let side = side_of(&r);
            out.push(Execution {
                id: first_of(&r, &["tradeID", "transactionID", "ibExecID"]),
                order_id: first_of(&r, &["ibOrderID", "orderID"]),
                at,
                symbol,
                side: side.into(),
                qty,
                price,
                // A commission is a cost, whatever sign the query writes it with: the
                // field is a cash flow in some Flex queries and a plain amount in others,
                // and nothing in the statement says which. The magnitude is the only part
                // that is reliable, so a rebate cannot be told apart and is not claimed.
                fee: num(&r, "ibCommission").abs(),
                fee_currency: first_of(&r, &["ibCommissionCurrency", "currency"]),
                currency: get(&r, "currency"),
                asset_class: asset_class(&category),
                multiplier: num(&r, "multiplier").max(1.0),
                venue: first_of(&r, &["exchange", "listingExchange"]),
                account: get(&r, "accountId"),
            });
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    /// The same statement as `positions`, read as a balance sheet: IBKR is the one broker
    /// here that reports a cost basis, so these lines arrive already priced.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let xml = statement(client, secrets).await?;
        let rows = elements(&xml, "OpenPosition");
        if rows.is_empty() {
            return Err(anyhow!(
                "this Flex query returns no open positions. Add the Open Positions section                  to it in Account Management"
            ));
        }
        Ok(rows
            .into_iter()
            .map(|r| {
                let qty = num(&r, "position");
                let category = get(&r, "assetCategory");
                Holding {
                    symbol: symbol_of(&r, &category),
                    name: get(&r, "description"),
                    qty: qty.abs(),
                    side: if qty < 0.0 {
                        "short".into()
                    } else {
                        "long".into()
                    },
                    asset_class: asset_class(&category),
                    avg_price: Some(num(&r, "costBasisPrice")).filter(|p| *p > 0.0),
                    currency: get(&r, "currency"),
                    venue: first_of(&r, &["listingExchange", "exchange"]),
                    account: get(&r, "accountId"),
                }
            })
            .collect())
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let xml = statement(client, secrets).await?;
        let rows = elements(&xml, "OpenPosition");
        if rows.is_empty() {
            return Err(anyhow!(
                "this Flex query returns no open positions. Add the Open Positions section \
                 to it in Account Management"
            ));
        }
        Ok(rows
            .into_iter()
            .map(|r| {
                let qty = num(&r, "position");
                let category = get(&r, "assetCategory");
                Position {
                    symbol: symbol_of(&r, &category),
                    side: if qty < 0.0 { "short".into() } else { "long".into() },
                    qty: qty.abs(),
                    avg_price: Some(num(&r, "costBasisPrice")).filter(|p| *p > 0.0),
                    currency: get(&r, "currency"),
                    asset_class: asset_class(&category),
                    multiplier: num(&r, "multiplier").max(1.0),
                    venue: first_of(&r, &["listingExchange", "exchange"]),
                    account: get(&r, "accountId"),
                }
            })
            .collect())
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(q) = config.get("query_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !q.chars().all(|c| c.is_ascii_digit()) {
                return Err(anyhow!("a Flex query id is the number IBKR shows next to the query"));
            }
        }
        if let Some(tz) = config.get("tz_offset").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let mins: i32 = tz
                .parse()
                .map_err(|_| anyhow!("the time offset is a number of minutes, e.g. -300"))?;
            if !(-840..=840).contains(&mins) {
                return Err(anyhow!("a time offset lies between -840 and 840 minutes"));
            }
        }
        Ok(())
    }
}

/// Ask for the statement and wait for IBKR to build it.
async fn statement(client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
    let token = super::require(secrets, "flex_token")?;
    let query = super::require(secrets, "query_id")?;
    let url = format!(
        "{SEND}?t={}&q={}&v=3",
        super::binance::enc(token),
        super::binance::enc(query)
    );
    let head = fetch(client, &url).await?;
    if let Some(err) = flex_error(&head) {
        return Err(anyhow!("IBKR refused the Flex request: {err}"));
    }
    let reference = text_of(&head, "ReferenceCode")
        .ok_or_else(|| anyhow!("IBKR did not return a reference code for this Flex query"))?;
    let base = text_of(&head, "Url").unwrap_or_else(|| {
        "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService/GetStatement".into()
    });
    let get_url = format!(
        "{base}?q={}&t={}&v=3",
        super::binance::enc(&reference),
        super::binance::enc(token)
    );
    for attempt in 0..POLL_TRIES {
        let body = fetch(client, &get_url).await?;
        if body.contains("<FlexQueryResponse") {
            return Ok(body);
        }
        match flex_error(&body) {
            // 1019: the statement is still being generated, which is the normal path.
            Some(e) if e.contains("1019") || e.to_lowercase().contains("in progress") => {}
            Some(e) => return Err(anyhow!("IBKR could not deliver the statement: {e}")),
            None => return Err(anyhow!("IBKR returned an answer that is not a Flex statement")),
        }
        if attempt + 1 < POLL_TRIES {
            tokio::time::sleep(std::time::Duration::from_secs(POLL_DELAY_SECS)).await;
        }
    }
    Err(anyhow!(
        "IBKR was still generating the statement after {}s. A large query can take longer, \
         try again in a minute",
        POLL_TRIES as u64 * POLL_DELAY_SECS
    ))
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<String> {
    let res = crate::rate::send("ibkr", client.get(url))
        .await
        .context("calling the IBKR Flex Web Service")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "the Flex Web Service answered {status}: {}",
            super::binance::trim_err(&body)
        ));
    }
    Ok(body)
}

/// `<Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>…` as one line.
fn flex_error(xml: &str) -> Option<String> {
    let status = text_of(xml, "Status")?;
    if status.eq_ignore_ascii_case("Success") {
        return None;
    }
    let code = text_of(xml, "ErrorCode").unwrap_or_default();
    let msg = text_of(xml, "ErrorMessage").unwrap_or_else(|| status.clone());
    Some(format!("{code} {msg}").trim().to_string())
}

// ── XML reading ──────────────────────────────────────────────────────────────

/// Every element named `name`, as its attribute map. A Flex statement is a flat list of
/// attribute-carrying elements, so this is all the shape there is to read.
fn elements(xml: &str, name: &str) -> Vec<HashMap<String, String>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == name.as_bytes() {
                    let mut map = HashMap::new();
                    for a in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                        let val = a
                            .decode_and_unescape_value(reader.decoder())
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        map.insert(key, val);
                    }
                    out.push(map);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Text content of the first `<name>` element.
fn text_of(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

fn attr_of_first(xml: &str, element: &str, attr: &str) -> Option<String> {
    elements(xml, element).first().and_then(|m| m.get(attr).cloned())
}

/// A Flex query can report the same fill at several levels of detail. Executions win; an
/// order-level row is used only when there is no execution-level one, and a closed-lot row
/// never is: counting both would double every trade.
fn execution_level(rows: Vec<HashMap<String, String>>) -> Vec<HashMap<String, String>> {
    let level = |r: &HashMap<String, String>| get(r, "levelOfDetail").to_ascii_uppercase();
    let has_exec = rows.iter().any(|r| level(r) == "EXECUTION");
    rows.into_iter()
        .filter(|r| {
            let l = level(r);
            if l == "CLOSED_LOT" || l == "LOT" {
                return false;
            }
            !has_exec || l == "EXECUTION"
        })
        .collect()
}

// ── Field reading ────────────────────────────────────────────────────────────

fn get(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default()
}

fn first_of(row: &HashMap<String, String>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| row.get(*k).map(|v| v.trim()).filter(|v| !v.is_empty()))
        .unwrap_or_default()
        .to_string()
}

fn num(row: &HashMap<String, String>, key: &str) -> f64 {
    row.get(key)
        .map(|v| v.replace(',', "").trim().to_string())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

/// IBKR's asset categories in journal vocabulary. A category nobody mapped falls back to
/// the import dictionary rather than to a silent "stock".
fn asset_class(category: &str) -> String {
    match category.to_ascii_uppercase().as_str() {
        "STK" => "stock".into(),
        "ETF" | "FUND" => "etf".into(),
        "OPT" | "FOP" | "WAR" => "option".into(),
        "FUT" | "CFD" => "future".into(),
        "CASH" => "forex".into(),
        "CRYPTO" => "crypto".into(),
        other => crate::import::parse::lookup_enum(other, crate::import::dict::ASSET_MAP)
            .unwrap_or("stock")
            .to_string(),
    }
}

/// The instrument as the journal should store it. Options are the one rewrite: IBKR pads
/// the OSI symbol with spaces (`SPY   251219C00650000`), and stripping that padding yields
/// the OCC form the rest of the app already parses. Everything else is kept verbatim,
/// a future's local symbol (`MNQU6`) included.
/// Which way a trade row went.
///
/// The sign leads, `buySell` follows. A cancellation is written `BUY (Ca.)` with a negative
/// quantity: reading the word first classes it as a buy, and the position doubles instead
/// of folding back to where it started. A query that emits unsigned quantities has no sign
/// to read, and there `buySell` is the only answer.
fn side_of(row: &HashMap<String, String>) -> &'static str {
    if num(row, "quantity") < 0.0 {
        return "sell";
    }
    match get(row, "buySell").to_ascii_uppercase().as_str() {
        s if s.starts_with("SELL") => "sell",
        _ => "buy",
    }
}

fn symbol_of(row: &HashMap<String, String>, category: &str) -> String {
    let raw = first_of(row, &["symbol", "underlyingSymbol", "description"]);
    match category.to_ascii_uppercase().as_str() {
        "OPT" | "FOP" => raw.split_whitespace().collect::<String>(),
        _ => raw.trim().to_string(),
    }
}

fn tz_offset(secrets: &HashMap<String, String>) -> UtcOffset {
    let mins: i32 = secrets
        .get("tz_offset")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    UtcOffset::from_whole_seconds(mins * 60).unwrap_or(UtcOffset::UTC)
}

/// A Flex timestamp: `20240115;103000`, `20240115;10:30:00`, `2024-01-15;10:30:00`, or a
/// bare date. The statement never names a time zone, so the account's offset is applied.
fn stamp(row: &HashMap<String, String>, offset: UtcOffset) -> Option<OffsetDateTime> {
    let raw = first_of(row, &["dateTime", "tradeDate", "reportDate", "settleDateTarget"]);
    if raw.is_empty() {
        return None;
    }
    let (date_part, time_part) = match raw.split_once([';', ' ', 'T']) {
        Some((d, t)) => (d.trim().to_string(), t.trim().to_string()),
        None => (raw.trim().to_string(), String::new()),
    };
    let digits: String = date_part.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 8 {
        return None;
    }
    let year: i32 = digits[0..4].parse().ok()?;
    let month = Month::try_from(digits[4..6].parse::<u8>().ok()?).ok()?;
    let day: u8 = digits[6..8].parse().ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;

    let t: String = time_part.chars().filter(char::is_ascii_digit).collect();
    let time = if t.len() >= 6 {
        Time::from_hms(t[0..2].parse().ok()?, t[2..4].parse().ok()?, t[4..6].parse().ok()?).ok()?
    } else if t.len() == 4 {
        Time::from_hms(t[0..2].parse().ok()?, t[2..4].parse().ok()?, 0).ok()?
    } else {
        Time::MIDNIGHT
    };
    Some(PrimitiveDateTime::new(date, time).assume_offset(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<FlexQueryResponse queryName="Trades" type="AF">
      <FlexStatements count="1">
        <FlexStatement accountId="U1234567" fromDate="20240101" toDate="20240131">
          <Trades>
            <Trade accountId="U1234567" currency="USD" assetCategory="STK" symbol="AAPL"
                   tradeID="7788" ibOrderID="99" dateTime="20240115;103000" quantity="10"
                   tradePrice="185.5" ibCommission="-1.05" ibCommissionCurrency="USD"
                   buySell="BUY" exchange="NASDAQ" multiplier="1" levelOfDetail="EXECUTION" />
            <Trade accountId="U1234567" currency="USD" assetCategory="STK" symbol="AAPL"
                   tradeID="7788" quantity="10" tradePrice="185.5" buySell="BUY"
                   levelOfDetail="ORDER" />
            <Trade accountId="U1234567" currency="USD" assetCategory="OPT"
                   symbol="SPY   251219C00650000" tradeID="7789" dateTime="20240116;0930"
                   quantity="-2" tradePrice="3.2" ibCommission="-1.30" buySell="SELL"
                   multiplier="100" levelOfDetail="EXECUTION" />
          </Trades>
        </FlexStatement>
      </FlexStatements>
    </FlexQueryResponse>"#;

    #[test]
    fn reads_trades_at_execution_level_only() {
        let rows = execution_level(elements(XML, "Trade"));
        assert_eq!(rows.len(), 2, "the order-level duplicate is dropped");
        assert_eq!(get(&rows[0], "tradeID"), "7788");
    }

    #[test]
    fn option_symbols_lose_their_osi_padding() {
        let rows = elements(XML, "Trade");
        assert_eq!(symbol_of(&rows[2], "OPT"), "SPY251219C00650000");
        assert_eq!(symbol_of(&rows[0], "STK"), "AAPL");
    }

    #[test]
    fn parses_both_time_widths_in_the_declared_zone() {
        let rows = elements(XML, "Trade");
        let utc = stamp(&rows[0], UtcOffset::UTC).unwrap();
        assert_eq!(utc.to_string(), "2024-01-15 10:30:00.0 +00:00:00");
        // HHmm without seconds, shifted by the account's own offset.
        let ny = stamp(&rows[2], UtcOffset::from_whole_seconds(-5 * 3600).unwrap()).unwrap();
        assert_eq!(ny.unix_timestamp(), 1705415400);
    }

    #[test]
    fn a_failed_request_reads_as_its_message() {
        let xml = "<FlexStatementResponse><Status>Fail</Status><ErrorCode>1012</ErrorCode>\
                   <ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>";
        assert_eq!(flex_error(xml).unwrap(), "1012 Token has expired.");
        assert!(flex_error("<FlexStatementResponse><Status>Success</Status></FlexStatementResponse>").is_none());
    }

    /// A cancellation is `BUY (Ca.)` with a negative quantity: the sign is what makes it
    /// undo the original fill instead of doubling it.
    #[test]
    fn a_cancellation_reverses_the_fill_it_names() {
        let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement>
          <Trades>
            <Trade currency="USD" assetCategory="STK" symbol="AAPL" tradeID="7788"
                   dateTime="20240115;103000" quantity="10" tradePrice="185.5"
                   buySell="BUY" levelOfDetail="EXECUTION" />
            <Trade currency="USD" assetCategory="STK" symbol="AAPL" tradeID="7788"
                   dateTime="20240115;103000" quantity="-10" tradePrice="185.5"
                   buySell="BUY (Ca.)" notes="Ca" levelOfDetail="EXECUTION" />
          </Trades>
        </FlexStatement></FlexStatements></FlexQueryResponse>"#;
        let rows = execution_level(elements(xml, "Trade"));
        assert_eq!(side_of(&rows[0]), "buy");
        assert_eq!(side_of(&rows[1]), "sell", "the cancellation must undo the buy");
    }

    /// An unsigned quantity leaves only the word to read.
    #[test]
    fn an_unsigned_sell_is_still_a_sell() {
        let xml = r#"<FlexQueryResponse><Trades>
            <Trade quantity="5" buySell="SELL" levelOfDetail="EXECUTION" />
          </Trades></FlexQueryResponse>"#;
        let rows = elements(xml, "Trade");
        assert_eq!(side_of(&rows[0]), "sell");
    }

    #[test]
    fn maps_ib_categories_to_journal_classes() {
        assert_eq!(asset_class("STK"), "stock");
        assert_eq!(asset_class("FUT"), "future");
        assert_eq!(asset_class("CASH"), "forex");
        assert_eq!(asset_class("FOP"), "option");
    }
}
