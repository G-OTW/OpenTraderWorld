//! What each broker account declares: credentials, settings, what its API can answer. The
//! settings page, the import modal and the server-side checks read these; the reads
//! themselves go through the connector framework ([`super::native`]).

use super::{BrokerCapability, ConfigField};

static ALPACA_FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or paper. Alpaca serves the two from different hosts and issues a separate \
           key for each, so this has to be said rather than guessed. Leave empty for live.",
}];

pub(crate) static ALPACA: BrokerCapability = BrokerCapability {
    broker: "alpaca",
    label: "Alpaca",
    website: "https://alpaca.markets",
    docs_url: "https://docs.alpaca.markets/reference/getaccount-1",
    rate_limit: "200 requests/minute on a free key. Fills page 100 at a time, and each new \
                 instrument costs one lookup to learn what it is.",
    key_note: "A trading API key and secret, from the dashboard of the environment you pick \
               below: a paper key does not work against the live account, or the other way \
               round. Nothing here places, changes or cancels an order. Alpaca reports no \
               commission on a fill, so an imported trade carries no fees: right for \
               equities, which are commission-free, and short of the truth for crypto and \
               options, whose fees arrive as separate account activities.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: ALPACA_FIELDS,
    asset_classes: &["stock", "option", "crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // A margin account can go short, and Alpaca reports it as such.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

pub(crate) static BINANCE: BrokerCapability = BrokerCapability {
    broker: "binance",
    label: "Binance",
    website: "https://www.binance.com",
    docs_url: "https://developers.binance.com/docs/binance-spot-api-docs",
    rate_limit: "6000 request weight per minute; a trade page costs 20.",
    key_note: "An API key with **Enable Reading** only. Do not enable trading or \
               withdrawals: this connector never writes.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: true,
    window_from_broker: false,
    history_days: 0,
    spot_only: true,
    positions: false,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static BINANCE_FUTURES_FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or testnet. Binance's futures testnet is a different host with its own key, \
           so this is stated rather than guessed. Leave empty for live.",
}];

pub(crate) static BINANCE_FUTURES: BrokerCapability = BrokerCapability {
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
    config_fields: BINANCE_FUTURES_FIELDS,
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

static BITGET_FIELDS: &[ConfigField] = &[ConfigField {
    name: "books",
    label: "Books to read",
    placeholder: "spot,usdt-futures",
    kind: "text",
    required: false,
    help: "Which Bitget books this account reads: spot, usdt-futures, or both separated by \
           a comma. Leave empty for both. A book you do not trade is worth turning off: \
           futures fills page one week at a time, so asking for them costs calls.",
}];

pub(crate) static BITGET: BrokerCapability = BrokerCapability {
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
    config_fields: BITGET_FIELDS,
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

static BYBIT_SITE_HELP: &str = "Which Bybit site issued the key: global (bybit.com, the default), \
     bytick, tr, kz, ge, ae, id, jp, hk, or testnet. A key works only on the site that issued it.";

static BYBIT_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "books",
        label: "Books to read",
        placeholder: "spot,linear,inverse,option",
        kind: "text",
        required: false,
        help: "Which Bybit books this account reads: spot, linear, inverse, option, several \
               separated by commas. Leave empty for all. Inverse fills are never imported (their \
               profit is counted in the coin), its positions and orders are. A book you do not \
               trade is worth turning off: fills page one week at a time, options once per coin.",
    },
    ConfigField { name: "site", label: "Site", placeholder: "global", kind: "text", required: false, help: BYBIT_SITE_HELP },
];

pub(crate) static BYBIT: BrokerCapability = BrokerCapability {
    broker: "bybit",
    label: "Bybit",
    website: "https://www.bybit.com",
    docs_url: "https://bybit-exchange.github.io/docs/v5/order/execution",
    rate_limit: "10 to 50 requests per second per account depending on the endpoint. Fills \
                 page a week at a time, options once per base coin, positions and orders once \
                 per settle coin.",
    key_note: "A **Read-Only** API key of a unified trading account, system-generated (HMAC) or \
               self-generated (RSA: paste the private key in PEM as the secret). Do not grant \
               Trade or Withdraw: this connector never writes, and its test says when a key \
               could. Bybit serves **two years** of fills over the API. Inverse fills are not \
               imported, their positions and orders are. Bybit EU is its own broker.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: BYBIT_FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 730,
    // The contract books are margin books: a short is a short. A spot-only account is worth
    // unticking "this account can go short" for, which the import modal offers.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static BYBIT_EU_FIELDS: &[ConfigField] = &[ConfigField {
    name: "site",
    label: "Site",
    placeholder: "eu",
    kind: "text",
    required: false,
    help: "eu (the default) or eu-testnet. A bybit.eu key works on Bybit EU only.",
}];

pub(crate) static BYBIT_EU: BrokerCapability = BrokerCapability {
    broker: "bybit_eu",
    label: "Bybit EU",
    website: "https://www.bybit.eu",
    docs_url: "https://bybit-exchange.github.io/docs/v5/order/execution",
    rate_limit: "10 to 50 requests per second per account depending on the endpoint. Fills \
                 page a week at a time.",
    key_note: "A **Read-Only** API key created on bybit.eu, system-generated (HMAC) or \
               self-generated (RSA: paste the private key in PEM as the secret). Do not grant \
               Trade or Withdraw: this connector never writes. Bybit EU is a separate platform \
               from bybit.com: its own spot book and its own keys. Two years of fills.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: BYBIT_EU_FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 730,
    spot_only: true,
    positions: false,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static GATEIO_FIELDS: &[ConfigField] = &[ConfigField {
    name: "books",
    label: "Books to read",
    placeholder: "spot,futures",
    kind: "text",
    required: false,
    help: "Which Gate.io books this account reads: spot, futures (the USDT-settled \
           perpetuals), or both separated by a comma. Leave empty for both. A book you do not \
           trade is worth turning off: fills page one week at a time, so asking for them \
           costs calls.",
}];

pub(crate) static GATEIO: BrokerCapability = BrokerCapability {
    broker: "gateio",
    label: "Gate.io",
    website: "https://www.gate.com",
    docs_url: "https://www.gate.com/docs/developers/apiv4/en/#list-personal-trading-history",
    rate_limit: "200 requests per 10 seconds per account on most private endpoints. Fills \
                 page a week at a time, narrower in a busy week.",
    key_note: "An API v4 key with **read-only** access to Spot (and Unified Account if the \
               account runs in a unified mode) and to Perpetual Futures. Do not grant \
               read-write or Withdraw: this connector never writes. Gate.io has no call that \
               names a key's permissions, so the test cannot tell a read-only key from one \
               that could trade. BTC-settled and dated futures, margin and stop orders are \
               not read.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: GATEIO_FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // The futures book is a margin book: a short is a short. A spot-only account is worth
    // unticking "this account can go short" for, which the import modal offers.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static MEXC_FIELDS: &[ConfigField] = &[ConfigField {
    name: "books",
    label: "Books to read",
    placeholder: "spot,futures",
    kind: "text",
    required: false,
    help: "Which MEXC books this account reads: spot, futures (the perpetuals settled in \
           USDT, USDC or USD1), or both separated by a comma. Leave empty for both. Name spot \
           pairs as BTCUSDT and contracts as BTC_USDT: each book reads the names written \
           its way.",
}];

pub(crate) static MEXC: BrokerCapability = BrokerCapability {
    broker: "mexc",
    label: "MEXC",
    website: "https://www.mexc.com",
    docs_url: "https://mexcdevelop.github.io/apidocs/spot_v3_en/#account-trade-list",
    rate_limit: "500 requests per 10 seconds per account on spot, 20 per 2 seconds on \
                 futures. Spot fills come 100 per call, one pair at a time, narrower in a \
                 busy week; futures fills page 100 at a time, a month per window.",
    key_note: "An API key with the **read** permissions only: spot Account and Order read, \
               futures Order and Position read. Do not grant trading or withdrawals: this \
               connector never writes. MEXC serves **30 days** of spot fills over the API \
               (anything older is an export from the web site), one pair at a time, so the \
               pairs and contracts to pull are named. A key not bound to an IP address \
               expires after 90 days. Coin-settled futures are not read.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: MEXC_FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    // `myTrades` and `order_deals` answer one instrument at a time.
    needs_symbols: true,
    window_from_broker: false,
    history_days: 30,
    // The futures book is a margin book: a short is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

pub(crate) static BITFINEX: BrokerCapability = BrokerCapability {
    broker: "bitfinex",
    label: "Bitfinex",
    website: "https://www.bitfinex.com",
    docs_url: "https://docs.bitfinex.com/reference/rest-auth-trades",
    rate_limit: "90 calls per minute per authenticated read; fills come 1000 per call, every \
                 instrument at once.",
    key_note: "An API key with **read** access only to Account, Wallets, Orders, Positions \
               and History. Do not grant any write permission or withdrawals: this connector \
               never writes. One key reads the exchange, margin and funding wallets and both \
               books (spot pairs and perpetuals). Give OpenTraderWorld a key of its own: \
               Bitfinex refuses a key whose nonce another program has moved ahead.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    // `trades/hist` answers every instrument in one list.
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Margin and perpetual positions can be short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

pub(crate) static DERIBIT: BrokerCapability = BrokerCapability {
    broker: "deribit",
    label: "Deribit",
    website: "https://www.deribit.com",
    docs_url: "https://docs.deribit.com/api-reference/trading/private-get_user_trades_by_currency_and_time",
    rate_limit: "About 20 calls per second per sub-account (credit based); fills come 1000 \
                 per call, a currency and a kind at a time.",
    key_note: "An API key (Client Id and Client Secret) with the scopes **account:read** and \
               **trade:read** only. Do not grant trading or withdrawals: this connector never \
               writes. Options, linear (USDC) futures and spot fills are imported; fills of \
               the inverse contracts (BTC-PERPETUAL, BTC-27DEC26) are not, since their profit \
               counts in the coin. Their positions and orders are read.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    // Fills are read by currency and kind, not by instrument.
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Futures and options can be short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

pub(crate) static KUCOIN: BrokerCapability = BrokerCapability {
    broker: "kucoin",
    label: "KuCoin",
    website: "https://www.kucoin.com",
    docs_url: "https://www.kucoin.com/docs-new/rest/spot-trading/orders/get-trade-history",
    rate_limit: "4000 request weight per 30 seconds per account on the spot pool; a fill page \
                 costs 2. Fills are asked one pair and one week at a time.",
    key_note: "An API key with the **General** permission only, plus the passphrase you chose \
               when creating it. Do not grant Spot, Margin, Futures or Withdrawal: this \
               connector never writes, and its test says when a key could trade. KuCoin \
               answers spot fills one pair at a time, so name the pairs to pull. The margin \
               account is not read, and KuCoin Futures is its own broker.",
    required_secrets: &["api_key", "api_secret", "api_passphrase"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    // `hf/fills` answers one pair at a time.
    needs_symbols: true,
    window_from_broker: false,
    history_days: 0,
    spot_only: true,
    positions: false,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

pub(crate) static KUCOIN_FUTURES: BrokerCapability = BrokerCapability {
    broker: "kucoin_futures",
    label: "KuCoin Futures",
    website: "https://www.kucoin.com/futures",
    docs_url: "https://www.kucoin.com/docs-new/rest/futures-trading/orders/get-trade-history",
    rate_limit: "2000 request weight per 30 seconds per account on the futures pool; a fill \
                 page costs 5 and holds 1000 fills, a week at a time.",
    key_note: "An API key with the **General** permission only, plus its passphrase. Do not \
               grant Futures or Withdrawal: this connector never writes. USDT- and \
               USDC-margined contracts are read; coin-margined ones are not. Quantities are \
               contracts, valued by the contract size KuCoin publishes.",
    required_secrets: &["api_key", "api_secret", "api_passphrase"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // A futures book is a margin book: a short is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: false,
    quotes: false,
    testable: true,
};

static COINBASE_FIELDS: &[ConfigField] = &[ConfigField {
    name: "key_name",
    label: "API key name",
    placeholder: "organizations/{org_id}/apiKeys/{key_id}",
    kind: "text",
    required: true,
    help: "The full key name Coinbase shows next to the key, organizations/… included. It \
           is an identifier, not the secret, so it is stored in the clear and can be \
           checked against the dashboard.",
}];

pub(crate) static COINBASE: BrokerCapability = BrokerCapability {
    broker: "coinbase",
    label: "Coinbase Advanced Trade",
    website: "https://www.coinbase.com",
    docs_url: "https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-fills",
    rate_limit: "30 requests per second on the private REST endpoints; a fill page is one \
                 request and the walk pages by cursor.",
    key_note: "A CDP API key with **View** permission only, created as **Ed25519** (the type \
               Coinbase recommends). Do not grant Trade or Transfer: this connector never \
               writes.",
    required_secrets: &["api_private_key"],
    config_fields: COINBASE_FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    spot_only: true,
    positions: false,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

pub(crate) static KRAKEN: BrokerCapability = BrokerCapability {
    broker: "kraken",
    label: "Kraken",
    website: "https://www.kraken.com",
    docs_url: "https://docs.kraken.com/api/docs/rest-api/get-trade-history",
    rate_limit: "A private-call counter that decays over time; a history page costs 2, so \
                 a long period is walked with a pause between pages.",
    key_note: "An API key with **Query Ledger & Trade History** (and Query Open Orders) \
               only. No order permission is needed: this connector never writes.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    spot_only: true,
    positions: false,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static IBKR_FLEX_FIELDS: &[ConfigField] = &[
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

pub(crate) static IBKR_FLEX: BrokerCapability = BrokerCapability {
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
    config_fields: IBKR_FLEX_FIELDS,
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

static OANDA_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "001-004-1234567-001",
        kind: "text",
        required: true,
        help: "The v20 account number, as OANDA writes it on the account page. One OANDA \
               account per broker account here: the token may reach several, and which one \
               is read has to be said rather than picked.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or practice. fxTrade and fxPractice are different hosts with different \
               tokens and different account numbers, so this is stated, not guessed. Leave \
               empty for live.",
    },
];

pub(crate) static OANDA: BrokerCapability = BrokerCapability {
    broker: "oanda",
    label: "OANDA",
    website: "https://www.oanda.com",
    docs_url: "https://developer.oanda.com/rest-live-v20/transaction-ep/",
    rate_limit: "120 requests per second per token, 30 per second on the instrument list and \
                 the transaction id-range pages. A year of fills is a handful of calls.",
    key_note: "A personal access token from *Manage API Access*. **OANDA issues no read-only \
               token**: the same token that reads this account can also trade it, so treat it \
               as a full-access credential and revoke it when the account is removed. Nothing \
               here places, changes or cancels an order. Commission and the guaranteed-stop \
               fee are billed in the account's own currency, not the instrument's.",
    required_secrets: &["api_token"],
    config_fields: OANDA_FIELDS,
    // CURRENCY is forex; a CFD or a metal has no journal class of its own, so it is filed as
    // "other" rather than dressed up as one it is not.
    asset_classes: &["forex", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Margin venue: a sell with nothing open really is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static OKX_FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or demo. OKX's demo trading shares the host but needs its own key and an \
           extra header, so this is stated rather than guessed. Leave empty for live.",
}];

pub(crate) static OKX: BrokerCapability = BrokerCapability {
    broker: "okx",
    label: "OKX",
    website: "https://www.okx.com",
    docs_url: "https://www.okx.com/docs-v5/en/",
    rate_limit: "10 requests per 2 seconds per account on the fill archive, and the same on \
                 positions and orders. A pull walks five instrument types.",
    key_note: "An API key with **Read** permission only, plus the passphrase you chose when \
               creating it. Do not grant Trade or Withdraw: this connector never writes. \
               OKX serves **90 days** of fills; anything older is a download from the web \
               site. Derivative fills are counted in contracts, and the contract size is \
               read from OKX's own instrument list.",
    required_secrets: &["api_key", "api_secret", "api_passphrase"],
    config_fields: OKX_FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 90,
    // Margin, swaps and futures: a short is a short. A spot-only account is worth unticking
    // "this account can go short" for, which the import modal offers.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static TRADESTATION_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "11111111",
        kind: "text",
        required: true,
        help: "The TradeStation account number this broker account reads. One account per \
               entry: the key may reach several (cash, margin, futures) and which one is \
               read has to be said rather than picked.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or sim. Simulated trading is a different host and its own account \
               numbers. Leave empty for live.",
    },
];

pub(crate) static TRADESTATION: BrokerCapability = BrokerCapability {
    broker: "tradestation",
    label: "TradeStation",
    website: "https://www.tradestation.com",
    docs_url: "https://api.tradestation.com/docs/",
    rate_limit: "The access token is minted once every twenty minutes and shared. Orders \
                 page 600 at a time.",
    key_note: "An API key (client id and secret) plus the **refresh token** from a one-time \
               sign-in granting `ReadAccount`, `MarketData`, `openid` and `offline_access` \
               only. **Do not grant `Trade`**: this connector never writes, and a token that \
               could trade would be a standing risk for nothing. TradeStation serves 90 days \
               of closed orders; anything older is a statement from its own site.",
    required_secrets: &["client_id", "client_secret", "refresh_token"],
    config_fields: TRADESTATION_FIELDS,
    asset_classes: &["stock", "option", "future"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 90,
    // A margin or futures account can be short, and TradeStation reports it as such.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static FOREXCOM_FIELDS: &[ConfigField] = &[ConfigField {
    name: "trading_account_id",
    label: "Trading account id",
    placeholder: "400123456",
    kind: "text",
    required: false,
    help: "Which trading account to read, when the login has more than one. Leave empty and \
           the connector asks StoneX; if the login carries several, it lists them and asks \
           you to name one rather than picking.",
}];

pub(crate) static FOREXCOM: BrokerCapability = BrokerCapability {
    broker: "forexcom",
    label: "FOREX.com (StoneX)",
    website: "https://www.forex.com",
    docs_url: "https://docs.labs.gaincapital.com/",
    rate_limit: "500 requests per 5 seconds. The session token is minted once every twenty \
                 minutes and shared; trade history pages 200 rows at a time.",
    key_note: "Your account **username and password**, plus the AppKey StoneX issues once \
               its API terms are signed. **StoneX has no read-only credential**: this login \
               can also trade, so treat it as a full-access secret and change the password \
               when you remove the account. Nothing here places, changes or cancels an \
               order. FX and CFD trades are priced on the spread, so an imported trade \
               carries no commission.",
    required_secrets: &["username", "password", "app_key"],
    config_fields: FOREXCOM_FIELDS,
    // A currency pair is forex; an index, commodity or share CFD has no journal class of
    // its own, so it is filed as "other" rather than as one it is not.
    asset_classes: &["forex", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Margin venue: a sell with nothing open really is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    // Pricing a market needs its id, which a bare asset name does not carry; see the note
    // in the connector docs.
    quotes: false,
    testable: true,
};

static CAPITALCOM_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "12345678901234567",
        kind: "text",
        required: false,
        help: "Which financial account to read, when the login has more than one (Capital.com \
               opens one per currency). Leave empty to read the preferred account.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or demo. The demo account is a different host with its own key. Leave \
               empty for live.",
    },
];

pub(crate) static CAPITALCOM: BrokerCapability = BrokerCapability {
    broker: "capitalcom",
    label: "Capital.com",
    website: "https://capital.com",
    docs_url: "https://open-api.capital.com/",
    rate_limit: "10 requests per second, and one sign-in per second. The activity log answers \
                 24 hours at a time, so a long period costs one call per day.",
    key_note: "An API key from Settings → API integrations (two-factor authentication has to \
               be on to create one), plus your login and the **custom password** you set on \
               the key. Capital.com grades no key read-only, so this login can also trade: \
               treat it as a full-access secret. Nothing here places, changes or cancels an \
               order. Every instrument is a **CFD**, so a share CFD is filed as a derivative \
               rather than as the share, which is what a tax form wants.",
    required_secrets: &["api_key", "identifier", "api_password"],
    config_fields: CAPITALCOM_FIELDS,
    asset_classes: &["forex", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Margin venue: a sell with nothing open really is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static NINJATRADER_FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "123456",
        kind: "text",
        required: false,
        help: "Which trading account to read, when the login has more than one. Leave empty \
               and the connector asks; if the login carries several, it lists them and asks \
               you to name one rather than picking.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or demo. The simulation platform is a different host with its own \
               account numbers. Leave empty for live.",
    },
];

pub(crate) static NINJATRADER: BrokerCapability = BrokerCapability {
    broker: "ninjatrader",
    label: "NinjaTrader",
    website: "https://ninjatrader.com",
    docs_url: "https://docs.ninjatrader.com/api",
    rate_limit: "The token is minted once every ninety minutes and shared. **Two sessions \
                 per login**: a third closes the oldest, so a trading application on the \
                 same login and this connector are already both of them.",
    key_note: "Your platform username and password plus the API key pair (`cid` and `sec`) \
               from NinjaTrader's developer access. **There is no read-only key**: the same \
               credentials can trade, so treat them as a full-access secret. Nothing here \
               places, changes or cancels an order. The platform answers the fills its \
               session can see and publishes no depth limit, so check the oldest row the \
               first pull returns before relying on it for a tax year.",
    required_secrets: &["username", "password", "cid", "sec"],
    config_fields: NINJATRADER_FIELDS,
    asset_classes: &["future", "stock", "crypto", "option", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Futures: a short is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    // Prices are a separate entitlement on a separate socket; see the module note.
    quotes: false,
    testable: true,
};
