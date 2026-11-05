<script>
  // A saved version of a database, read-only: its columns and rows as a plain table, with
  // the way to restore it. Open while `version` is set.
  // props: version (full document version | null), onclose(), onrestore(version)
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { displayValue } from './db/cells.js';
  import { fmtVersionDate } from '$lib/versioning/state.svelte.js';
  import { t } from '$lib/i18n';

  let { version = null, onclose = () => {}, onrestore = () => {} } = $props();

  let open = $state(false);
  $effect(() => {
    open = !!version;
  });

  const columns = $derived(version?.data?.columns ?? []);
  const rows = $derived(version?.data?.rows ?? []);
</script>

<Modal bind:open title={version?.title || $t('editor.page.untitledDatabase')} size="lg" {onclose}>
  {#if version}
    <p class="meta">
      {fmtVersionDate(version.created_at)}{#if version.note}<span class="note"> · {version.note}</span>{/if}
    </p>
    {#if !columns.length}
      <p class="meta">{$t('editor.database.noColumns')}</p>
    {:else}
      <div class="wrap">
        <table>
          <thead>
            <tr>{#each columns as c (c.id)}<th>{c.name}</th>{/each}</tr>
          </thead>
          <tbody>
            {#each rows as r (r.id)}
              <tr>{#each columns as c (c.id)}<td>{displayValue(c, r.cells?.[c.id])}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="meta">{$t('versioning.preview.rows', { n: rows.length })}</p>
    {/if}
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
  .wrap {
    overflow: auto;
    max-height: 55vh;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    margin-bottom: var(--space-2);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    text-align: left;
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--surface-2);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
</style>
