<script>
  import { t } from '$lib/i18n';
  // Credits: data sources and upstream projects each module relies on. Provider
  // attributions that a source's terms require to appear on the data itself (e.g.
  // Dataroma, TradingView) stay inside their module; this page is the central,
  // plain-view credit list. Static — edit the list below when a source changes.
  // Grouped by kind of data, not by module: the same market-data providers feed several
  // modules (quotes, watchlist, historical download), so a per-module split duplicated most
  // of the list. Market data lists every connector the platform can pull from, whether or
  // not you've configured one.
  const credits = [
    {
      group: 'Market data',
      sources: [
        { name: 'Yahoo Finance', url: 'https://finance.yahoo.com' },
        { name: 'Binance', url: 'https://www.binance.com' },
        { name: 'Kraken', url: 'https://www.kraken.com' },
        { name: 'Coinbase', url: 'https://www.coinbase.com' },
        { name: 'CoinGecko', url: 'https://www.coingecko.com' },
        { name: 'Alpha Vantage', url: 'https://www.alphavantage.co' },
        { name: 'EODHD', url: 'https://eodhd.com' },
        { name: 'Alpaca', url: 'https://alpaca.markets' },
        { name: 'Massive (Polygon.io)', url: 'https://massive.com' }
      ]
    },
    {
      group: 'Economic calendar',
      sources: [{ name: 'TradingView', url: 'https://www.tradingview.com' }]
    },
    {
      group: 'Investor portfolios',
      sources: [{ name: 'Dataroma', url: 'https://www.dataroma.com' }]
    },
    {
      group: 'Instrument reference',
      sources: [
        { name: 'FinanceDatabase (JerBouma)', url: 'https://github.com/JerBouma/FinanceDatabase' }
      ]
    },
    {
      group: 'FX rates',
      sources: [
        { name: 'Frankfurter', url: 'https://frankfurter.dev' },
        { name: 'open.er-api', url: 'https://www.exchangerate-api.com' }
      ]
    }
  ];
</script>

<div class="section">
  <div class="head">
    <h2>{$t('settings.credits.title')}</h2>
  </div>
  <p class="muted small">{$t('settings.credits.subtitle')}</p>

  <table class="tbl">
    <thead>
      <tr><th>{$t('settings.credits.colGroup')}</th><th>{$t('settings.credits.colSource')}</th></tr>
    </thead>
    <tbody>
      {#each credits as c (c.group)}
        {#each c.sources as s, i (s.name)}
          <tr>
            {#if i === 0}
              <td rowspan={c.sources.length} class="mod">{c.group}</td>
            {/if}
            <td>
              <a href={s.url} target="_blank" rel="noopener noreferrer">{s.url}</a>
              <span class="name">— {s.name}</span>
            </td>
          </tr>
        {/each}
      {/each}
    </tbody>
  </table>
</div>

<style>
  .section {
    max-width: 760px;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: var(--text-base);
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
    margin-top: var(--space-3);
    font-size: var(--text-base);
  }
  .tbl th,
  .tbl td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: var(--hairline) solid var(--border);
    vertical-align: top;
  }
  .tbl th {
    color: var(--muted);
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
  }
  .mod {
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .tbl a {
    color: var(--text);
    text-decoration: underline;
    word-break: break-all;
  }
  .tbl a:hover {
    text-decoration: underline;
  }
  .name {
    color: var(--muted);
    margin-left: var(--space-1);
  }
</style>
