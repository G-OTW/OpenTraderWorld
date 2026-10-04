<script>
  // `agent` block: one model turn, no conversation and no tools.
  //
  // The persona, provider and model come from the Agent module, so a workflow inherits
  // whatever the user configured there. The output contract is the field that matters:
  // a block after this one needs fields, not prose.
  import { onMount } from 'svelte';
  import Select from '$lib/ui/Select.svelte';
  import ExprField from './ExprField.svelte';
  import { agentApi } from '$lib/modules/agent/api.js';
  import { promptsApi } from '$lib/modules/prompt-store/api.js';
  import { t } from '$lib/i18n';

  let { config = $bindable({}), sources = [] } = $props();

  let agents = $state([]);
  let prompts = $state([]);

  onMount(async () => {
    // Best effort: the block still works with the default agent if either list fails.
    try {
      agents = await agentApi.listAgents();
    } catch {
      agents = [];
    }
    try {
      prompts = await promptsApi.list();
    } catch {
      prompts = [];
    }
  });

  const agentOptions = $derived([
    { value: '', label: $t('automator.agent.defaultAgent') },
    ...agents.map((a) => ({ value: a.id, label: a.name }))
  ]);
  const promptOptions = $derived([
    { value: '', label: $t('automator.agent.customPrompt') },
    ...prompts.map((p) => ({ value: p.id, label: p.name }))
  ]);
</script>

<div class="stack">
  <div class="row">
    <Select label={$t('automator.agent.agent')} options={agentOptions} bind:value={config.agent_id} />
    <Select
      label={$t('automator.agent.output')}
      options={[
        { value: 'text', label: $t('automator.agent.outputText') },
        { value: 'json', label: $t('automator.agent.outputJson') }
      ]}
      bind:value={config.output}
    />
  </div>

  <!-- The prompt library grows without bound, so this one always carries its search
       field rather than waiting for the option count to cross a threshold. -->
  <Select
    label={$t('automator.agent.prompt')}
    options={promptOptions}
    bind:value={config.prompt_id}
    searchable={true}
    searchPlaceholder={$t('automator.agent.promptSearch')}
  />

  {#if !config.prompt_id}
    <ExprField
      label={$t('automator.agent.system')}
      bind:value={config.system}
      rows={3}
      {sources}
      hint={$t('automator.agent.systemHint')}
    />
  {/if}

  <ExprField label={$t('automator.agent.input')} bind:value={config.input} rows={5} {sources} />

  {#if config.output === 'json'}
    <ExprField
      label={$t('automator.agent.schema')}
      bind:value={config.schema}
      rows={4}
      hint={$t('automator.agent.schemaHint')}
      sources={[]}
    />
    <p class="hint">{$t('automator.agent.jsonHint')}</p>
  {/if}

  <p class="hint">{$t('automator.agent.costHint')}</p>
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
  }
  .row :global(> *) {
    flex: 1;
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
