<script>
  // A saved version of a strategy, read-only: what each wizard step said at the time, with
  // the way to restore it. Open while `version` is set.
  // props: version (full strategy version | null), onclose(), onrestore(version)
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { migrateSettings } from './api.js';
  import { strategySummary, filtersSummary, sizingSummary, advancedSummary } from './summary.js';
  import { fmtVersionDate } from '$lib/versioning/state.svelte.js';
  import { t } from '$lib/i18n';

  let { version = null, onclose = () => {}, onrestore = () => {} } = $props();

  let open = $state(false);
  $effect(() => {
    open = !!version;
  });

  const steps = $derived.by(() => {
    if (!version) return [];
    const s = migrateSettings(version.settings);
    return [
      ['backtest.step.strategy', strategySummary(s, $t)],
      ['backtest.step.filters', filtersSummary(s, $t)],
      ['backtest.step.sizing', sizingSummary(s, $t)],
      ['backtest.step.advanced', advancedSummary(s, $t)]
    ];
  });
</script>

<Modal bind:open title={version?.name ?? ''} {onclose}>
  {#if version}
    <p class="meta">
      {fmtVersionDate(version.created_at)}{#if version.note}<span> · {version.note}</span>{/if}
    </p>
    {#if version.tags?.length}
      <p class="tags">{#each version.tags as tag (tag)}<span class="tag">{tag}</span>{/each}</p>
    {/if}
    <dl>
      {#each steps as [key, text] (key)}
        <dt>{$t(key)}</dt>
        <dd>{text}</dd>
      {/each}
    </dl>
  {/if}

  {#snippet footer()}
    <button class="ghost" onclick={() => { open = false; onclose(); }}>{$t('common.close')}</button>
    <button class="primary" onclick={() => onrestore(version)}>
      <Icon name="rotate-ccw" size={12} /> {$t('versioning.restore')}
    </button>
  {/snippet}
</Modal>

<style>
  .meta {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .tags {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
    margin: 0 0 var(--space-3);
  }
  .tag {
    font-size: var(--text-xs);
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    padding: 1px var(--space-2);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0;
    font-size: var(--text-sm);
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    color: var(--text);
  }
</style>
