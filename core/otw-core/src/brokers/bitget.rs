//! Bitget account (read-only HMAC key): spot and USDⓈ-M futures.
//!
//! Signed REST in the OKX family: a prehash of `timestamp + METHOD + path + ?query`, HMAC
//! SHA-256'd with the secret and base64'd, plus the passphrase chosen when the key was made.
//! A key without its passphrase is refused with a message nobody guesses from its wording,
//! so the passphrase is a required secret rather than an optional extra.
//!
//! Three shapes of the API decide how this connector works:
//!   • **History stops at 90 days.** Both books say so, so `history_days` says so too: an
//!     older fill is a web download, not an API call.
//!   • **The two books page differently.** A spot fill page accepts a 90-day window; a
//!     futures one accepts **a week**, which is why a year of futures costs fifty-odd calls
//!     and a year of spot costs five. Both walk newest-first by cursor.
//!   • **A perpetual is not a spot balance.** The futures book is read through its own
//!     endpoints, carries its own venue label and reports positions with a side; the spot
//!     book has balances and no side at all.
//!
//! Only spot and **USDT-M** futures are read. Coin-M and USDC-M contracts are sized in
//! contracts rather than in the base coin, and a point value that cannot be verified is a
//! PnL off by the contract size, so they are refused by name instead of half-supported.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

const BASE: &str = "https://api.bitget.com";
/// The only futures product type this connector reads; see the module note.
const FUTURES: &str = "USDT-FUTURES";
/// Rows a fill page returns at most, on both books.
const PAGE: usize = 100;
/// Hard stop on the cursor walk, so a mis-specified window cannot loop.
const MAX_PAGES: usize = 500;
/// Widest window each book's fill endpoint accepts, in days.
const SPOT_WINDOW_DAYS: i64 = 90;
const FUTURES_WINDOW_DAYS: i64 = 7;

pub struct Bitget;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "books",
    label: "Books to read",
    placeholder: "spot,usdt-futures",
    kind: "text",
    required: false,
    help: "Which Bitget books this account reads: spot, usdt-futures, or both separated by \
           a comma. Leave empty for both. A book you do not trade is worth turning off: \
           futures fills page one week at a time, so asking for them costs calls.",
}];

static CAP: BrokerCapability = BrokerCapability {
    broker: "bitget",
    label: "Bitget",
    website: "https://www.bitget.com",
    docs_url: "https://www.bitget.com/docs/classic/catalog",
    rate_limit: "10 requests per second per account on the fill endpoints, 6000 per minute \
                 per IP overall. Spot fills page 90 days at a time, futures fills a week.",
    key_note: "An API key with **Read-only** permission, plus the passphrase you chose when \
               creating it. Do not grant Trade or Withdraw: this connector never writes. \
               Bitget serves **90 days** of fills over the API; anything older is a download \
               from the web site. Only spot and USDT-M futures are read, so a Coin-M or \
               USDC-M book stays outside this import.",
    required_secrets: &["api_key", "api_secret", "api_passphrase"],
    config_fields: FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 90,
    // The futures book is a margin book: a short is a short. A spot-only account is worth
    // unticking "this account can go short" for, which the import modal offers.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

/// Which books an account reads, as its settings say.
#[derive(Clone, Copy)]
struct Books {
    spot: bool,
    futures: bool,
}

fn books(secrets: &HashMap<String, String>) -> Result<Books> {
    let raw = secrets.get("books").map(|s| s.trim()).unwrap_or_default();
    if raw.is_empty() {
        return Ok(Books { spot: true, futures: true });
    }
    let mut out = Books { spot: false, futures: false };
    for part in raw.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "" => {}
            "spot" => out.spot = true,
            "usdt-futures" | "futures" => out.futures = true,
            other => {
                return Err(anyhow!(
                    "\"{other}\" is not a Bitget book this connector reads. Use spot, \
                     usdt-futures, or both separated by a comma."
                ))
            }
        }
    }
    if !out.spot && !out.futures {
        return Err(anyhow!("name at least one Bitget book to read: spot or usdt-futures"));
    }
    Ok(out)
}

#[async_trait::async_trait]
impl Broker for Bitget {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        books(config).map(|_| ())
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let b = books(secrets)?;
        let mut said: Vec<String> = Vec::new();
        if b.spot {
            let rows = get(client, secrets, "/api/v2/spot/account/assets", &[]).await?;
            let funded = rows
                .as_array()
                .map(|r| r.iter().filter(|a| held(a) > 0.0).count())
                .unwrap_or(0);
            said.push(format!("spot: {funded} coin(s) with a balance"));
        }
        if b.futures {
            let rows = get(
                client,
                secrets,
                "/api/v2/mix/account/accounts",
                &[("productType".into(), FUTURES.into())],
            )
            .await?;
            let equity: f64 = rows
                .as_array()
                .map(|r| r.iter().map(|a| num(a, "usdtEquity")).sum())
                .unwrap_or(0.0);
            said.push(format!("USDT-M futures: {equity:.2} USDT of equity"));
        }
        Ok(format!("key accepted. {}", said.join("; ")))
    }

    /// Every fill of the window on every book the account reads, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let b = books(secrets)?;
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut out: Vec<Execution> = Vec::new();
        if b.spot {
            out.extend(spot_fills(client, secrets, q, &wanted).await?);
        }
        if b.futures {
            out.extend(futures_fills(client, secrets, q, &wanted).await?);
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    /// Open futures positions. The spot book has balances, not positions: a coin sitting in
    /// a wallet has no side and no entry price, and it is reported as a holding instead.
    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        if !books(secrets)?.futures {
            return Err(anyhow!(
                "this Bitget account reads the spot book only, and a spot balance is a \
                 holding rather than a position. Add usdt-futures to its books to read \
                 contract positions."
            ));
        }
        let rows = get(
            client,
            secrets,
            "/api/v2/mix/position/all-position",
            &[("productType".into(), FUTURES.into())],
        )
        .await?;
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            let qty = num(p, "total");
            if qty == 0.0 {
                continue;
            }
            out.push(Position {
                symbol: str_of(p, "symbol").to_uppercase(),
                side: if str_of(p, "holdSide") == "short" { "short".into() } else { "long".into() },
                qty: qty.abs(),
                avg_price: Some(num(p, "openPriceAvg")).filter(|v| *v > 0.0),
                currency: str_of(p, "marginCoin").to_uppercase(),
                asset_class: "crypto".into(),
                // A USDT-M contract is sized in the base coin: one unit of quantity is one
                // coin, so the point value is one. Coin-M is not read here for exactly the
                // reason this line would otherwise have to guess.
                multiplier: 1.0,
                venue: "Bitget USDT-M futures".into(),
                account: String::new(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn orders(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        let b = books(secrets)?;
        let mut out = Vec::new();
        if b.spot {
            let rows = get(
                client,
                secrets,
                "/api/v2/spot/trade/unfilled-orders",
                &[("limit".into(), PAGE.to_string())],
            )
            .await?;
            for o in rows.as_array().into_iter().flatten() {
                let symbol = str_of(o, "symbol").to_uppercase();
                out.push(OpenOrder {
                    id: str_of(o, "orderId"),
                    side: str_of(o, "side").to_ascii_lowercase(),
                    order_type: str_of(o, "orderType"),
                    qty: num(o, "size"),
                    filled_qty: num(o, "baseVolume"),
                    limit_price: Some(num(o, "priceAvg")).filter(|v| *v > 0.0),
                    stop_price: Some(num(o, "triggerPrice")).filter(|v| *v > 0.0),
                    currency: quote_of(&symbol),
                    asset_class: "crypto".into(),
                    placed_at: ms_stamp(o, "cTime"),
                    venue: "Bitget".into(),
                    account: String::new(),
                    symbol,
                });
            }
        }
        if b.futures {
            let body = get(
                client,
                secrets,
                "/api/v2/mix/order/orders-pending",
                &[
                    ("productType".into(), FUTURES.into()),
                    ("limit".into(), PAGE.to_string()),
                ],
            )
            .await?;
            for o in body.get("entrustedList").and_then(Value::as_array).into_iter().flatten() {
                let symbol = str_of(o, "symbol").to_uppercase();
                out.push(OpenOrder {
                    id: str_of(o, "orderId"),
                    side: str_of(o, "side").to_ascii_lowercase(),
                    order_type: str_of(o, "orderType"),
                    qty: num(o, "size"),
                    filled_qty: num(o, "baseVolume"),
                    limit_price: Some(num(o, "price")).filter(|v| *v > 0.0),
                    stop_price: Some(num(o, "presetStopLossPrice")).filter(|v| *v > 0.0),
                    currency: str_of(o, "marginCoin").to_uppercase(),
                    asset_class: "crypto".into(),
                    placed_at: ms_stamp(o, "cTime"),
                    venue: "Bitget USDT-M futures".into(),
                    account: String::new(),
                    symbol,
                });
            }
        }
        Ok(out)
    }

    /// Spot balances: available plus what an order or a lock is holding, so the line is what
    /// the account owns rather than what it could spend this second. A futures position is
    /// not a holding and is reported by `positions`.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        if !books(secrets)?.spot {
            return Err(anyhow!(
                "this Bitget account reads the futures book only, which holds margin rather \
                 than assets. Add spot to its books to read balances."
            ));
        }
        let rows = get(
            client,
            secrets,
            "/api/v2/spot/account/assets",
            &[("assetType".into(), "hold_only".into())],
        )
        .await?;
        let mut out = Vec::new();
        for a in rows.as_array().into_iter().flatten() {
            let qty = held(a);
            if qty <= 0.0 {
                continue;
            }
            out.push(Holding {
                symbol: str_of(a, "coin").to_uppercase(),
                name: String::new(),
                qty,
                side: "long".into(),
                asset_class: "crypto".into(),
                // A balance carries no cost basis: Bitget reports what is there, not what it
                // was paid for.
                avg_price: None,
                currency: String::new(),
                venue: "Bitget".into(),
                account: String::new(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What the exchange trades these coins at right now. The whole spot board is read in
    /// one public call: which pair prices a coin is exactly what is being looked for, and
    /// naming one Bitget does not list fails the call outright.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let board = ticker_board(client).await?;
        let mut out = Vec::new();
        for s in symbols {
            let asset = s.trim().to_uppercase();
            if asset.is_empty() {
                continue;
            }
            for q in QUOTE_COINS {
                if *q == asset {
                    continue;
                }
                let direct = format!("{asset}{q}");
                if let Some(p) = board.get(&direct).copied().filter(|p| *p > 0.0) {
                    out.push(Quote { symbol: s.clone(), price: p, currency: (*q).into(), pair: direct });
                    break;
                }
                // The other way round: USDT has no USDT market, but USDCUSDT prices it.
                let reverse = format!("{q}{asset}");
                if let Some(p) = board.get(&reverse).copied().filter(|p| *p > 0.0) {
                    out.push(Quote {
                        symbol: s.clone(),
                        price: 1.0 / p,
                        currency: (*q).into(),
                        pair: reverse,
                    });
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Pairs worth offering: whatever has a working order, plus the coins the spot wallet
    /// holds against the usual quotes. A suggestion, not a filter.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .orders(client, secrets)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|o| o.symbol)
            .collect();
        if books(secrets)?.spot {
            let board = ticker_board(client).await.unwrap_or_default();
            for h in self.holdings(client, secrets).await.unwrap_or_default() {
                for q in QUOTE_COINS {
                    let pair = format!("{}{q}", h.symbol);
                    if board.contains_key(&pair) {
                        out.push(pair);
                        break;
                    }
                }
            }
        }
        out.retain(|s| !s.is_empty());
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// Quote coins a holding is priced against, most dollar-like first.
const QUOTE_COINS: &[&str] = &["USDT", "USDC", "EUR", "BTC"];

// ── Fills ────────────────────────────────────────────────────────────────────

/// Spot fills over the window. The endpoint answers newest first and pages backwards on
/// `idLessThan`, so each 90-day slice is walked down to its own start.
async fn spot_fills(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    q: &ExecQuery,
    wanted: &[String],
) -> Result<Vec<Execution>> {
    let mut out = Vec::new();
    let mut pages_left = MAX_PAGES;
    let mut slice_to = q.to;
    while slice_to > q.from {
        let slice_from = (slice_to - time::Duration::days(SPOT_WINDOW_DAYS)).max(q.from);
        let mut cursor: Option<String> = None;
        loop {
            if pages_left == 0 {
                return Err(anyhow!(
                    "this period holds more Bitget spot fills than one pull reads. Ask for a \
                     shorter period."
                ));
            }
            pages_left -= 1;
            let mut params = vec![
                ("startTime".into(), ms(slice_from).to_string()),
                ("endTime".into(), ms(slice_to).to_string()),
                ("limit".into(), PAGE.to_string()),
            ];
            if let Some(c) = &cursor {
                params.push(("idLessThan".into(), c.clone()));
            }
            let rows = get(client, secrets, "/api/v2/spot/trade/fills", &params).await?;
            let rows = rows.as_array().cloned().unwrap_or_default();
            for r in &rows {
                let symbol = str_of(r, "symbol").to_uppercase();
                if symbol.is_empty() || (!wanted.is_empty() && !wanted.contains(&symbol)) {
                    continue;
                }
                out.push(spot_fill(r, &symbol)?);
            }
            if rows.len() < PAGE {
                break;
            }
            cursor = rows.last().map(|r| str_of(r, "tradeId")).filter(|s| !s.is_empty());
            if cursor.is_none() {
                break;
            }
        }
        slice_to = slice_from;
    }
    Ok(out)
}

fn spot_fill(r: &Value, symbol: &str) -> Result<Execution> {
    let fee = r.get("feeDetail").unwrap_or(&Value::Null);
    Ok(Execution {
        id: str_of(r, "tradeId"),
        order_id: str_of(r, "orderId"),
        at: stamp(r, "cTime")?,
        symbol: symbol.to_string(),
        side: str_of(r, "side").to_ascii_lowercase(),
        qty: num(r, "size"),
        price: num(r, "priceAvg"),
        // Bitget signs a charge negative; a cost is filed positive.
        fee: num(fee, "totalFee").abs(),
        fee_currency: str_of(fee, "feeCoin").to_uppercase(),
        currency: quote_of(symbol),
        asset_class: "crypto".into(),
        multiplier: 1.0,
        venue: "Bitget".into(),
        account: String::new(),
    })
}

/// USDT-M futures fills. The endpoint accepts **a week** at a time and answers newest first
/// under `data.fillList`, paging backwards on the id it hands back.
async fn futures_fills(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    q: &ExecQuery,
    wanted: &[String],
) -> Result<Vec<Execution>> {
    let mut out = Vec::new();
    let mut pages_left = MAX_PAGES;
    let mut slice_to = q.to;
    while slice_to > q.from {
        let slice_from = (slice_to - time::Duration::days(FUTURES_WINDOW_DAYS)).max(q.from);
        let mut cursor: Option<String> = None;
        loop {
            if pages_left == 0 {
                return Err(anyhow!(
                    "this period holds more Bitget futures fills than one pull reads. Bitget \
                     answers futures fills a week at a time, so ask for a shorter period."
                ));
            }
            pages_left -= 1;
            let mut params = vec![
                ("productType".into(), FUTURES.to_string()),
                ("startTime".into(), ms(slice_from).to_string()),
                ("endTime".into(), ms(slice_to).to_string()),
                ("limit".into(), PAGE.to_string()),
            ];
            if let Some(c) = &cursor {
                params.push(("idLessThan".into(), c.clone()));
            }
            let body = get(client, secrets, "/api/v2/mix/order/fill-history", &params).await?;
            let rows = body
                .get("fillList")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for r in &rows {
                let symbol = str_of(r, "symbol").to_uppercase();
                if symbol.is_empty() || (!wanted.is_empty() && !wanted.contains(&symbol)) {
                    continue;
                }
                out.push(futures_fill(r, &symbol)?);
            }
            if rows.len() < PAGE {
                break;
            }
            cursor = Some(str_of(&body, "endId")).filter(|s| !s.is_empty());
            if cursor.is_none() {
                break;
            }
        }
        slice_to = slice_from;
    }
    Ok(out)
}

fn futures_fill(r: &Value, symbol: &str) -> Result<Execution> {
    // The futures book reports fees as a list, one line per coin they were charged in.
    let fees = r.get("feeDetail").and_then(Value::as_array).cloned().unwrap_or_default();
    let fee: f64 = fees.iter().map(|f| num(f, "totalFee").abs()).sum();
    let fee_currency = fees
        .first()
        .map(|f| str_of(f, "feeCoin").to_uppercase())
        .unwrap_or_default();
    Ok(Execution {
        id: str_of(r, "tradeId"),
        order_id: str_of(r, "orderId"),
        at: stamp(r, "cTime")?,
        symbol: symbol.to_string(),
        side: str_of(r, "side").to_ascii_lowercase(),
        qty: num(r, "baseVolume"),
        price: num(r, "price"),
        fee,
        fee_currency,
        currency: str_of(r, "marginCoin").to_uppercase(),
        asset_class: "crypto".into(),
        multiplier: 1.0,
        venue: "Bitget USDT-M futures".into(),
        account: String::new(),
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Everything a spot line holds: free, frozen by an order, and locked.
fn held(a: &Value) -> f64 {
    num(a, "available") + num(a, "frozen") + num(a, "locked")
}

/// The quote coin of a pair, from the longest known suffix it ends in. Bitget concatenates
/// base and quote with nothing between them, so the split is the exchange's own list of
/// quote coins rather than a fixed number of characters.
fn quote_of(symbol: &str) -> String {
    QUOTE_COINS
        .iter()
        .find(|q| symbol.len() > q.len() && symbol.ends_with(*q))
        .map(|q| (*q).to_string())
        .unwrap_or_default()
}

/// Every listed spot pair and its last price, in one public call.
async fn ticker_board(client: &reqwest::Client) -> Result<HashMap<String, f64>> {
    let body: Value = crate::rate::send(
        "bitget",
        client.get(format!("{}/api/v2/spot/market/tickers", super::at(BASE))),
    )
    .await
    .context("asking Bitget for its prices")?
    .json()
    .await
    .context("reading Bitget's prices")?;
    let mut out = HashMap::new();
    for row in body.get("data").and_then(Value::as_array).into_iter().flatten() {
        let sym = str_of(row, "symbol").to_uppercase();
        if !sym.is_empty() {
            out.insert(sym, num(row, "lastPr"));
        }
    }
    Ok(out)
}

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

fn stamp(v: &Value, key: &str) -> Result<OffsetDateTime> {
    let raw = str_of(v, key);
    let at: i64 = raw
        .parse()
        .map_err(|_| anyhow!("Bitget reported a timestamp that cannot be read: {raw}"))?;
    OffsetDateTime::from_unix_timestamp_nanos(at as i128 * 1_000_000)
        .map_err(|_| anyhow!("Bitget reported an impossible timestamp: {at}"))
}

fn ms_stamp(v: &Value, key: &str) -> Option<OffsetDateTime> {
    stamp(v, key).ok()
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

/// The string Bitget signs: its own worked example is
/// `<ms>GET/api/mix/v2/market/depth?limit=20&symbol=BTCUSDT`.
pub(super) fn prehash(ts: i64, method: &str, path: &str, query: &str) -> String {
    if query.is_empty() {
        format!("{ts}{method}{path}")
    } else {
        format!("{ts}{method}{path}?{query}")
    }
}

/// A signed GET: the prehash, its base64 HMAC, and the three key headers.
async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    use base64::Engine as _;

    let key = super::require(secrets, "api_key")?;
    let secret = super::require(secrets, "api_secret")?;
    let pass = super::require(secrets, "api_passphrase")?;
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect::<Vec<_>>()
        .join("&");
    let ts = ms(OffsetDateTime::now_utc());
    let sign = base64::engine::general_purpose::STANDARD.encode(super::binance::hmac_sha256(
        secret.as_bytes(),
        prehash(ts, "GET", path, &query).as_bytes(),
    ));
    let url = if query.is_empty() {
        format!("{}{path}", super::at(BASE))
    } else {
        format!("{}{path}?{query}", super::at(BASE))
    };
    let res = crate::rate::send(
        "bitget",
        client
            .get(&url)
            .header("ACCESS-KEY", key)
            .header("ACCESS-SIGN", sign)
            .header("ACCESS-TIMESTAMP", ts.to_string())
            .header("ACCESS-PASSPHRASE", pass)
            .header("locale", "en-US")
            .header("Content-Type", "application/json"),
    )
    .await
    .with_context(|| format!("calling Bitget {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Bitget {path} answered {status}: {}",
            super::binance::trim_err(&body)
        ));
    }
    let v: Value = serde_json::from_str(&body)
        .with_context(|| format!("reading Bitget's {path} answer"))?;
    // Bitget answers 200 with a code in the envelope, so success is the code, not the status.
    let code = str_of(&v, "code");
    if code != "00000" {
        let msg = if v.get("msg").is_some() { str_of(&v, "msg") } else { str_of(&v, "message") };
        return Err(match code.as_str() {
            "40037" | "40006" | "40009" | "40012" => anyhow!(
                "Bitget refused the key ({code} {msg}). Check the key, its secret and the \
                 passphrase chosen when it was created, and that the key is not IP-restricted \
                 to another machine."
            ),
            "40014" => anyhow!(
                "Bitget says this key lacks the permission for {path} ({msg}). A read-only \
                 key is enough, but it must have the right books enabled."
            ),
            _ => anyhow!("Bitget refused {path}: {code} {msg}"),
        });
    }
    Ok(v.get("data").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signed_string_is_the_one_the_docs_show() {
        // Bitget's own worked example, from the REST authentication page.
        assert_eq!(
            prehash(16273667805456, "GET", "/api/mix/v2/market/depth", "limit=20&symbol=BTCUSDT"),
            "16273667805456GET/api/mix/v2/market/depth?limit=20&symbol=BTCUSDT"
        );
        // No query means no question mark: a stray one signs a different string than it sends.
        assert_eq!(
            prehash(1, "GET", "/api/v2/spot/account/assets", ""),
            "1GET/api/v2/spot/account/assets"
        );
    }

    #[test]
    fn a_spot_fill_files_its_fee_as_a_cost() {
        let r = serde_json::json!({
            "symbol": "BTCUSDT", "orderId": "12345", "tradeId": "67890", "side": "buy",
            "priceAvg": "13000", "size": "0.0007", "cTime": "1695865232579",
            "feeDetail": {"deduction": "no", "feeCoin": "BTC", "totalFee": "-0.0000007"}
        });
        let e = spot_fill(&r, "BTCUSDT").unwrap();
        assert_eq!(e.side, "buy");
        assert_eq!(e.fee, 0.0000007);
        assert_eq!(e.fee_currency, "BTC");
        assert_eq!(e.currency, "USDT");
        assert_eq!(e.at.unix_timestamp_nanos(), 1_695_865_232_579_000_000);
    }

    #[test]
    fn a_futures_fill_adds_up_every_fee_line_and_keeps_its_venue() {
        let r = serde_json::json!({
            "tradeId": "x1", "symbol": "ETHUSDT", "marginCoin": "USDT", "orderId": "o1",
            "price": "1801.33", "baseVolume": "0.02", "side": "sell", "cTime": "1698730804882",
            "feeDetail": [{"feeCoin": "USDT", "totalFee": "-0.02161596"},
                          {"feeCoin": "USDT", "totalFee": "-0.001"}]
        });
        let e = futures_fill(&r, "ETHUSDT").unwrap();
        assert!((e.fee - 0.02261596).abs() < 1e-9);
        assert_eq!(e.currency, "USDT");
        assert_eq!(e.venue, "Bitget USDT-M futures");
        assert_eq!(e.qty, 0.02);
    }

    #[test]
    fn the_books_setting_is_checked_rather_than_guessed() {
        let all = HashMap::new();
        let b = books(&all).unwrap();
        assert!(b.spot && b.futures);
        let spot = HashMap::from([("books".to_string(), " spot ".to_string())]);
        let b = books(&spot).unwrap();
        assert!(b.spot && !b.futures);
        // Coin-M is not read here, so asking for it is refused by name rather than ignored.
        let coinm = HashMap::from([("books".to_string(), "coin-futures".to_string())]);
        assert!(books(&coinm).is_err());
    }

    #[test]
    fn a_pair_names_its_own_quote_coin() {
        assert_eq!(quote_of("BTCUSDT"), "USDT");
        assert_eq!(quote_of("ETHBTC"), "BTC");
        // A coin whose name *is* a quote coin is not split into nothing.
        assert_eq!(quote_of("USDT"), "");
    }
}
