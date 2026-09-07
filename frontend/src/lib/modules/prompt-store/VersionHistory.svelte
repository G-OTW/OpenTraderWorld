<script>
  // Version history for one prompt. Lists every saved revision newest-first; the
  // current version is marked, older ones offer a preview + one-click rollback
  // (which appends the chosen content as a new version).
  import Icon from '$lib/ui/Icon.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { promptsApi, fmtDateTime } from '$lib/modules/prompt-store/api.js';
  import { t as tr } from '$lib/i18n';

  let { prompt, onrolledback = () => {} } = $props();

  let versions = $state([]);
  let loading = $state(true);
  let expanded = $state(null); // version number whose body is shown
  let busy = $state(false);

  let confirmOpen = $state(false);
  let pending = $state(null); // version awaiting rollback confirmation

  // Load on mount and whenever the prompt's current version changes (e.g. after a rollback).
  $effect(() => {
    void prompt.version;
    load();
  });

  async function load() {
    loading = true;
    versions = await promptsApi.versions(prompt.id);
    loading = false;
  }

  function askRollback(v) {
    if (busy) return;
    pending = v;
    confirmOpen = true;
  }
  async function doRollback() {
    if (!pending) return;
    busy = true;
    try {
      const updated = await promptsApi.rollback(prompt.id, pending.version);
      pending = null;
      await load();
      onrolledback(updated);
    } finally {
      busy = false;
    }
  }
</script>

{#if loading}
  <p class="muted">{$tr('common.loading')}</p>
{:else if !versions.length}
  <p class="muted">{$tr('promptStore.history.none')}</p>
{:else}
  <ul class="versions">
    {#each versions as v (v.version)}
      {@const current = v.version === prompt.version}
      <li class:current>
        <div class="v-head">
          <button class="v-toggle" onclick={() => (expanded = expanded === v.version ? null : v.version)}>
            <Icon name={expanded === v.version ? 'chevron-down' : 'chevron-right'} size={14} />
            <span class="v-num">v{v.version}</span>
            {#if current}<span class="badge">{$tr('promptStore.history.current')}</span>{/if}
            <span class="v-name">{v.name}</span>
            <span class="v-when">{fmtDateTime(v.created_at)}</span>
          </button>
          {#if !current}
            <button class="restore" onclick={() => askRollback(v)} disabled={busy}>
              <Icon name="rotate-ccw" size={13} /> {$tr('promptStore.history.restore')}
            </button>
          {/if}
        </div>
        {#if expanded === v.version}
          <div class="v-body">
            {#if v.tags.length}
              <div class="v-tags">
                {#each v.tags as t}<span class="tag">{t}</span>{/each}
              </div>
            {/if}
            <pre>{v.body || $tr('promptStore.history.empty')}</pre>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<ConfirmModal
  bind:open={confirmOpen}
  title={$tr('promptStore.history.title')}
  message={pending ? $tr('promptStore.history.confirmRollback', { version: pending.version }) : ''}
  confirmLabel={$tr('promptStore.history.restore')}
  cancelLabel={$tr('common.cancel')}
  onconfirm={doRollback}
  oncancel={() => (pending = null)}
/>

<style>
  .muted {
    color: var(--muted);
  }
  .versions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  li {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  li.current {
    border-color: var(--accent);
  }
  .v-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 6px 8px;
  }
  .v-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    text-align: left;
    flex: 1;
    min-width: 0;
    font-size: var(--text-base);
  }
  .v-num {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .v-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
  .v-when {
    color: var(--dim);
    font-family: var(--mono);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  .badge {
    color: var(--accent);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 0.68rem;
  }
  .restore {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 4px 8px;
    cursor: pointer;
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  .restore:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-control);
  }
  .restore:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .v-body {
    border-top: 1px solid var(--border);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .v-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tag {
    background: var(--surface-2);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 1px 7px;
    font-size: 0.7rem;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 8px;
    max-height: 260px;
    overflow-y: auto;
  }
</style>
