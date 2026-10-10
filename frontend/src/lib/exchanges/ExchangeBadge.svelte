<script>
  // Top bar: the home exchange's three letters and a dot, green while it trades.
  import { t } from '$lib/i18n';
  import { homeExchange } from './home.svelte.js';

  const info = $derived(homeExchange.info);
  const label = $derived(
    info ? `${info.name}: ${$t(info.open ? 'exchanges.open' : 'exchanges.closed')}` : ''
  );
</script>

{#if info}
  <a class="xch" href="/settings#exchanges" title={label} aria-label={label}>
    <span class="dot" class:open={info.open}></span>{info.code}
  </a>
{/if}

<style>
  .xch {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: var(--muted);
    text-decoration: none;
    padding: 2px 6px;
  }
  .xch:hover {
    color: var(--text);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    opacity: 0.6;
  }
  .dot.open {
    background: var(--green);
    opacity: 1;
  }
</style>
