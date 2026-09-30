<script>
  // Provider + model chooser. Plain panel, no chrome of its own: the Agent page hands it to
  // a modal, the floating assistant renders it as a view over its own thread (`compact`,
  // one column). Nothing is applied until "Use this model" — a click in the list is a draft.
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { agentApi } from '$lib/modules/agent/api.js';

  let {
    providers = [],
    providerId = null, // current override, null/'' = inherit
    model = '', // current override, '' = inherit
    effective = '', // what is in force while inheriting
    scope = '', // one line saying who this choice applies to
    compact = false,
    onapply = () => {},
    oncancel = () => {}
  } = $props();

  let draftProv = $state('');
  let draftModel = $state('');
  let search = $state('');
  let models = $state([]);
  let modelsErr = $state('');
  let loading = $state(false);

  const enabled = $derived(providers.filter((p) => p.enabled));
  const overridden = $derived(!!(providerId || model));
  const draftRow = $derived(providers.find((p) => p.id === draftProv) ?? null);
  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return (q ? models.filter((m) => m.toLowerCase().includes(q)) : models).slice(0, 200);
  });

  // Seeded once: the panel is created fresh each time it is shown, so the drafts never
  // fight the props while the user types.
  onMount(() => {
    draftProv = providerId || '';
    draftModel = model || '';
    if (draftProv) loadModels(draftProv);
  });

  async function loadModels(id) {
    models = [];
    modelsErr = '';
    const p = providers.find((x) => x.id === id);
    if (!p) return;
    if (!p.has_key) {
      modelsErr = $t('agent.set.noKey');
      return;
    }
    loading = true;
    try {
      models = await agentApi.listProviderModels(id);
    } catch (e) {
      modelsErr = e.message;
    } finally {
      loading = false;
    }
  }

  function pickProvider(id) {
    if (draftProv === id) return;
    draftProv = id;
    draftModel = '';
    search = '';
    models = [];
    modelsErr = '';
    if (id) loadModels(id);
  }

  function apply() {
    onapply({ provider_id: draftProv || null, model: draftProv ? draftModel.trim() : '' });
  }

  /** Drop the override entirely and follow the settings again. */
  function useDefault() {
    onapply({ provider_id: null, model: '' });
  }
</script>

<div class="mp" class:compact>
  {#if scope}<p class="mp-scope">{scope}</p>{/if}

  <div class="mp-cols">
    <div class="mp-rail">
      <button class="mp-prov" class:sel={!draftProv} onclick={() => pickProvider('')}>
        <span class="mp-name">{$t('agent.pick.inherit')}</span>
        {#if effective}<span class="mp-meta">{effective}</span>{/if}
        {#if !draftProv}<span class="mp-check"><Icon name="check" size={13} /></span>{/if}
      </button>
      {#each enabled as p (p.id)}
        <button class="mp-prov" class:sel={draftProv === p.id} onclick={() => pickProvider(p.id)}>
          <span class="mp-name">{p.label || p.kind}</span>
          {#if !p.has_key}<span class="mp-warn">{$t('agent.set.noKey')}</span>{/if}
          {#if draftProv === p.id}<span class="mp-check"><Icon name="check" size={13} /></span>{/if}
        </button>
      {/each}
    </div>

    <div class="mp-main">
      {#if !draftProv}
        <div class="mp-blank">
          <Icon name="zap" size={20} />
          <p>{$t('agent.pick.pickProvider')}</p>
        </div>
      {:else}
        <div class="mp-find">
          <span class="mp-find-ico"><Icon name="search" size={13} /></span>
          <input
            type="text"
            autocomplete="off"
            placeholder={$t('agent.pick.modelSearch')}
            aria-label={$t('agent.pick.modelSearch')}
            bind:value={search}
          />
        </div>
        <div class="mp-list">
          {#if loading}
            <p class="mp-hint">{$t('common.loading')}</p>
          {:else if modelsErr}
            <p class="mp-hint">{$t('agent.pick.noModels')}</p>
          {:else if !shown.length}
            <p class="mp-hint">{$t('common.noMatches')}</p>
          {:else}
            {#each shown as m (m)}
              <button
                class="mp-model"
                class:sel={m === draftModel.trim()}
                title={m}
                onclick={() => (draftModel = m)}
                ondblclick={apply}
              >
                <span class="mp-model-name">{m}</span>
                {#if m === draftModel.trim()}<Icon name="check" size={13} />{/if}
              </button>
            {/each}
          {/if}
        </div>
        <label class="mp-custom">
          <span>{$t('agent.pick.custom')}</span>
          <input
            type="text"
            autocomplete="off"
            placeholder={draftRow?.default_model || $t('agent.set.modelPh')}
            bind:value={draftModel}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                apply();
              }
            }}
          />
        </label>
      {/if}
    </div>
  </div>

  <div class="mp-actions">
    {#if overridden}
      <span class="mp-left">
        <Button size="sm" variant="ghost" onclick={useDefault}>{$t('agent.pick.useDefault')}</Button>
      </span>
    {/if}
    <Button size="sm" variant="ghost" onclick={oncancel}>{$t('common.cancel')}</Button>
    <Button size="sm" variant="primary" icon="check" onclick={apply}>{$t('agent.pick.apply')}</Button>
  </div>
</div>

<style>
  .mp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .mp.compact {
    height: 100%;
    gap: var(--space-2);
  }
  .mp-scope {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .mp-cols {
    display: grid;
    grid-template-columns: 200px 1fr;
    gap: var(--space-3);
    height: 340px;
  }
  /* One column in the assistant panel: 400px cannot hold a rail and a list side by side. */
  .mp.compact .mp-cols {
    grid-template-columns: 1fr;
    grid-template-rows: auto 1fr;
    gap: var(--space-2);
    height: auto;
    flex: 1;
    min-height: 0;
  }

  /* Providers */
  .mp-rail {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    padding-right: var(--space-1);
    border-right: var(--hairline) solid var(--border);
  }
  .mp.compact .mp-rail {
    max-height: 108px;
    padding: 0 0 var(--space-2);
    border-right: none;
    border-bottom: var(--hairline) solid var(--border);
  }
  .mp-prov {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 2px var(--space-2);
    padding: var(--space-2);
    border: var(--hairline) solid transparent;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .mp-prov:hover {
    background: var(--surface-2);
  }
  .mp-prov.sel {
    border-color: var(--border-control);
    background: var(--surface-2);
  }
  .mp-name {
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mp-meta,
  .mp-warn {
    grid-column: 1;
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mp-warn {
    color: var(--amber);
  }
  .mp-check {
    grid-row: 1;
    grid-column: 2;
    display: inline-flex;
    color: var(--accent);
  }

  /* Models */
  .mp-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
  }
  .mp-find {
    position: relative;
  }
  .mp-find-ico {
    position: absolute;
    left: var(--space-2);
    top: 50%;
    transform: translateY(-50%);
    color: var(--muted);
    display: inline-flex;
  }
  .mp-find input {
    width: 100%;
    height: 30px;
    padding: 0 var(--space-2) 0 26px;
    border: var(--hairline) solid var(--border-control);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
  }
  .mp-find input:focus,
  .mp-custom input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .mp-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .mp-model {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border: var(--hairline) solid transparent;
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .mp-model:hover {
    background: var(--surface-2);
  }
  .mp-model.sel {
    border-color: var(--border-control);
    background: var(--surface-2);
    color: var(--accent);
  }
  .mp-model-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mp-hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .mp-blank {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--muted);
    text-align: center;
  }
  .mp-blank p {
    margin: 0;
    font-size: var(--text-sm);
  }
  .mp-custom {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .mp-custom input {
    height: 30px;
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
  }

  .mp-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  /* Pushes the "follow settings" escape hatch away from the confirming pair; in the narrow
     panel it takes its own row instead of squeezing them. */
  .mp-left {
    margin-right: auto;
  }
  .mp.compact .mp-left {
    width: 100%;
    margin-right: 0;
  }

  @media (max-width: 560px) {
    .mp-cols {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr;
      height: auto;
    }
    .mp-rail {
      max-height: 140px;
      border-right: none;
      border-bottom: var(--hairline) solid var(--border);
      padding-bottom: var(--space-2);
    }
    .mp-list {
      max-height: 200px;
    }
  }
</style>
