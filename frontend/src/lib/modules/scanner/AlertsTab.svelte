<script>
  // Scanner alerts: each row says what it scans, with which signals, how it notifies, when it
  // last ran, and why it could not (an alert that cannot scan says so here and on its
  // channels). "Run now" judges the target immediately, records nothing and notifies nobody,
  // and shows the result as a grid.
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import AlertModal from './AlertModal.svelte';
  import { scannerApi, chartUrl } from './api.js';
  import { cssColor } from './board.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { alerts = [], items = [], hives = [], signals = [], focus = null, onchanged, ongo } = $props();

  let open = $state(false);
  let editing = $state(null);
  let deleting = $state(null);
  let error = $state('');
  let running = $state(null);
  let preview = $state(null);

  const sigById = $derived(new Map(signals.map((s) => [s.id, s])));
  const itemById = $derived(new Map(items.map((i) => [i.id, i])));

  function targetName(a) {
    if (a.target_kind === 'hive') return hives.find((h) => h.id === a.target_id)?.name ?? $t('scanner.alerts.gone');
    const it = itemById.get(a.target_id);
    return it ? `${it.ticker} ${it.timeframe}` : $t('scanner.alerts.gone');
  }

  function edit(a) {
    editing = a;
    open = true;
  }

  async function toggle(a) {
    try {
      await scannerApi.saveAlert(a.id, { ...a, enabled: !a.enabled, channels: Array.isArray(a.channels) ? a.channels : [] });
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function runNow(a) {
    running = a.id;
    try {
      const r = await scannerApi.runNow(a.id);
      preview = { alert: a, ...r };
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      running = null;
    }
  }

  async function remove() {
    try {
      await scannerApi.deleteAlert(deleting.id);
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      deleting = null;
    }
  }

  const canCreate = $derived(signals.length > 0 && (items.length > 0 || hives.length > 0));
</script>

<div class="wrap">
  <div class="filter-bar">
    <Button variant="primary" icon="plus" disabled={!canCreate} onclick={() => edit(null)}>{$t('scanner.alerts.new')}</Button>
    {#if !canCreate}
      <span class="hint">{$t('scanner.alerts.needs')}</span>
    {/if}
  </div>
  {#if error}<ErrorText {error} />{/if}

  {#if !alerts.length}
    <EmptyState icon="bell" title={$t('scanner.alerts.emptyTitle')} description={$t('scanner.alerts.emptyBody')}>
      {#snippet action()}
        {#if canCreate}
          <Button variant="primary" icon="plus" onclick={() => edit(null)}>{$t('scanner.alerts.new')}</Button>
        {:else}
          <Button icon="zap" onclick={() => ongo?.('signals')}>{$t('scanner.alerts.toSignals')}</Button>
        {/if}
      {/snippet}
    </EmptyState>
  {:else}
    <ul class="card-list">
      {#each alerts as a (a.id)}
        <li class:focus={focus === a.id} class:off={!a.enabled}>
          <div class="main">
            <div class="line">
              <button class="name" onclick={() => edit(a)}>{a.name || $t('scanner.alerts.unnamed')}</button>
              <span class="badge">{targetName(a)}</span>
              <span class="sigs">
                {#each a.signal_ids as sid, i (sid)}
                  {#if i > 0}<span class="and">{$t('scanner.and')}</span>{/if}
                  <span class="sig"><span class="dot" style="--c:{cssColor(sigById.get(sid)?.color) || 'var(--border-control)'}"></span>{sigById.get(sid)?.name ?? '?'}</span>
                {/each}
              </span>
            </div>
            <div class="line meta">
              <span>{a.notify === 'per_hit' ? $t('scanner.alerts.perHit') : $t('scanner.alerts.digest')}</span>
              {#if a.cooldown_secs}<span>{$t('scanner.alerts.cooldownShort', { min: Math.round(a.cooldown_secs / 60) })}</span>{/if}
              <span>{a.last_run_at ? $t('scanner.alerts.ranAt', { at: new Date(a.last_run_at).toLocaleString() }) : $t('scanner.alerts.notRun')}</span>
            </div>
            {#if a.last_error}
              <div class="err"><Icon name="alert-triangle" size={12} /> {a.last_error}</div>
            {/if}
          </div>
          <div class="act">
            <label class="switch" title={$t('scanner.alerts.enabled')}>
              <input type="checkbox" checked={a.enabled} onchange={() => toggle(a)} aria-label={$t('scanner.alerts.enabled')} />
            </label>
            <Button size="sm" icon="play" loading={running === a.id} onclick={() => runNow(a)}>{$t('scanner.alerts.runNow')}</Button>
            <button class="icon" title={$t('scanner.edit')} aria-label={$t('scanner.edit')} onclick={() => edit(a)}><Icon name="pencil" size={13} /></button>
            <button class="icon" title={$t('scanner.delete')} aria-label={$t('scanner.delete')} onclick={() => (deleting = a)}><Icon name="trash" size={13} /></button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<AlertModal bind:open alert={editing} {items} {hives} {signals} onsaved={onchanged} />

<Modal open={!!preview} size="lg" title={preview ? $t('scanner.alerts.previewTitle', { name: preview.alert.name || targetName(preview.alert) }) : ''} onclose={() => (preview = null)}>
  {#if preview}
    <p class="hint">{$t('scanner.alerts.previewHint', { hits: preview.hits, n: preview.rows.length })}</p>
    <div class="scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('scanner.items.instrument')}</th>
            <th>{$t('scanner.alerts.held')}</th>
            {#each preview.alert.signal_ids as sid (sid)}<th>{sigById.get(sid)?.name ?? '?'}</th>{/each}
            <th class="num">{$t('scanner.col.close')}</th>
            <th>{$t('scanner.col.bar')}</th>
          </tr>
        </thead>
        <tbody>
          {#each preview.rows as r (r.item_id)}
            {@const it = itemById.get(r.item_id)}
            <tr class:hit={r.held}>
              <td>{#if it}<a class="sym" href={chartUrl(it)}>{r.ticker}</a>{:else}{r.ticker}{/if} <span class="hint">{r.timeframe}</span></td>
              {#if r.error}
                <td colspan={preview.alert.signal_ids.length + 3} class="errc">{r.error}</td>
              {:else}
                <td>{#if r.held}<Icon name="check" size={13} />{/if}</td>
                {#each preview.alert.signal_ids as sid (sid)}
                  <td><span class="lit" class:on={r.states[sid]} style="--c:{cssColor(sigById.get(sid)?.color) || 'var(--accent)'}"></span></td>
                {/each}
                <td class="num">{r.values?.close != null ? fmtNum(r.values.close, 4) : ''}</td>
                <td class="hint">{r.bar_ts ? new Date(r.bar_ts).toLocaleString() : ''}</td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</Modal>

<ConfirmModal
  open={!!deleting}
  title={$t('scanner.alerts.deleteTitle')}
  message={$t('scanner.alerts.deleteBody')}
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
  li.focus {
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
  }
  li.off .main {
    opacity: 0.6;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .meta {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .name {
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .sigs,
  .sig {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
  }
  .and {
    color: var(--dim);
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
  }
  .err,
  .errc {
    color: var(--red);
    font-size: var(--text-xs);
  }
  .act {
    display: flex;
    align-items: center;
    gap: var(--space-2);
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
  .scroll {
    overflow-x: auto;
  }
  .sym {
    font-family: var(--mono);
    color: var(--text);
  }
  tr.hit td:first-child {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .lit {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 1px solid var(--border-control);
  }
  .lit.on {
    background: var(--c);
    border-color: var(--c);
  }
</style>
