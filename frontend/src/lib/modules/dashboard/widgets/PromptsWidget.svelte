<script>
  // Prompt-store widget: a horizontal tag rail on top, prompt names listed below.
  // Clicking a name copies the prompt body to the clipboard — nothing else.
  import { promptsApi } from '$lib/modules/prompt-store/api.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();

  let prompts = $state(null);
  let tags = $state([]);
  let tag = $state('all');
  let err = $state('');
  let copiedId = $state(null);
  let copyTimer;

  async function load() {
    err = '';
    try {
      [prompts, tags] = await Promise.all([promptsApi.list(), promptsApi.tags()]);
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const filtered = $derived((prompts ?? []).filter((p) => tag === 'all' || p.tags.includes(tag)));

  async function copy(p) {
    try {
      await navigator.clipboard.writeText(p.body);
      copiedId = p.id;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedId = null), 1200);
    } catch {
      /* clipboard unavailable — silent */
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.prompts.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if prompts === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else if prompts.length === 0}
  <p class="hint">{$t('dashboard.widgets.prompts.empty')}</p>
{:else}
  <div class="wrap">
    {#if tags.length}
      <nav class="rail" aria-label={$t('promptStore.tags')}>
        <button class="pill" class:active={tag === 'all'} onclick={() => (tag = 'all')}>
          {$t('dashboard.widgets.prompts.all')}
        </button>
        {#each tags as tg (tg)}
          <button class="pill" class:active={tag === tg} onclick={() => (tag = tag === tg ? 'all' : tg)}>
            {tg}
          </button>
        {/each}
      </nav>
    {/if}
    <div class="list">
      {#each filtered as p (p.id)}
        <button class="row" onclick={() => copy(p)} title={$t('dashboard.widgets.prompts.copyTitle')}>
          <span class="name">{p.name}</span>
          {#if copiedId === p.id}
            <span class="copied"><Icon name="check" size={13} /> {$t('dashboard.widgets.prompts.copied')}</span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .hint {
    color: var(--dim);
  }
  .sk {
    padding: var(--space-1) 0;
  }
  .wrap {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .rail {
    display: flex;
    gap: var(--space-1);
    overflow-x: auto;
    flex-shrink: 0;
    scrollbar-width: thin;
    padding-bottom: 2px;
  }
  .pill {
    flex-shrink: 0;
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    color: var(--dim);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 999px;
    cursor: pointer;
    white-space: nowrap;
  }
  .pill:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .pill.active {
    color: var(--text);
    border-color: var(--accent);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 3px 0;
    font-size: var(--text-sm);
    color: var(--text);
    background: none;
    border: 0;
    cursor: pointer;
    text-align: left;
  }
  .row:hover .name {
    color: var(--accent);
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .copied {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
    font-size: var(--text-xs);
    color: var(--green);
  }
</style>
