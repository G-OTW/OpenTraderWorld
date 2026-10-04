//! Binance USDⓈ-M futures account (read-only key).
//!
//! The same signature as the spot connector (HMAC of the query string, the exchange's own
//! clock) against a different service, `fapi`, and a different book. What differs is
//! everything the API's shape imposes:
//!
//!   • **`userTrades` is per symbol**, so the capability declares `needs_symbols`: this
//!     account cannot be asked "what did I trade in May".
//!   • **A window is a week and the archive is three months.** `startTime`/`endTime` may
//!     not span more than seven days and `fromId` may not be combined with either, so a
//!     period is walked week by week and each week is paged forward on the clock.
//!   • **A position has a side of its own.** Hedge mode reports `positionSide` LONG/SHORT
//!     on both the fill and the position, and one-way mode puts the sign on the amount;
//!     both are read rather than one being assumed.
//!
//! Only USDⓈ-M contracts are read. Coin-M futures are inverse, sized in contracts of quote
//! currency on a different host, and a point value that cannot be verified is a PnL off by
//! the contract size, so they are left out by name rather than half-supported.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

const LIVE: &str = "https://fapi.binance.com";
const TESTNET: &str = "https://testnet.binancefuture.com";
/// Rows one `userTrades` page returns at most.
const PAGE: usize = 1000;
/// Widest window `userTrades` accepts, in days.
const WINDOW_DAYS: i64 = 7;
/// Hard stop on the page walk, so a mis-specified window cannot loop.
const MAX_PAGES: usize = 500;

pub struct BinanceFutures;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or testnet. Binance's futures testnet is a different host with its own key, \
           so this is stated rather than guessed. Leave empty for live.",
}];

static CAP: BrokerCapability = BrokerCapability {
    broker: "binance_futures",
    label: "Binance USDⓈ-M Futures",
    website: "https://www.binance.com",
    docs_url: "https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Account-Trade-List",
    rate_limit: "2400 request weight per minute per IP; a trade page costs 5. Fills are \
                 asked one instrument and one week at a time.",
    key_note: "An API key with **Enable Reading** and futures access, and nothing else. Do \
               not enable trading or withdrawals: this connector never writes. Binance \
               serves **90 days** of futures fills; anything older is a download from the \
               web site. Coin-M (inverse) contracts are not read here.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    // `userTrades` answers one symbol at a time.
    needs_symbols: true,
    window_from_broker: false,
    history_days: 90,
    // A futures book is a margin book: a short is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

#[async_trait::async_trait]
impl Broker for BinanceFutures {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "testnet" => Ok(()),
            Some(other) => Err(anyhow!(
                "the Binance futures environment is \"live\" or \"testnet\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let acct = signed_get(client, secrets, "/fapi/v3/account", &[]).await?;
        let open = acct
            .get("positions")
            .and_then(Value::as_array)
            .map(|p| p.iter().filter(|x| num(x, "positionAmt") != 0.0).count())
            .unwrap_or(0);
        let trade = acct.get("canTrade").and_then(Value::as_bool).unwrap_or(false);
        Ok(format!(
            "{} futures account, wallet balance {}, {open} open position(s){}",
            env_label(secrets),
            str_of(&acct, "totalWalletBalance"),
            if trade { ". The key can trade; a read-only key is enough here" } else { "" }
        ))
    }

    /// Every fill of the window for the named instruments, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.symbols.is_empty() {
            return Err(anyhow!(
                "Binance answers its futures trade history one instrument at a time. Name \
                 the symbols to pull (BTCUSDT, ETHUSDT…)"
            ));
        }
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let mut out: Vec<Execution> = Vec::new();
        let mut pages_left = MAX_PAGES;
        for symbol in &q.symbols {
            let symbol = symbol.trim().to_uppercase();
            if symbol.is_empty() {
                continue;
            }
            let end_ms = ms(q.to);
            let mut window_from = ms(q.from);
            while window_from < end_ms {
                let window_to = (window_from + WINDOW_DAYS * 86_400_000).min(end_ms);
                // Inside a week, the walk moves forward on the clock: `fromId` cannot be
                // combined with a time range, so the last fill's stamp opens the next page.
                let mut cursor = window_from;
                loop {
                    if pages_left == 0 {
                        return Err(anyhow!(
                            "this period holds more Binance futures fills than one pull \
                             reads. Ask for a shorter period."
                        ));
                    }
                    pages_left -= 1;
                    let rows = signed_get(
                        client,
                        secrets,
                        "/fapi/v1/userTrades",
                        &[
                            ("symbol".into(), symbol.clone()),
                            ("startTime".into(), cursor.to_string()),
                            ("endTime".into(), window_to.to_string()),
                            ("limit".into(), PAGE.to_string()),
                        ],
                    )
                    .await?;
                    let rows = rows.as_array().cloned().unwrap_or_default();
                    let mut last_ms = cursor;
                    for r in &rows {
                        let at_ms = r.get("time").and_then(Value::as_i64).unwrap_or(0);
                        last_ms = last_ms.max(at_ms);
                        out.push(fill(r, &symbol, at_ms)?);
                    }
                    if rows.len() < PAGE || last_ms >= window_to {
                        break;
                    }
                    // A full page and more to come: continue one millisecond past the last
                    // fill read. Two fills in the same millisecond would otherwise repeat,
                    // and the import deduplicates on the fill id.
                    cursor = last_ms + 1;
                }
                window_from = window_to;
            }
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let rows = signed_get(client, secrets, "/fapi/v3/positionRisk", &[]).await?;
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            let amt = num(p, "positionAmt");
            if amt == 0.0 {
                continue;
            }
            let symbol = str_of(p, "symbol").to_uppercase();
            out.push(Position {
                // Hedge mode names the side; one-way mode puts it in the sign.
                side: match str_of(p, "positionSide").as_str() {
                    "SHORT" => "short".into(),
                    "LONG" => "long".into(),
                    _ if amt < 0.0 => "short".into(),
                    _ => "long".into(),
                },
                qty: amt.abs(),
                avg_price: Some(num(p, "entryPrice")).filter(|v| *v > 0.0),
                currency: quote_of(&symbol),
                asset_class: "crypto".into(),
                // A USDⓈ-M contract is sized in the base coin, so one unit of quantity is
                // one coin. Coin-M is not read here for exactly the reason this line would
                // otherwise have to guess.
                multiplier: 1.0,
                venue: "Binance USDⓈ-M futures".into(),
                account: String::new(),
                symbol,
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
        let rows = signed_get(client, secrets, "/fapi/v1/openOrders", &[]).await?;
        let mut out = Vec::new();
        for o in rows.as_array().into_iter().flatten() {
            let symbol = str_of(o, "symbol").to_uppercase();
            out.push(OpenOrder {
                id: o
                    .get("orderId")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .unwrap_or_default(),
                side: str_of(o, "side").to_ascii_lowercase(),
                order_type: str_of(o, "type"),
                qty: num(o, "origQty"),
                filled_qty: num(o, "executedQty"),
                limit_price: Some(num(o, "price")).filter(|v| *v > 0.0),
                stop_price: Some(num(o, "stopPrice")).filter(|v| *v > 0.0),
                currency: quote_of(&symbol),
                asset_class: "crypto".into(),
                placed_at: o
                    .get("time")
                    .and_then(Value::as_i64)
                    .and_then(|t| OffsetDateTime::from_unix_timestamp(t / 1000).ok()),
                venue: "Binance USDⓈ-M futures".into(),
                account: String::new(),
                symbol,
            });
        }
        Ok(out)
    }

    /// The margin wallet, asset by asset. A futures position is collateralised, not owned,
    /// and it is reported by `positions`; what a holding lists here is the margin itself.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let rows = signed_get(client, secrets, "/fapi/v3/balance", &[]).await?;
        let mut out = Vec::new();
        for b in rows.as_array().into_iter().flatten() {
            let qty = num(b, "balance");
            if qty <= 0.0 {
                continue;
            }
            out.push(Holding {
                symbol: str_of(b, "asset").to_uppercase(),
                name: String::new(),
                qty,
                side: "long".into(),
                asset_class: "crypto".into(),
                // A margin balance carries no cost basis.
                avg_price: None,
                currency: String::new(),
                venue: "Binance USDⓈ-M futures".into(),
                account: String::new(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What the futures board trades these assets at right now, read in one public call.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let board = ticker_board(client, secrets).await?;
        let mut out = Vec::new();
        for s in symbols {
            let asset = s.trim().to_uppercase();
            if asset.is_empty() {
                continue;
            }
            // The asset may be named as a contract already (BTCUSDT) or as a coin (BTC).
            let pair = if board.contains_key(&asset) {
                Some(asset.clone())
            } else {
                QUOTE_COINS
                    .iter()
                    .map(|q| format!("{asset}{q}"))
                    .find(|p| board.contains_key(p))
            };
            let Some(pair) = pair else { continue };
            let Some(price) = board.get(&pair).copied().filter(|p| *p > 0.0) else { continue };
            out.push(Quote { symbol: s.clone(), price, currency: quote_of(&pair), pair });
        }
        Ok(out)
    }

    /// Contracts worth offering: what the account is positioned in or is working.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .positions(client, secrets)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.symbol)
            .collect();
        out.extend(
            self.orders(client, secrets)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|o| o.symbol),
        );
        out.retain(|s| !s.is_empty());
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// Quote coins a USDⓈ-M contract settles in, longest first so `BTCUSDT` is not read as
/// ending in `USD`.
const QUOTE_COINS: &[&str] = &["USDT", "USDC", "BUSD", "USD"];

// ── Mapping ──────────────────────────────────────────────────────────────────

fn fill(r: &Value, symbol: &str, at_ms: i64) -> Result<Execution> {
    Ok(Execution {
        id: r
            .get("id")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default(),
        order_id: r
            .get("orderId")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default(),
        // Kept to the millisecond: two fills of one order land in the same second.
        at: OffsetDateTime::from_unix_timestamp_nanos(at_ms as i128 * 1_000_000)
            .map_err(|_| anyhow!("Binance reported an impossible timestamp: {at_ms}"))?,
        symbol: symbol.to_string(),
        // `side` is BUY or SELL; `buyer` says the same thing and is read as the fallback.
        side: match str_of(r, "side").to_ascii_lowercase() {
            s if s == "buy" || s == "sell" => s,
            _ if r.get("buyer").and_then(Value::as_bool).unwrap_or(false) => "buy".into(),
            _ => "sell".into(),
        },
        qty: num(r, "qty"),
        price: num(r, "price"),
        fee: num(r, "commission"),
        fee_currency: str_of(r, "commissionAsset").to_uppercase(),
        currency: quote_of(symbol),
        asset_class: "crypto".into(),
        multiplier: 1.0,
        venue: "Binance USDⓈ-M futures".into(),
        account: String::new(),
    })
}

/// The settlement coin of a USDⓈ-M contract, from the longest known suffix it ends in.
fn quote_of(symbol: &str) -> String {
    QUOTE_COINS
        .iter()
        .find(|q| symbol.len() > q.len() && symbol.ends_with(*q))
        .map(|q| (*q).to_string())
        .unwrap_or_default()
}

/// Every listed contract and its last price, in one public call.
async fn ticker_board(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<HashMap<String, f64>> {
    let body: Value = crate::rate::send(
        "binance_futures",
        client.get(format!("{}/fapi/v1/ticker/price", super::at(base(secrets)))),
    )
    .await
    .context("asking Binance futures for its prices")?
    .json()
    .await
    .context("reading Binance futures' prices")?;
    let mut out = HashMap::new();
    for row in body.as_array().into_iter().flatten() {
        let sym = str_of(row, "symbol").to_uppercase();
        if !sym.is_empty() {
            out.insert(sym, num(row, "price"));
        }
    }
    Ok(out)
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "testnet" => TESTNET,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == TESTNET { "Testnet" } else { "Live" }
}

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
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

/// A signed GET: the exchange's clock, the query string, its HMAC, and the key header.
async fn signed_get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let key = super::require(secrets, "api_key")?;
    let secret = super::require(secrets, "api_secret")?;
    let mut qs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect();
    qs.push(format!("timestamp={}", server_time(client, secrets).await?));
    qs.push("recvWindow=20000".into());
    let query = qs.join("&");
    let sig = super::binance::hmac_sha256_hex(secret.as_bytes(), query.as_bytes());
    let url = format!("{}{path}?{query}&signature={sig}", super::at(base(secrets)));
    let res = crate::rate::send(
        "binance_futures",
        client.get(&url).header("X-MBX-APIKEY", key),
    )
    .await
    .with_context(|| format!("calling Binance futures {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = super::binance::trim_err(&body);
        return Err(match status.as_u16() {
            401 => anyhow!(
                "Binance refused the key on {path} ({detail}). Check the key belongs to the \
                 {} environment this account is set to and that futures access is enabled \
                 on it.",
                env_label(secrets).to_ascii_lowercase()
            ),
            418 | 429 => anyhow!(
                "Binance is rate-limiting this key ({detail}). Ask for a shorter period or \
                 fewer instruments."
            ),
            _ => anyhow!("Binance futures {path} answered {status}: {detail}"),
        });
    }
    serde_json::from_str(&body)
        .with_context(|| format!("reading Binance futures' {path} answer"))
}

/// The exchange's own clock in milliseconds. A signed request is rejected outright when the
/// host's clock drifts, so the timestamp is never taken from this machine.
async fn server_time(client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<i64> {
    let body: Value = crate::rate::send(
        "binance_futures",
        client.get(format!("{}/fapi/v1/time", super::at(base(secrets)))),
    )
    .await
    .context("asking Binance futures for its clock")?
    .json()
    .await
    .context("reading Binance futures' clock")?;
    body.get("serverTime")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("Binance futures did not report its server time"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fill_keeps_its_side_its_fee_and_its_milliseconds() {
        let r = serde_json::json!({
            "id": 42, "orderId": 7, "symbol": "BTCUSDT", "side": "SELL", "positionSide": "SHORT",
            "qty": "0.5", "price": "30000", "commission": "0.0075",
            "commissionAsset": "USDT", "realizedPnl": "1.2"
        });
        let e = fill(&r, "BTCUSDT", 1_700_000_000_123).unwrap();
        assert_eq!(e.side, "sell");
        assert_eq!(e.fee, 0.0075);
        assert_eq!(e.fee_currency, "USDT");
        assert_eq!(e.currency, "USDT");
        assert_eq!(e.at.unix_timestamp_nanos(), 1_700_000_000_123_000_000);
        // A linear contract is sized in the base coin, so its point value is one.
        assert_eq!(e.multiplier, 1.0);
    }

    #[test]
    fn a_contract_names_its_own_settlement_coin() {
        // Longest suffix first: BTCUSDT settles in USDT, not in USD.
        assert_eq!(quote_of("BTCUSDT"), "USDT");
        assert_eq!(quote_of("BTCUSDC"), "USDC");
        assert_eq!(quote_of("ETHUSD"), "USD");
        assert_eq!(quote_of("USDT"), "");
    }

    #[test]
    fn the_environment_is_checked_rather_than_guessed() {
        let live = HashMap::new();
        assert_eq!(base(&live), LIVE);
        let t = HashMap::from([("env".to_string(), "testnet".to_string())]);
        assert_eq!(base(&t), TESTNET);
        assert!(BinanceFutures.validate_config(&t).is_ok());
        let bad = HashMap::from([("env".to_string(), "paper".to_string())]);
        assert!(BinanceFutures.validate_config(&bad).is_err());
    }
}
