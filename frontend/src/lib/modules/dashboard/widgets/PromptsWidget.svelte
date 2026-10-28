<script>
  // Prompt-store widget: a horizontal tag rail on top, prompt names listed below.
  // Clicking a name copies the prompt body to the clipboard — nothing else.
  import { promptsApi } from '$lib/modules/prompt-store/api.js';
  
  import { t } from '$lib/i18n';
import { ago } from '$lib/format.js';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';

  let { item, editing } = $props();
  const limit = $derived(Math.max(1, Math.min(50, item.config?.limit ?? 12)));
  const variant = $derived(item.config?.variant ?? 'library');

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

  // The flash clears itself; cancel it so it cannot fire on a destroyed component.
  $effect(() => () => clearTimeout(copyTimer));

  const filtered = $derived((prompts ?? []).filter((p) => tag === 'all' || p.tags.includes(tag)).slice(0, limit));

  const recent = $derived(
    [...(prompts ?? [])].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, limit)
  );
  // "Iterated" is the version counter the module already keeps: how many times the body
  // was rewritten, not how often it was used.
  const iterated = $derived(
    [...(prompts ?? [])].sort((a, b) => b.version - a.version).slice(0, limit)
  );

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

<WidgetState
  {editing}
  error={err}
  loading={prompts === null}
  empty={prompts?.length === 0}
  preview={$t('dashboard.widgets.prompts.preview')}
  emptyText={$t('dashboard.widgets.prompts.empty')}
  rows={4}
>
  {#if variant === 'recent'}
    <div class="w-list">
      {#each recent as p (p.id)}
        <button class="w-row stack" onclick={() => copy(p)} title={$t('dashboard.widgets.prompts.copyTitle')}>
          <span class="line">
            <span class="w-name name">{p.name}</span>
            {#if copiedId === p.id}
              <span class="w-pill pos"><Icon name="check" size={12} /> {$t('dashboard.widgets.prompts.copied')}</span>
            {:else}
              <span class="copy" aria-hidden="true"><Icon name="copy" size={13} /></span>
            {/if}
          </span>
          <span class="w-sub meta">
            {#if p.tags?.length}<span class="tagref">#{p.tags[0]}</span> · {/if}{$ago(p.updated_at)}
          </span>
        </button>
      {/each}
    </div>
  {:else if variant === 'iterated'}
    <ul class="w-list">
      {#each iterated as p, i (p.id)}
        <li class="w-row">
          <span class="rank">{i + 1}</span>
          <span class="w-name name">{p.name}</span>
          <span class="w-num">{p.version}</span>
        </li>
      {/each}
    </ul>
  {:else}
  <div class="w-body">
    {#if tags.length}
      <nav class="w-chips" aria-label={$t('promptStore.tags')}>
        <button class="w-chip" aria-pressed={tag === 'all'} onclick={() => (tag = 'all')}>
          {$t('dashboard.widgets.prompts.all')}
        </button>
        {#each tags as tg (tg)}
          <button class="w-chip" aria-pressed={tag === tg} onclick={() => (tag = tag === tg ? 'all' : tg)}>
            {tg}
          </button>
        {/each}
      </nav>
    {/if}
    <div class="w-list w-scroll">
      {#each filtered as p (p.id)}
        <button class="w-row" onclick={() => copy(p)} title={$t('dashboard.widgets.prompts.copyTitle')}>
          <span class="w-name name">{p.name}</span>
          {#if copiedId === p.id}
            <span class="w-pill pos"><Icon name="check" size={12} /> {$t('dashboard.widgets.prompts.copied')}</span>
          {:else}
            <span class="copy" aria-hidden="true"><Icon name="copy" size={13} /></span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
  {/if}
</WidgetState>

<style>
  .name {
    flex: 1;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-width: 0;
  }
  .meta {
    text-align: left;
  }
  .tagref {
    color: var(--accent);
  }
  .rank {
    width: 14px;
    color: var(--faint);
    font-family: var(--mono);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  /* The copy affordance appears on approach: the row is the button, the icon only
     says so. */
  .copy {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--faint);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .w-row:hover .copy,
  .w-row:focus-visible .copy {
    opacity: 1;
  }
  .w-row:hover .name {
    color: var(--accent);
  }
</style>
