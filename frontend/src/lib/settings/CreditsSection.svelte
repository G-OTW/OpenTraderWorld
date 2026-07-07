<script>
  import { t } from '$lib/i18n';
  // Credits: data sources and upstream projects each module relies on. Provider
  // attributions that a source's terms require to appear on the data itself (e.g.
  // Dataroma, TradingView) stay inside their module; this page is the central,
  // plain-view credit list. Static — edit the list below when a source changes.
  const credits = [
    {
      module: 'FinanceDatabase',
      sources: [
        { name: 'FinanceDatabase (JerBouma)', url: 'https://github.com/JerBouma/FinanceDatabase' }
      ]
    },
    {
      module: 'Economic Calendar',
      sources: [{ name: 'TradingView', url: 'https://www.tradingview.com' }]
    },
    {
      module: "Managers' Portfolios",
      sources: [{ name: 'Dataroma', url: 'https://www.dataroma.com' }]
    },
    {
      module: 'Portfolio Tracker',
      sources: [
        { name: 'CoinGecko', url: 'https://www.coingecko.com' },
        { name: 'Yahoo Finance', url: 'https://finance.yahoo.com' },
        { name: 'Binance', url: 'https://www.binance.com' },
        { name: 'Kraken', url: 'https://www.kraken.com' },
        { name: 'Coinbase', url: 'https://www.coinbase.com' }
      ]
    },
    {
      module: 'Trading Journal — FX',
      sources: [
        { name: 'Frankfurter', url: 'https://frankfurter.dev' },
        { name: 'open.er-api', url: 'https://www.exchangerate-api.com' }
      ]
    },
    {
      // All connectors the module can download from, whether or not you've configured one.
      module: 'Historical Data',
      sources: [
        { name: 'Yahoo Finance', url: 'https://finance.yahoo.com' },
        { name: 'Binance', url: 'https://www.binance.com' },
        { name: 'Coinbase', url: 'https://www.coinbase.com' },
        { name: 'Kraken', url: 'https://www.kraken.com' },
        { name: 'Alpha Vantage', url: 'https://www.alphavantage.co' },
        { name: 'EODHD', url: 'https://eodhd.com' },
        { name: 'Alpaca', url: 'https://alpaca.markets' },
        { name: 'Massive (Polygon.io)', url: 'https://massive.com' }
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
      <tr><th>{$t('settings.credits.colModule')}</th><th>{$t('settings.credits.colSource')}</th></tr>
    </thead>
    <tbody>
      {#each credits as c (c.module)}
        {#each c.sources as s, i (s.name)}
          <tr>
            {#if i === 0}
              <td rowspan={c.sources.length} class="mod">{c.module}</td>
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
    color: var(--muted);
  }
  .small {
    font-size: 0.85rem;
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
    margin-top: var(--space-3);
    font-size: 0.9rem;
  }
  .tbl th,
  .tbl td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
  .tbl th {
    color: var(--muted);
    font-weight: 600;
    font-size: 0.8rem;
  }
  .mod {
    font-weight: 600;
    color: var(--text);
    white-space: nowrap;
  }
  .tbl a {
    color: var(--accent);
    text-decoration: none;
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
