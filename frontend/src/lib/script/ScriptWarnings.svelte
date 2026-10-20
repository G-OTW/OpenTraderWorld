<script>
  // What one script run reported: compile errors, refused inputs, the runtime error that
  // stopped it, then the warnings grouped by feature (count and lines). An unknown `syminfo`
  // field offers the instrument settings, where the fact is set once for every script.
  import { t } from '$lib/i18n';
  import { diagText, groupWarnings, linesText } from './diag.js';
  import InstrumentMetaModal from './InstrumentMetaModal.svelte';

  let {
    warnings = [],
    error = null,
    diagnostics = [],
    inputErrors = [],
    onBricks = false,
    coords = null,
    // Called after the instrument facts are saved, so the caller reruns its scripts.
    onmeta
  } = $props();

  const tr = (k, p) => $t(k, p);
  const groups = $derived(groupWarnings(warnings));
  let metaOpen = $state(false);
  const errorCount = $derived(diagnostics.length + inputErrors.length + (error ? 1 : 0));
</script>

{#if errorCount}
  <h4>{$t('histviz.script.errors')}</h4>
  <ul class="err">
    {#each diagnostics as d, i (i)}<li>{diagText(tr, d)}</li>{/each}
    {#each inputErrors as e (e.key)}
      <li>{$t('histviz.script.inputError', { key: e.key, reason: $t(`histviz.script.input.${e.reason}`) })}</li>
    {/each}
    {#if error}
      <li>
        {diagText(tr, error)}
        {#if error.code === 'SyminfoUnknown' && coords?.ticker}
          <button type="button" class="mini" onclick={() => (metaOpen = true)}>{$t('histviz.instrument.title')}</button>
        {/if}
      </li>
    {/if}
  </ul>
{/if}

<h4>{$t('histviz.script.warnings')}</h4>
{#if groups.length}
  <ul>
    {#each groups as g (`${g.code}:${g.feature}`)}
      <li>
        {diagText((k, p) => (k === 'script.diag.atLine' ? '' : $t(k, p)), g.code === 'SkippedVisual' ? { ...g.diag, args: [g.feature, String(g.count)] } : g.diag)}
        {#if g.lines.length}<span class="lines">{linesText(tr, g.lines)}</span>{/if}
      </li>
    {/each}
  </ul>
{:else}
  <p class="muted">{$t('histviz.script.noIssues')}</p>
{/if}
{#if onBricks}<p class="muted">{$t('histviz.script.bricksNote')}</p>{/if}

{#if coords?.ticker}
  <InstrumentMetaModal bind:open={metaOpen} {coords} onsaved={onmeta} />
{/if}

<style>
  h4 {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
    font-weight: 500;
  }
  ul {
    margin: 0;
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
  }
  ul.err {
    color: var(--red);
  }
  .lines,
  .muted {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .lines {
    margin-left: var(--space-2);
  }
  .muted {
    margin: 0;
  }
  .mini {
    margin-left: var(--space-2);
    font-size: var(--text-xs);
    padding: 1px var(--space-2);
  }
</style>
