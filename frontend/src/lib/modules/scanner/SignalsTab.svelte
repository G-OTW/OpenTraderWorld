<script>
  // The signal library. A signal is reusable: the same "SMA cross" can sit on three alerts
  // over three hives, and editing it changes all of them from the next bar. A signal that
  // cannot be judged (a deleted indicator, a session gone) says why on its row.
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import SignalModal from './SignalModal.svelte';
  import { scannerApi } from './api.js';
  import { cssColor } from './board.js';
  import { t } from '$lib/i18n';

  let { signals = [], items = [], alerts = [], onchanged } = $props();

  let open = $state(false);
  let editing = $state(null);
  let deleting = $state(null);
  let error = $state('');
  let filter = $state('all');

  const uses = (id) => alerts.filter((a) => a.signal_ids.includes(id)).length;
  const shown = $derived(filter === 'all' ? signals : signals.filter((s) => s.builder === filter));

  function edit(s) {
    editing = s;
    open = true;
  }

  async function remove() {
    try {
      await scannerApi.deleteSignal(deleting.id);
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      deleting = null;
    }
  }
</script>

<div class="wrap">
  <div class="filter-bar">
    <Button variant="primary" icon="plus" onclick={() => edit(null)}>{$t('scanner.signals.new')}</Button>
    {#each ['all', 'signal', 'event', 'pattern', 'time'] as f (f)}
      <button class="chip" class:active={filter === f} aria-pressed={filter === f} onclick={() => (filter = f)}>
        {f === 'all' ? $t('scanner.all') : $t(`scanner.builder.${f}`)}
      </button>
    {/each}
  </div>
  {#if error}<ErrorText {error} />{/if}

  {#if !signals.length}
    <EmptyState icon="zap" title={$t('scanner.signals.emptyTitle')} description={$t('scanner.signals.emptyBody')}>
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={() => edit(null)}>{$t('scanner.signals.new')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <ul class="card-list">
      {#each shown as s (s.id)}
        <li>
          <div class="who">
            <span class="dot" style="--c:{cssColor(s.color) || 'var(--border-control)'}"></span>
            <button class="name" onclick={() => edit(s)}>{s.name}</button>
            <span class="badge">{$t(`scanner.builder.${s.builder}`)}</span>
            {#if s.error}
              <span class="err"><Icon name="alert-triangle" size={12} /> {s.error}</span>
            {:else}
              <span class="sum">{s.summary}</span>
            {/if}
          </div>
          <div class="act">
            <span class="sum">{$t('scanner.signals.uses', { n: uses(s.id) })}</span>
            <button class="icon" title={$t('scanner.edit')} aria-label={$t('scanner.edit')} onclick={() => edit(s)}><Icon name="pencil" size={13} /></button>
            <button class="icon" title={$t('scanner.delete')} aria-label={$t('scanner.delete')} onclick={() => (deleting = s)}><Icon name="trash" size={13} /></button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<SignalModal bind:open signal={editing} {signals} {items} onsaved={onchanged} />

<ConfirmModal
  open={!!deleting}
  title={$t('scanner.signals.deleteTitle')}
  message={$t('scanner.signals.deleteBody', { name: deleting?.name ?? '' })}
  confirmLabel={$t('scanner.delete')}
  cancelLabel={$t('scanner.cancel')}
  danger
  onconfirm={remove}
  oncancel={() => (deleting = null)}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* Filter chips share the New button's height. */
  .filter-bar .chip {
    height: var(--control-h);
    padding: 0 var(--space-3);
  }
  .who,
  .act {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--c);
    flex: none;
  }
  .name {
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .name:hover {
    text-decoration: underline;
  }
  .sum {
    color: var(--dim);
    font-size: var(--text-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
    display: inline-flex;
    gap: var(--space-1);
    align-items: center;
  }
  .icon {
    background: none;
    border: none;
    color: var(--dim);
    cursor: pointer;
    padding: var(--space-1);
  }
  .icon:hover {
    color: var(--text);
  }
</style>
