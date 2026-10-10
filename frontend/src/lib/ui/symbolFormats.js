/**
 * How each provider spells an instrument.
 *
 * One table for every box where a symbol is typed by hand: the download form, the chart's
 * instrument search, the journal's contract map, a broker's symbol filter. The examples are
 * the shape the provider's own API accepts, taken from what the connectors actually send
 * (`core/otw-core/src/histdata/`, `core/otw-core/src/brokers/`), never a plausible guess: a
 * symbol that cannot be resolved is an error naming the fix, not a best-effort match.
 *
 * `hint` is an i18n key under `symbolHelp.hint.`, and the hints are shared between providers,
 * so a rule is translated once. `provider` matches both a connector's `provider` and a
 * broker account's `broker` (see `ALIASES` for the few that differ).
 */
export const SYMBOL_FORMATS = [
  {
    provider: 'binance',
    label: 'Binance',
    docs: 'https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints',
    rows: [{ asset: 'crypto', examples: ['BTCUSDT', 'ETHUSDT', 'SOLUSDC'], hint: 'concat' }]
  },
  {
    provider: 'binance_futures',
    label: 'Binance USDⓈ-M Futures',
    docs: 'https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api',
    rows: [{ asset: 'crypto', examples: ['BTCUSDT', 'ETHUSDT', 'ETHUSDT_250926'], hint: 'binanceFutures' }]
  },
  {
    provider: 'bitget',
    label: 'Bitget',
    docs: 'https://www.bitget.com/docs/classic/catalog',
    rows: [{ asset: 'crypto', examples: ['BTCUSDT', 'ETHUSDT', 'SOLUSDC'], hint: 'concat' }]
  },
  {
    provider: 'okx',
    label: 'OKX',
    docs: 'https://www.okx.com/docs-v5/en/',
    rows: [{ asset: 'crypto', examples: ['BTC-USDT', 'BTC-USDT-SWAP', 'BTC-USD-241227'], hint: 'okxDash' }]
  },
  {
    provider: 'oanda',
    label: 'OANDA',
    docs: 'https://developer.oanda.com/rest-live-v20/pricing-ep/',
    rows: [{ asset: 'fx', examples: ['EUR_USD', 'XAU_USD', 'SPX500_USD'], hint: 'oandaUnderscore' }]
  },
  {
    provider: 'kraken',
    label: 'Kraken',
    docs: 'https://docs.kraken.com/api/docs/rest-api/get-ohlc-data',
    rows: [{ asset: 'crypto', examples: ['XBTUSD', 'ETHUSD', 'SOLEUR'], hint: 'xbt' }]
  },
  {
    provider: 'coinbase',
    label: 'Coinbase',
    docs: 'https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproductcandles',
    rows: [{ asset: 'crypto', examples: ['BTC-USD', 'ETH-EUR', 'SOL-USD'], hint: 'dash' }]
  },
  {
    provider: 'alpaca',
    label: 'Alpaca',
    docs: 'https://docs.alpaca.markets/reference/stockbars',
    rows: [
      { asset: 'equity', examples: ['AAPL', 'MSFT'], hint: 'plain' },
      { asset: 'etf', examples: ['SPY', 'QQQ'], hint: 'plain' },
      { asset: 'crypto', examples: ['BTC/USD', 'ETH/USD'], hint: 'slash' },
      { asset: 'option', examples: ['SPY251219C00650000'], hint: 'occ' }
    ]
  },
  {
    provider: 'yahoo',
    label: 'Yahoo Finance',
    docs: 'https://finance.yahoo.com',
    rows: [
      { asset: 'equity', examples: ['AAPL', 'AIR.PA', 'SAP.DE'], hint: 'suffixYahoo' },
      { asset: 'etf', examples: ['SPY', 'CW8.PA'], hint: 'suffixYahoo' },
      { asset: 'index', examples: ['^GSPC', '^FCHI'], hint: 'caretIndex' },
      { asset: 'crypto', examples: ['BTC-USD', 'ETH-EUR'], hint: 'dash' },
      { asset: 'fx', examples: ['EURUSD=X'], hint: 'yahooFx' }
    ]
  },
  {
    provider: 'eodhd',
    label: 'EODHD',
    docs: 'https://eodhd.com/financial-apis/',
    rows: [
      { asset: 'equity', examples: ['AAPL.US', 'AIR.PA'], hint: 'suffixEodhd' },
      { asset: 'etf', examples: ['SPY.US', 'CW8.PA'], hint: 'suffixEodhd' },
      { asset: 'crypto', examples: ['BTC-USD.CC'], hint: 'suffixEodhd' },
      { asset: 'fx', examples: ['EURUSD.FOREX'], hint: 'suffixEodhd' }
    ]
  },
  {
    provider: 'alphavantage',
    label: 'Alpha Vantage',
    docs: 'https://www.alphavantage.co/documentation/',
    rows: [
      { asset: 'equity', examples: ['AAPL', 'IBM', 'RELIANCE.BSE'], hint: 'avSymbol' },
      { asset: 'etf', examples: ['SPY', 'QQQ'], hint: 'plain' }
    ]
  },
  {
    provider: 'massive',
    label: 'Massive (Polygon.io)',
    docs: 'https://massive.com/docs/rest/quickstart',
    rows: [
      { asset: 'equity', examples: ['AAPL', 'MSFT'], hint: 'plain' },
      { asset: 'etf', examples: ['SPY', 'QQQ'], hint: 'plain' },
      { asset: 'crypto', examples: ['BTCUSD', 'ETHUSD'], hint: 'massivePrefix' },
      { asset: 'fx', examples: ['EURUSD'], hint: 'massivePrefix' },
      { asset: 'index', examples: ['SPX', 'NDX'], hint: 'massivePrefix' },
      { asset: 'option', examples: ['SPY251219C00650000'], hint: 'occ' },
      { asset: 'future', examples: ['GCJ5', 'ESU5'], hint: 'monthCode' }
    ]
  },
  {
    provider: 'tradestation',
    label: 'TradeStation',
    docs: 'https://api.tradestation.com/docs/',
    rows: [
      { asset: 'equity', examples: ['AAPL', 'MSFT'], hint: 'plain' },
      { asset: 'etf', examples: ['SPY', 'QQQ'], hint: 'plain' },
      { asset: 'index', examples: ['$SPX.X', '$INDU'], hint: 'tsIndex' },
      { asset: 'future', examples: ['@ES', 'ESH26'], hint: 'tsFuture' },
      { asset: 'option', examples: ['MSFT 260116C400'], hint: 'tsOption' }
    ]
  },
  {
    provider: 'forexcom',
    label: 'FOREX.com (StoneX)',
    docs: 'https://docs.labs.gaincapital.com/',
    rows: [{ asset: 'fx', examples: ['EUR/USD', 'GBP/JPY', '401484347'], hint: 'stonexMarket' }]
  },
  {
    provider: 'capitalcom',
    label: 'Capital.com',
    docs: 'https://open-api.capital.com/',
    rows: [
      { asset: 'fx', examples: ['EURUSD', 'GBPJPY'], hint: 'capitalEpic' },
      { asset: 'index', examples: ['US500', 'DE40'], hint: 'capitalEpic' },
      { asset: 'equity', examples: ['AAPL', 'TSLA'], hint: 'capitalEpic' },
      { asset: 'crypto', examples: ['BTCUSD', 'ETHUSD'], hint: 'capitalEpic' }
    ]
  },
  {
    provider: 'ninjatrader',
    label: 'NinjaTrader',
    docs: 'https://docs.ninjatrader.com/api',
    rows: [{ asset: 'future', examples: ['ESH6', 'MNQM6', 'CLZ6'], hint: 'monthCode' }]
  },
  {
    provider: 'ibkr',
    label: 'Interactive Brokers',
    docs: 'https://www.interactivebrokers.com/campus/ibkr-api-page/twsapi-doc/',
    rows: [
      { asset: 'equity', examples: ['AAPL', 'SAN:EUR', '7203@TSEJ:JPY'], hint: 'ibkrStock' },
      { asset: 'etf', examples: ['SPY', 'CW8@SBF:EUR'], hint: 'ibkrStock' },
      { asset: 'crypto', examples: ['BTC', 'ETH'], hint: 'ibkrCrypto' },
      { asset: 'fx', examples: ['EURUSD', 'EUR.USD'], hint: 'ibkrFx' },
      { asset: 'future', examples: ['ES.202512@CME', 'MNQU6', 'ES@CME'], hint: 'ibkrFuture' },
      { asset: 'option', examples: ['SPY251219C00650000'], hint: 'occ' }
    ]
  }
];

/** Broker and connector keys that name the same house under two ids. */
const ALIASES = { ibkr_flex: 'ibkr', ib: 'ibkr', polygon: 'massive' };

/** The table entry for a connector `provider` / broker `broker`, or null. */
export function formatFor(provider) {
  if (!provider) return null;
  const key = ALIASES[provider] ?? provider;
  return SYMBOL_FORMATS.find((f) => f.provider === key) ?? null;
}
