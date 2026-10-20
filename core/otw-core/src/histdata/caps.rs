//! What each market-data connector declares: credentials, settings, timeframes, limits.
//! The connectors page, the job form and the server-side checks read these; the downloads
//! themselves go through the connector framework ([`super::native`]).

use super::{Capability, ConfigField};

pub(crate) static BINANCE: Capability = Capability {
    provider: "binance",
    label: "Binance",
    website: "https://binance.com",
    docs_url: "https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints",
    rate_limit: "Keyless. IP request-weight ~1200/min; each klines call costs 1–2. 429 = back off, repeated abuse → 418 IP ban. Downloads auto-retry with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    // ~1200 weight/min, klines cost 1-2: 5 req/s stays far under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Binance publishes a kline channel per interval, so every downloadable timeframe
    // streams natively — a 24/7 market's daily candle *is* the epoch day.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live klines are public and keyless, like the history.",
};

pub(crate) static BINANCE_FUTURES: Capability = Capability {
    provider: "binance_futures",
    label: "Binance USDⓈ-M Futures",
    website: "https://www.binance.com",
    docs_url: "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api",
    rate_limit: "Keyless. 2400 request weight per minute per IP; a klines call costs 1–10 by \
                 page size. 429 = back off, repeated abuse → 418 IP ban. Downloads auto-retry \
                 with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1500,
    // 2400 weight/minute and a full page costs 10: 5 requests a second stays well under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Binance publishes a kline channel per interval, so every downloadable timeframe
    // streams natively; a 24/7 market's daily candle is the epoch day.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live klines are public and keyless, like the history.",
};

static BITGET_FIELDS: &[ConfigField] = &[ConfigField {
    name: "market",
    label: "Market",
    placeholder: "spot",
    kind: "text",
    required: false,
    help: "spot or usdt-futures. BTCUSDT is a spot pair and a perpetual at once, and the \
           two are different series, so which one this connector downloads is stated rather \
           than inferred. Leave empty for spot.",
}];

pub(crate) static BITGET: Capability = Capability {
    provider: "bitget",
    label: "Bitget",
    website: "https://www.bitget.com",
    docs_url: "https://www.bitget.com/docs/catalog/classic-spot-market/classic-spot-market",
    rate_limit: "Keyless. 20 requests/second per IP on the public candle endpoints, 6000 per \
                 minute overall. A candle page is 200 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 200,
    // 20 req/s is the published ceiling; 5 a second leaves room and never trips it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: BITGET_FIELDS,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Bitget publishes a candle channel per interval, so every downloadable timeframe
    // streams natively. The daily and weekly channels are the UTC-anchored ones, like the
    // download.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. The connector's \
                  Market setting picks the socket: spot and USDT-M futures are separate \
                  feeds for the same ticker.",
};

static BYBIT_FIELDS: &[ConfigField] = &[ConfigField {
    name: "market",
    label: "Market",
    placeholder: "spot",
    kind: "text",
    required: false,
    help: "spot, linear or inverse. BTCUSDT is a spot pair and a perpetual at once, and the \
           two are different series, so which one this connector downloads is stated rather \
           than inferred. linear covers the USDT and USDC perpetuals and the USDT dated \
           futures; inverse the coin-settled contracts (BTCUSD), whose volume is counted in \
           the coin. Leave empty for spot. Bybit EU is its own connector.",
}];

pub(crate) static BYBIT: Capability = Capability {
    provider: "bybit",
    label: "Bybit",
    website: "https://www.bybit.com",
    docs_url: "https://bybit-exchange.github.io/docs/v5/market/kline",
    rate_limit: "Keyless. 600 requests per 5 seconds per IP; past it Bybit answers 403 for ten \
                 minutes. A candle page is 1000 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    // 120 requests a second is the published ceiling; 5 a second stays far under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: BYBIT_FIELDS,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Bybit publishes a kline topic per interval, each row flagged once closed, so every
    // downloadable timeframe streams natively. Its daily bar opens at midnight UTC.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. The connector's \
                  Market setting picks the socket: spot, linear and inverse are separate \
                  feeds.",
};

pub(crate) static BYBIT_EU: Capability = Capability {
    provider: "bybit_eu",
    label: "Bybit EU",
    website: "https://www.bybit.eu",
    docs_url: "https://bybit-exchange.github.io/docs/v5/market/kline",
    rate_limit: "Keyless. 600 requests per 5 seconds per IP; past it Bybit answers 403 for ten \
                 minutes. A candle page is 1000 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Bybit EU (bybit.eu) is a separate platform: its own spot book, thinner than \
                  bybit.com's, so its prices and volumes are its own. Live candles are public \
                  and keyless.",
};

static GATEIO_FIELDS: &[ConfigField] = &[ConfigField {
    name: "market",
    label: "Market",
    placeholder: "spot",
    kind: "text",
    required: false,
    help: "spot or futures. BTC_USDT is a spot pair and a perpetual at once, and the two are \
           different series, so which one this connector downloads is stated rather than \
           inferred. futures covers the USDT-settled perpetuals, volume counted in the coin. \
           Leave empty for spot.",
}];

pub(crate) static GATEIO: Capability = Capability {
    provider: "gateio",
    label: "Gate.io",
    website: "https://www.gate.com",
    docs_url: "https://www.gate.com/docs/developers/apiv4/en/#market-candles",
    rate_limit: "Keyless. 200 requests per 10 seconds per IP and endpoint. A candle page is \
                 1000 bars on spot, 2000 on futures, and Gate.io keeps only the last 10000 \
                 bars of each timeframe: 1m reaches back about a week, 1h a year.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    // 20 requests a second is the published ceiling; 5 a second stays far under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: GATEIO_FIELDS,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Gate.io publishes a candle topic per interval, each row flagged once closed. Its
    // weekly bar opens on Monday, its daily bar at midnight UTC.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. The connector's \
                  Market setting picks the socket: spot and futures are separate feeds for \
                  the same ticker.",
};

static MEXC_FIELDS: &[ConfigField] = &[ConfigField {
    name: "market",
    label: "Market",
    placeholder: "spot",
    kind: "text",
    required: false,
    help: "spot or futures. A spot pair is written BTCUSDT, a perpetual BTC_USDT. futures \
           covers the perpetuals settled in USDT, USDC or USD1, volume counted in the coin. \
           Leave empty for spot.",
}];

pub(crate) static MEXC: Capability = Capability {
    provider: "mexc",
    label: "MEXC",
    website: "https://www.mexc.com",
    docs_url: "https://mexcdevelop.github.io/apidocs/spot_v3_en/#kline-candlestick-data",
    rate_limit: "Keyless. 500 requests per 10 seconds per IP on spot, 20 per 2 seconds on \
                 futures. A candle page is 500 bars on spot, 2000 on futures. MEXC keeps 30 \
                 days of 1m bars and 360 days of 5m and 15m bars; 1h and longer reach back \
                 to the listing.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 500,
    min_interval_ms: 200,
    searchable: true,
    config_fields: MEXC_FIELDS,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // MEXC pushes every interval; no frame says a bar is closed, so the engine closes it
    // when the next one starts. Its weekly bar opens on Monday, its daily bar at midnight UTC.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. The connector's \
                  Market setting picks the socket. One spot socket streams 30 panes at most.",
};

static BITFINEX_FIELDS: &[ConfigField] = &[ConfigField {
    name: "market",
    label: "Market",
    placeholder: "spot",
    kind: "text",
    required: false,
    help: "spot or derivatives. A spot pair is written BTCUSD (or tBTCUSD), AVAX:USD when a \
           code is longer than three letters; a perpetual BTCF0:USTF0. Leave empty for spot.",
}];

pub(crate) static BITFINEX: Capability = Capability {
    provider: "bitfinex",
    label: "Bitfinex",
    website: "https://www.bitfinex.com",
    docs_url: "https://docs.bitfinex.com/reference/rest-public-candles",
    rate_limit: "Keyless. 30 candle pages per minute per IP, 10000 bars a page; an IP over a \
                 limit is blocked for a minute. Every timeframe reaches back to the listing.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 10000,
    min_interval_ms: 2000,
    searchable: true,
    config_fields: BITFINEX_FIELDS,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Bitfinex pushes every interval; no frame says a bar is closed, so the engine closes it
    // when the next one starts. Its weekly bar opens on Monday, its daily bar at midnight UTC.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. One socket streams \
                  30 panes at most.",
};

pub(crate) static DERIBIT: Capability = Capability {
    provider: "deribit",
    label: "Deribit",
    website: "https://www.deribit.com",
    docs_url: "https://docs.deribit.com/",
    rate_limit: "Keyless. About 20 calls per second per IP, 5000 bars a page; every \
                 timeframe reaches back to the listing. Daily bars open at 08:00 UTC, \
                 Deribit's settlement time.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 5000,
    min_interval_ms: 100,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // Deribit pushes 1m to 2h and 1d; no frame says a bar is closed, so the engine closes it
    // when the next one starts. 4h and 1w are folded from hours and days.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. Spot pairs matched \
                  on Coinbase Exchange (BTC_USDC, ETH_USDC) have no candles here: read them \
                  from Coinbase.",
};

pub(crate) static KUCOIN: Capability = Capability {
    provider: "kucoin",
    label: "KuCoin",
    website: "https://www.kucoin.com",
    docs_url: "https://www.kucoin.com/docs-new/rest/spot-trading/market-data/get-klines",
    rate_limit: "Keyless. 2000 request weight per 30 seconds per IP; a candle page costs 3 \
                 and holds 1500 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1500,
    // 2000 weight per 30 s and a page costs 3: 5 a second stays far under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // KuCoin publishes a candle topic per interval. Its weekly bar opens on Thursday, so a
    // week streams as days folded Monday to Sunday, like the download.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. Spot and futures are \
                  separate platforms: KuCoin Futures is its own connector.",
};

pub(crate) static KUCOIN_FUTURES: Capability = Capability {
    provider: "kucoin_futures",
    label: "KuCoin Futures",
    website: "https://www.kucoin.com/futures",
    docs_url: "https://www.kucoin.com/docs-new/rest/futures-trading/market-data/get-klines",
    rate_limit: "Keyless. 2000 request weight per 30 seconds per IP; a candle page costs 3 \
                 and holds 200 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 200,
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless. USDT- and USDC-margined contracts \
                  (XBTUSDTM, ETHUSDCM), volume counted in the coin; coin-margined contracts \
                  are not served.",
};

pub(crate) static COINBASE: Capability = Capability {
    provider: "coinbase",
    label: "Coinbase",
    website: "https://coinbase.com",
    docs_url: "https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproductcandles",
    rate_limit: "Keyless public API ~10 req/s per IP; 300 candles max per request. Bursts return 429. Downloads auto-retry with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    // Coinbase granularities: 60,300,900,3600,21600,86400 — no 4h or 1w.
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    adjusted: false,
    max_bars_per_req: 300,
    // Public API tolerates ~10 req/s per IP; half that leaves room for bursts.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // The candle is built here from the public trade feed, so any period works.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are aggregated from the public trade feed, keyless like the history.",
};

pub(crate) static KRAKEN: Capability = Capability {
    provider: "kraken",
    label: "Kraken",
    website: "https://kraken.com",
    docs_url: "https://docs.kraken.com/api/docs/rest-api/get-ohlc-data",
    rate_limit: "Keyless public API ~1 req/s sustained; OHLC returns up to 720 bars ending now (limited history for fine timeframes). Downloads auto-retry with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 720,
    // ~1 req/s sustained is the documented ceiling.
    min_interval_ms: 1100,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live OHLC is public and keyless, like the history.",
};

pub(crate) static ALPHAVANTAGE: Capability = Capability {
    provider: "alphavantage",
    label: "Alpha Vantage",
    website: "https://alphavantage.co",
    docs_url: "https://www.alphavantage.co/documentation/",
    rate_limit: "Free key: 25 requests/day and 5/min — very tight. Over-limit returns a JSON note (surfaced as a job error), not 429. Consider a premium key for bulk history.",
    required_secrets: &["api_key"],
    asset_types: &["equity", "etf", "crypto", "fx"],
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    // Free daily endpoint (TIME_SERIES_DAILY) is unadjusted; the adjusted variant is premium.
    adjusted: false,
    max_bars_per_req: 5000,
    // Free key: 5 requests/minute. One every 12 s is the honest pace.
    min_interval_ms: 12_000,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &["estimates", "earnings", "dividends", "etf", "calendar", "transcripts"],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

pub(crate) static EODHD: Capability = Capability {
    provider: "eodhd",
    label: "EODHD",
    website: "https://eodhd.com",
    docs_url: "https://eodhd.com/financial-apis/",
    rate_limit: "Paid key. Daily API-call allowance by plan (intraday costs more calls per request); 429 when exhausted. Downloads auto-retry with backoff.",
    required_secrets: &["api_key"],
    asset_types: &["equity", "etf", "fx", "crypto"],
    timeframes: &["1m", "5m", "1h", "1d", "1w"],
    adjusted: true,
    max_bars_per_req: 5000,
    // Paid plans meter per day, not per second; a light spacing is enough.
    min_interval_ms: 250,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &["dividends", "etf", "calendar"],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

pub(crate) static YAHOO: Capability = Capability {
    provider: "yahoo",
    label: "Yahoo Finance",
    website: "https://finance.yahoo.com",
    docs_url: "https://query1.finance.yahoo.com/v8/finance/chart/AAPL",
    rate_limit: "Unofficial/keyless endpoint — no published limits. Aggressive polling can get your IP throttled (429) or blocked; 1m data is capped to recent weeks. Best-effort only.",
    required_secrets: &[],
    asset_types: &["equity", "etf", "index", "crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    adjusted: true,
    max_bars_per_req: 5000,
    // Unofficial endpoint with no published limit: stay deliberately slow.
    min_interval_ms: 600,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

pub(crate) static ALPACA: Capability = Capability {
    provider: "alpaca",
    label: "Alpaca",
    website: "https://alpaca.markets",
    docs_url: "https://docs.alpaca.markets/reference/stockbars",
    rate_limit: "Free key: 200 requests/min. Free plan serves IEX equities (15-min delayed, limited history) and the indicative options feed (15-min delayed, history only from Feb 2024); SIP/OPRA full history needs a paid plan. Downloads auto-retry with backoff.",
    required_secrets: &["api_key", "api_secret"],
    asset_types: &["equity", "crypto", "option"],
    // `1w` is folded from daily bars here rather than asked of the vendor's own `1Week`
    // aggregate: see [`fold_weekly`].
    timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    adjusted: true,
    max_bars_per_req: 10000,
    // Free key: 200 requests/minute.
    min_interval_ms: 350,
    searchable: true,
    config_fields: ALPACA_FIELDS,
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["equity", "crypto", "option"],
    // Every downloadable timeframe streams: the one-minute source folds up, and the daily and
    // weekly buckets are anchored on Alpaca's own session stamp rather than the UTC epoch.
    stream_timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    stream_note: "Live streams one-minute bars folded up to your timeframe, so the chart \
                  updates once a minute (the daily and weekly candles too, seeded from the \
                  session's own bar). A free key streams IEX equities, crypto and the \
                  indicative options feed and allows one connection at a time; SIP and OPRA \
                  need a paid plan and the connector's Feed setting set to sip.",
};

/// Alpaca's one non-secret setting. Not a credential: which market-data entitlement the key
/// is on decides *which socket* the live feed opens, and asking for one the plan does not
/// have is a refusal rather than thinner data. Optional, because the free entitlement is
/// what a key created today has.
static ALPACA_FIELDS: &[ConfigField] = &[ConfigField {
    name: "feed",
    label: "Market data feed",
    placeholder: "iex",
    kind: "text",
    required: false,
    help: "Which Alpaca market-data entitlement this key is on: iex for the free plan (IEX \
           equities and the indicative options feed), sip for a paid one (full tape and OPRA). \
           Leave empty for iex. It selects the live socket; downloads are unaffected.",
}];

pub(crate) static MASSIVE: Capability = Capability {
    provider: "massive",
    label: "Massive (Polygon.io)",
    website: "https://massive.com",
    docs_url: "https://massive.com/docs/rest/quickstart",
    rate_limit: "Free key: 5 requests/minute (over-limit returns HTTP 429, auto-retried with backoff — the cap makes wide intraday ranges page slowly). Free plans are end-of-day: use the 1d timeframe. Stocks, ETFs, crypto, forex, options, indices and futures all work at EOD with ~2 years of history; intraday and older ranges need a paid plan. A few tickers stay premium even at EOD (e.g. the I:SPX index) and return NOT_AUTHORIZED, surfaced as a clear job error rather than empty data. Paid plans add intraday, real-time and full history.",
    required_secrets: &["api_key"],
    // Aggregates cover every Polygon market. Options/futures use the O:/ (and vendor)
    // ticker prefixes; crypto is X:BTCUSD, fx is C:EURUSD, indices I:SPX.
    asset_types: &["equity", "etf", "option", "future", "crypto", "fx", "index"],
    timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    // Aggregates fold splits/dividends straight into OHLC when adjusted=true (no separate
    // adj_* column), so we request unadjusted and leave adj_* None for consistency with peers.
    adjusted: false,
    max_bars_per_req: 50000,
    // Free key: 5 requests/minute, and 429 over it.
    min_interval_ms: 12_000,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &["dividends", "short_interest"],
    stream_asset_types: &["equity", "etf", "option", "crypto", "fx", "index", "future"],
    // Minute aggregates folded up to every downloadable timeframe, daily and weekly included:
    // their buckets follow the stamp Massive's own bars carry, not the UTC epoch.
    stream_timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    stream_note: "Live streaming is a paid Massive feature: a free (end-of-day) key opens the \
                  socket and is refused at login, while the same key still downloads history. \
                  One live connection per market at a time, and futures are their own \
                  entitlement (Futures Starter and up) on top of the equity one.",
};

static OANDA_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "001-004-1234567-001",
        kind: "text",
        required: true,
        help: "The v20 account number the candles are read through. OANDA serves prices per \
               account, because which instruments exist depends on the division the account \
               is in.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or practice. fxTrade and fxPractice are different hosts with different \
               tokens and different account numbers. Leave empty for live.",
    },
];

pub(crate) static OANDA: Capability = Capability {
    provider: "oanda",
    label: "OANDA",
    website: "https://www.oanda.com",
    docs_url: "https://developer.oanda.com/rest-live-v20/pricing-ep/",
    rate_limit: "120 requests per second per token. A candle request returns up to 5000 bars, \
                 so a long history is a few dozen calls.",
    required_secrets: &["api_token"],
    // CURRENCY instruments are fx; the index, commodity and metal CFDs are not an asset type
    // of their own here, so they are downloaded as fx pairs of their quote currency would be.
    asset_types: &["fx"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 5000,
    // 120 req/s is the published ceiling; 10 a second is plenty for a download and leaves
    // the account's own calls room.
    min_interval_ms: 100,
    searchable: true,
    config_fields: OANDA_FIELDS,
    testable: true,
    fundamentals: &[],
    // Live prices are an HTTP stream, not a WebSocket: see `stream_note`.
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "OANDA streams prices over a long-lived HTTP response rather than a \
                  WebSocket, which this app's live charts do not speak. History downloads \
                  and the account side work; the live candle does not.",
};

pub(crate) static OKX: Capability = Capability {
    provider: "okx",
    label: "OKX",
    website: "https://www.okx.com",
    docs_url: "https://www.okx.com/docs-v5/en/#order-book-trading-market-data",
    rate_limit: "Keyless. 20 requests per 2 seconds per IP on the candle archive. A page is \
                 300 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 300,
    // 20 requests per 2 seconds is the published ceiling; 5 a second stays under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    fundamentals: &[],
    stream_asset_types: &["crypto"],
    // OKX publishes a candle channel per interval, so every downloadable timeframe streams
    // natively. The daily and weekly channels are the UTC-anchored ones, like the download.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. They ride OKX's \
                  business socket, which allows a few connections per IP.",
};

static TRADESTATION_FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or sim. Simulated trading is a different host with its own key. Leave empty \
           for live.",
}];

pub(crate) static TRADESTATION: Capability = Capability {
    provider: "tradestation",
    label: "TradeStation",
    website: "https://www.tradestation.com",
    docs_url: "https://api.tradestation.com/docs/",
    rate_limit: "500 bar-chart requests per 5 minutes, plus a credit allowance for how much \
                 history is asked for per minute. One intraday request returns at most \
                 57,600 bars.",
    required_secrets: &["client_id", "client_secret", "refresh_token"],
    asset_types: &["equity", "etf", "option", "future", "index"],
    // See the module note: the hourly, four-hourly and weekly bars are anchored on the
    // session rather than on the epoch, so they are not offered.
    timeframes: &["1m", "5m", "15m", "1d"],
    // Bar charts are not split-adjusted on this endpoint.
    adjusted: false,
    max_bars_per_req: 57_600,
    // 500 requests per 5 minutes is 1.67 a second; 600 ms leaves room for the account side.
    min_interval_ms: 600,
    searchable: true,
    config_fields: TRADESTATION_FIELDS,
    testable: true,
    fundamentals: &[],
    // TradeStation streams over a long-lived HTTP response, not a WebSocket: see the note.
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "TradeStation streams bars and quotes over a long-lived HTTP response \
                  rather than a WebSocket, which this app's live charts do not speak. \
                  History downloads and the account side work; the live candle does not.",
};

pub(crate) static FOREXCOM: Capability = Capability {
    provider: "forexcom",
    label: "FOREX.com (StoneX)",
    website: "https://www.forex.com",
    docs_url: "https://docs.labs.gaincapital.com/",
    rate_limit: "500 requests per 5 seconds. One bar request returns at most 4000 bars, and \
                 the session token is shared with the broker account on the same login.",
    required_secrets: &["username", "password", "app_key"],
    asset_types: &["fx"],
    // See the module note: the four-hourly and weekly bars have no documented anchor here,
    // so they are refused rather than stored out of step.
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    adjusted: false,
    max_bars_per_req: 4000,
    // 500 requests per 5 seconds is far more than a download needs; stay gentle, the same
    // session serves the account side.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: true,
    fundamentals: &[],
    // StoneX streams over Lightstreamer, not a WebSocket: see `stream_note`.
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "FOREX.com streams prices over Lightstreamer rather than a WebSocket, which \
                  this app's live charts do not speak. History downloads and the account \
                  side work; the live candle does not.",
};

static CAPITALCOM_FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or demo. The demo platform is a different host with its own key. Leave empty \
           for live.",
}];

pub(crate) static CAPITALCOM: Capability = Capability {
    provider: "capitalcom",
    label: "Capital.com",
    website: "https://capital.com",
    docs_url: "https://open-api.capital.com/",
    rate_limit: "10 requests per second per user, and one sign-in per second. One price \
                 request returns at most 1000 bars.",
    required_secrets: &["api_key", "identifier", "api_password"],
    asset_types: &["fx", "index", "equity", "crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    // 10 requests a second is the ceiling; 5 a second leaves the account side room.
    min_interval_ms: 200,
    searchable: true,
    config_fields: CAPITALCOM_FIELDS,
    testable: true,
    fundamentals: &[],
    stream_asset_types: &["fx", "index", "equity", "crypto"],
    // Capital.com publishes a candle channel per resolution, and the ones it serves are the
    // ones downloaded here.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles ride the same login as the history and count against the same \
                  session, which allows 40 instruments at once. Capital.com streams the bid \
                  and the ask as two separate candles and the chart shows their mid, so a \
                  pane fills once both sides have ticked.",
};

static IBKR_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "host",
        label: "Gateway host",
        placeholder: "host.docker.internal",
        kind: "text",
        required: true,
        help: "Where IB Gateway or TWS is running. OpenTraderWorld runs in a container, so its own \
               localhost is not yours: use host.docker.internal for a gateway on this machine.",
    },
    ConfigField {
        name: "port",
        label: "API port",
        placeholder: "4002",
        kind: "number",
        required: true,
        help: "The port the gateway listens on: 4001 (IB Gateway, live), 4002 (IB Gateway, paper), \
               7496 (TWS, live), 7497 (TWS, paper).",
    },
];

pub(crate) static IBKR: Capability = Capability {
    provider: "ibkr",
    label: "Interactive Brokers",
    website: "https://www.interactivebrokers.com",
    docs_url: "https://interactivebrokers.github.io/tws-api/historical_bars.html",
    rate_limit: "Runs against your own IB Gateway or TWS, so there is no key and no vendor plan: \
                 the data is whatever your IB account is subscribed to, and an instrument you have \
                 no market-data subscription for returns an error rather than bars. Interactive \
                 Brokers allows 60 historical requests per rolling 10 minutes per account, which \
                 this connector paces for you, so a long backfill is slow by design. Futures and \
                 options need their own exchange subscription, and a contract you are not \
                 subscribed to reports that rather than returning empty. The gateway \
                 restarts itself once a day and the connection is re-opened on its own. Volume is \
                 reported as Interactive Brokers sends it.",
    required_secrets: &[],
    asset_types: otw_connector_providers::ibkr::gateway::contract::ASSET_TYPES,
    timeframes: &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"],
    // Historical bars come back split-adjusted, folded straight into OHLC rather than as a
    // separate series, so there is no distinct adjusted close to store.
    adjusted: false,
    // One worker chunk; `fetch_chunk` slices it again into windows IB accepts (see
    // `contract::max_window_secs`), so this number is about progress granularity only.
    max_bars_per_req: 2000,
    // The real limiter is the rolling window in `session`; this only keeps the worker from
    // queueing chunks back to back.
    min_interval_ms: 1_500,
    searchable: true,
    config_fields: IBKR_FIELDS,
    testable: true,
    fundamentals: &[],
    // Whatever the account is subscribed to, which is the same set the history covers.
    stream_asset_types: otw_connector_providers::ibkr::gateway::contract::ASSET_TYPES,
    // Live candles are folded from tick-by-tick prints, or from the five-second stream where
    // IB serves no ticks. Daily and weekly work too: their buckets are anchored on the
    // session stamp IB's own bars carry.
    stream_timeframes: &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are built from Interactive Brokers' tick-by-tick prints, so \
                  every timeframe works and the price moves on the trade. Options and \
                  non-CME indices have no tick stream at IB and fall back to its five-second \
                  bars. It needs a live market-data subscription on the instrument (delayed \
                  data is not a live feed), IB allows one live session per account (a running \
                  TWS or mobile app holds the seat), and it allows only as many simultaneous \
                  tick streams as the account has market-depth lines, a handful on a plain \
                  account.",
};
