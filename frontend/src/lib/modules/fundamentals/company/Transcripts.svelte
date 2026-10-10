<script>
  // Earnings-call transcripts: the calls stored for this company, listed from the first
  // connected transcript provider (FMP, Alpha Vantage, Finnhub); a call's text is fetched
  // when it is opened, then kept.
  import Section from '../Section.svelte';
  import TranscriptReader from '../TranscriptReader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fundamentalsApi } from '../api.js';
  import { errorText } from '../deferred.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let list = $state([]);
  let open = $state(null);
  let error = $state('');
  let busy = $state(false);

  async function load() {
    list = await fundamentalsApi.transcripts(company.ticker);
    if (!open || !list.some((d) => d.id === open.id)) open = list[0] ?? null;
  }

  // `force` (the Refresh button) asks the providers that refused lately too.
  async function fetchList(force = false) {
    busy = true;
    error = '';
    try {
      await fundamentalsApi.loadTranscripts(company.ticker, force);
      await load();
    } catch (e) {
      error = errorText($t, e);
    }
    busy = false;
  }

  $effect(() => {
    load().then(() => {
      if (!list.length) fetchList();
    });
  });
</script>

<div class="wrap">
  <Section title={$t('fundamentals.tr.calls')}>
    {#snippet actions()}
      <Button size="sm" variant="ghost" icon="refresh-cw" disabled={busy} onclick={() => fetchList(true)}>{$t('fundamentals.refresh')}</Button>
    {/snippet}
    {#if error}<ErrorText {error} />{/if}
    <ul>
      {#each list as d (d.id)}
        <li>
          <button class="item" class:on={open?.id === d.id} onclick={() => (open = d)}>
            <span>{d.title.replace(company.name, '').replace('earnings call', '').trim() || d.title}</span>
            <span class="fd-muted">{d.filed_at}</span>
          </button>
        </li>
      {:else}
        {#if !busy && !error}<li class="fd-muted">{$t('fundamentals.none')}</li>{/if}
      {/each}
    </ul>
  </Section>
  {#if open}
    <TranscriptReader doc={open} />
  {/if}
</div>

<style>
  .wrap {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    gap: var(--fd-gap);
    align-items: start;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .item {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 0;
    border-left: 2px solid transparent;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .item:hover {
    background: var(--surface-2);
  }
  .item.on {
    background: color-mix(in srgb, var(--accent) 9%, var(--surface));
    border-left-color: var(--accent);
  }
  @media (max-width: 760px) {
    .wrap {
      grid-template-columns: 1fr;
    }
  }
</style>
