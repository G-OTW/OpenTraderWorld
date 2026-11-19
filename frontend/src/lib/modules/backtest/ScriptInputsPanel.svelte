<script>
  // The input values of every script a condition reads, one collapsible block per script.
  // They belong to the script, not to an operand: two conditions on one script read the same
  // values. Stored in `settings.scripts[id].inputs`, where the run reads them; a refused value
  // is shown next to its field, and the server refuses the run with it too.
  import ScriptInputs from '$lib/script/ScriptInputs.svelte';
  import { usedCustomIndicatorIds } from './embed.js';
  import { t } from '$lib/i18n';

  // `library` = the custom-indicator rows `[{ id, name, kind, meta }]` (`meta` = ScriptMeta).
  let { settings = $bindable(), library = [] } = $props();

  const used = $derived(
    usedCustomIndicatorIds(settings)
      .map((id) => library.find((r) => r.id === id && r.kind === 'script'))
      .filter(Boolean)
  );

  function setInputs(row, inputs) {
    const prev = settings.scripts?.[row.id];
    settings.scripts = {
      ...(settings.scripts ?? {}),
      [row.id]: { name: prev?.name ?? row.name, source: prev?.source ?? '', inputs }
    };
  }
</script>

{#if used.length}
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.script.inputs')}</span>
    <p class="bt-note">{$t('backtest.script.inputsHint')}</p>
    {#each used as row (row.id)}
      <details open={used.length === 1}>
        <summary>{row.name}</summary>
        <div class="body">
          <ScriptInputs
            inputs={row.meta?.inputs ?? []}
            bind:values={() => settings.scripts?.[row.id]?.inputs ?? {}, (v) => setInputs(row, v)}
          />
        </div>
      </details>
    {/each}
  </div>
{/if}

<style>
  summary {
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-1) 0;
  }
  .body {
    padding: var(--space-2) 0 var(--space-3);
  }
</style>
