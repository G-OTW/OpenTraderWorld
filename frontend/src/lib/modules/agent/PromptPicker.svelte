<script>
  // Prompt Store browser. Plain panel: a modal on the Agent page, a view over its own thread
  // in the floating assistant (`compact`, preview stacked under the list). Nothing lands in
  // the composer until "Insert prompt" — or a double-click on a row.
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { promptsApi } from '$lib/modules/prompt-store/api.js';

  let { compact = false, oninsert = () => {}, oncancel = () => {} } = $props();

  let prompts = $state(null); // null until loaded
  let err = $state('');
  let q = $state('');
  let selId = $state(null);

  const sel = $derived(prompts?.find((p) => p.id === selId) ?? null);
  const shown = $derived.by(() => {
    if (!prompts) return [];
    const needle = q.trim().toLowerCase();
    if (!needle) return prompts;
    return prompts.filter(
      (p) =>
        p.name.toLowerCase().includes(needle) ||
        (p.tags ?? []).some((tg) => tg.toLowerCase().includes(needle)) ||
        (p.body ?? '').toLowerCase().includes(needle)
    );
  });

  onMount(async () => {
    try {
      prompts = await promptsApi.list();
    } catch (e) {
      err = e.message;
      prompts = [];
    }
  });

  function insert() {
    if (sel) oninsert(sel.body ?? '');
  }
</script>

<div class="pp" class:compact>
  <div class="pp-cols">
    <div class="pp-side">
      <div class="pp-find">
        <input
          type="text"
          autocomplete="off"
          placeholder={$t('assistant.promptSearch')}
          aria-label={$t('assistant.promptSearch')}
          bind:value={q}
        />
      </div>
      <div class="pp-list">
        {#if prompts === null}
          <p class="pp-hint">{$t('common.loading')}</p>
        {:else if err}
          <p class="pp-hint">{err}</p>
        {:else if !prompts.length}
          <p class="pp-hint">{$t('assistant.promptEmpty')}</p>
        {:else if !shown.length}
          <p class="pp-hint">{$t('common.noMatches')}</p>
        {:else}
          {#each shown as p (p.id)}
            <button
              class="pp-row"
              class:sel={p.id === selId}
              onclick={() => (selId = p.id)}
              ondblclick={() => {
                selId = p.id;
                insert();
              }}
            >
              <span class="pp-row-name">{p.name || '—'}</span>
              {#if p.tags?.length}
                <span class="pp-row-tags">{p.tags.join(' · ')}</span>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    </div>

    <div class="pp-preview">
      {#if sel}
        {#if !compact}<p class="pp-title">{sel.name || '—'}</p>{/if}
        <pre>{sel.body || ''}</pre>
      {:else}
        <div class="pp-blank">
          <Icon name="message-square" size={20} />
          <p>{$t('agent.promptPick.preview')}</p>
        </div>
      {/if}
    </div>
  </div>

  <div class="pp-actions">
    <span class="pp-left">
      <a class="pp-manage" href="/prompt-store">{$t('agent.promptPick.manage')} →</a>
    </span>
    <Button size="sm" variant="ghost" onclick={oncancel}>{$t('common.cancel')}</Button>
    <Button size="sm" variant="primary" icon="check" disabled={!sel} onclick={insert}>
      {$t('agent.promptPick.insert')}
    </Button>
  </div>
</div>

<style>
  .pp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .pp.compact {
    height: 100%;
    gap: var(--space-2);
  }
  .pp-cols {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: var(--space-3);
    height: 380px;
  }
  /* Stacked in the assistant panel: list on top, preview under it. */
  .pp.compact .pp-cols {
    grid-template-columns: 1fr;
    grid-template-rows: 1fr auto;
    gap: var(--space-2);
    height: auto;
    flex: 1;
    min-height: 0;
  }
  .pp-side {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
    border-right: var(--hairline) solid var(--border);
    padding-right: var(--space-3);
  }
  .pp.compact .pp-side {
    border-right: none;
    padding-right: 0;
  }
  .pp-find {
    position: relative;
  }
  .pp-find input {
    width: 100%;
    height: 30px;
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
  }
  .pp-find input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .pp-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .pp-row {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2);
    border: var(--hairline) solid transparent;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .pp-row:hover {
    background: var(--surface-2);
  }
  .pp-row.sel {
    border-color: var(--border-control);
    background: var(--surface-2);
  }
  .pp-row-name {
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pp-row-tags {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pp-preview {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
  }
  .pp-title {
    margin: 0;
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .pp-preview pre {
    flex: 1;
    min-height: 0;
    margin: 0;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    padding: var(--space-3);
    color: var(--text);
    font-size: var(--text-xs);
    line-height: 1.5;
  }
  .pp.compact .pp-preview pre {
    max-height: 110px;
    padding: var(--space-2);
  }
  .pp-blank {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--muted);
    text-align: center;
  }
  .pp.compact .pp-blank {
    flex-direction: row;
    gap: var(--space-2);
    padding: var(--space-2) 0;
  }
  .pp-blank p {
    margin: 0;
    font-size: var(--text-sm);
  }
  .pp.compact .pp-blank p {
    font-size: var(--text-xs);
    text-align: left;
  }
  .pp-hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }

  .pp-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-2);
  }
  .pp-left {
    margin-right: auto;
    display: inline-flex;
    align-items: center;
  }
  .pp-manage {
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }
  .pp-manage:hover {
    color: var(--accent);
  }

  @media (max-width: 640px) {
    .pp-cols {
      grid-template-columns: 1fr;
      grid-template-rows: 1fr auto;
      height: auto;
    }
    .pp-side {
      border-right: none;
      padding-right: 0;
    }
    .pp-list {
      max-height: 180px;
    }
    .pp-preview pre {
      max-height: 200px;
    }
  }
</style>
