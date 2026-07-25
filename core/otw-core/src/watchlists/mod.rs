//! Watchlists: starter templates + the per-list auto-refresh loop.

pub mod quotes;

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use otw_store::watchlists as store;

/// Serializes refreshes (scheduled loop vs. manual trigger) so the same list is never
/// quoted twice concurrently and provider pacing stays honest.
pub type RefreshLock = Arc<tokio::sync::Mutex<()>>;

pub fn new_lock() -> RefreshLock {
    Arc::new(tokio::sync::Mutex::new(()))
}

/// How often the loop wakes to look for lists whose own interval has elapsed. The floor on
/// `refresh_secs` is 5 (custom-connector lists can go sub-minute), so the tick matches it —
/// the wake query is a single indexed SELECT on a tiny table, cheap at this rate.
const TICK: Duration = Duration::from_secs(5);

/// Spawn the auto-refresh loop: every tick, refresh each sync-enabled watchlist whose
/// `refresh_secs` has elapsed since its last refresh. Each list stamps `refreshed_at` even
/// when some items fail, so one bad symbol can't wedge the list into a hot retry loop.
pub fn spawn(pool: PgPool, lock: RefreshLock) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let due = match store::list_due(&pool).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!("watchlists: listing due lists failed: {e:#}");
                    continue;
                }
            };
            for wl in &due {
                let _guard = lock.lock().await;
                if let Err(e) = quotes::refresh_watchlist(&pool, wl).await {
                    tracing::error!("watchlist {} refresh failed: {e:#}", wl.id);
                }
            }
        }
    });
}

// ── Starter templates ─────────────────────────────────────────────────────────

/// A symbol a template seeds: (asset_class, provider, provider_id, symbol, name).
pub struct TemplateSymbol {
    pub asset_class: &'static str,
    pub provider: &'static str,
    pub provider_id: &'static str,
    pub symbol: &'static str,
    pub name: &'static str,
}

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub symbols: &'static [TemplateSymbol],
}

macro_rules! sym {
    ($class:literal, $provider:literal, $pid:literal, $sym:literal, $name:literal) => {
        TemplateSymbol {
            asset_class: $class,
            provider: $provider,
            provider_id: $pid,
            symbol: $sym,
            name: $name,
        }
    };
}

/// Curated starter lists. Provider ids are the canonical CoinGecko coin ids / Yahoo tickers,
/// same resolution the search endpoint would produce.
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "crypto-top10",
        name: "Crypto Top 10",
        symbols: &[
            sym!("crypto", "coingecko", "bitcoin", "BTC", "Bitcoin"),
            sym!("crypto", "coingecko", "ethereum", "ETH", "Ethereum"),
            sym!("crypto", "coingecko", "ripple", "XRP", "XRP"),
            sym!("crypto", "coingecko", "binancecoin", "BNB", "BNB"),
            sym!("crypto", "coingecko", "solana", "SOL", "Solana"),
            sym!("crypto", "coingecko", "cardano", "ADA", "Cardano"),
            sym!("crypto", "coingecko", "dogecoin", "DOGE", "Dogecoin"),
            sym!("crypto", "coingecko", "tron", "TRX", "TRON"),
            sym!("crypto", "coingecko", "avalanche-2", "AVAX", "Avalanche"),
            sym!("crypto", "coingecko", "chainlink", "LINK", "Chainlink"),
        ],
    },
    Template {
        id: "mag7",
        name: "Magnificent 7",
        symbols: &[
            sym!("stock", "yahoo", "AAPL", "AAPL", "Apple Inc."),
            sym!("stock", "yahoo", "MSFT", "MSFT", "Microsoft Corporation"),
            sym!("stock", "yahoo", "GOOGL", "GOOGL", "Alphabet Inc."),
            sym!("stock", "yahoo", "AMZN", "AMZN", "Amazon.com, Inc."),
            sym!("stock", "yahoo", "NVDA", "NVDA", "NVIDIA Corporation"),
            sym!("stock", "yahoo", "META", "META", "Meta Platforms, Inc."),
            sym!("stock", "yahoo", "TSLA", "TSLA", "Tesla, Inc."),
        ],
    },
    Template {
        id: "us-index-etfs",
        name: "US Index ETFs",
        symbols: &[
            sym!("etf", "yahoo", "SPY", "SPY", "SPDR S&P 500 ETF Trust"),
            sym!("etf", "yahoo", "QQQ", "QQQ", "Invesco QQQ Trust"),
            sym!("etf", "yahoo", "DIA", "DIA", "SPDR Dow Jones Industrial Average ETF"),
            sym!("etf", "yahoo", "IWM", "IWM", "iShares Russell 2000 ETF"),
            sym!("etf", "yahoo", "VTI", "VTI", "Vanguard Total Stock Market ETF"),
        ],
    },
    Template {
        id: "semis",
        name: "Semiconductors",
        symbols: &[
            sym!("stock", "yahoo", "NVDA", "NVDA", "NVIDIA Corporation"),
            sym!("stock", "yahoo", "AMD", "AMD", "Advanced Micro Devices, Inc."),
            sym!("stock", "yahoo", "AVGO", "AVGO", "Broadcom Inc."),
            sym!("stock", "yahoo", "TSM", "TSM", "Taiwan Semiconductor Manufacturing"),
            sym!("stock", "yahoo", "ASML", "ASML", "ASML Holding N.V."),
            sym!("stock", "yahoo", "INTC", "INTC", "Intel Corporation"),
            sym!("stock", "yahoo", "QCOM", "QCOM", "QUALCOMM Incorporated"),
            sym!("stock", "yahoo", "MU", "MU", "Micron Technology, Inc."),
        ],
    },
];

pub fn template_by_id(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}
