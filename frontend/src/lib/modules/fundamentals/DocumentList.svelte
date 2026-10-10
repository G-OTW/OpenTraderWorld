<script>
  // Filings and transcripts in one table, with a form-type filter. Used per company and
  // on the cross-company Documents page (`showTicker`).
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // props: docs, showTicker, onopen (called with a transcript row)
  let { docs = [], showTicker = false, onopen } = $props();

  let form = $state('');
  const forms = $derived([...new Set(docs.map((d) => (d.kind === 'transcript' ? 'transcript' : d.form)))].sort());
  const shown = $derived(docs.filter((d) => !form || (d.kind === 'transcript' ? form === 'transcript' : d.form === form)));
</script>

<div class="fd-row filters">
  <button class="chip" class:active={!form} onclick={() => (form = '')}>{$t('fundamentals.all')}</button>
  {#each forms as f (f)}
    <button class="chip" class:active={form === f} onclick={() => (form = f)}>{f === 'transcript' ? $t('fundamentals.tr.transcript') : f}</button>
  {/each}
</div>

<div class="fd-scroll">
  <table class="tbl">
    <thead>
      <tr>
        <th>{$t('fundamentals.date')}</th>
        {#if showTicker}<th>{$t('fundamentals.ticker')}</th>{/if}
        <th>{$t('fundamentals.doc.form')}</th>
        <th>{$t('fundamentals.doc.title')}</th>
        <th>{$t('fundamentals.doc.source')}</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each shown as d (d.id)}
        <tr>
          <td>{d.filed_at}</td>
          {#if showTicker}<td>{d.ticker}</td>{/if}
          <td><Badge tone={d.kind === 'transcript' ? 'accent' : 'neutral'}>{d.kind === 'transcript' ? $t('fundamentals.tr.transcript') : d.form}</Badge></td>
          <td>{d.title}</td>
          <td class="fd-muted">{d.source}</td>
          <td class="act">
            {#if d.kind === 'transcript'}
              {#if onopen}<button class="link" onclick={() => onopen(d)}>{$t('fundamentals.doc.read')}</button>{/if}
            {:else}
              <a href={d.url} target="_blank" rel="noopener noreferrer" title={$t('fundamentals.doc.openSource')}><Icon name="external-link" size={14} /></a>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .filters {
    margin-bottom: var(--space-3);
  }
  .act {
    text-align: right;
    white-space: nowrap;
  }
  .act a {
    color: var(--muted);
  }
</style>
