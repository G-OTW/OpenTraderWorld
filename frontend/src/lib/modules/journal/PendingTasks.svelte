<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  // Pending tasks: dates where no FX source could supply rates, so the breakdown can't
  // convert trades on those days. The user enters USD-based rates by hand to resolve them.
  import { onMount } from 'svelte';
  import { journalApi } from './api.js';
  import { t } from '$lib/i18n';

  let { onchanged = () => {} } = $props();

  let pending = $state([]);
  let quotes = $state([]); // the 11 non-USD majors to fill in
  let loading = $state(true);

  // date -> { [quote]: value } draft inputs for the expanded row.
  let drafts = $state({});
  let openDate = $state(null);
  let saving = $state(false);
  let error = $state('');

  onMount(load);

  async function load() {
    loading = true;
    const [p, q] = await Promise.all([journalApi.fxPending(), journalApi.fxQuotes()]);
    pending = p;
    quotes = q.quotes;
    loading = false;
  }

  function toggle(date) {
    if (openDate === date) {
      openDate = null;
      return;
    }
    openDate = date;
    error = '';
    if (!drafts[date]) {
      drafts[date] = Object.fromEntries(quotes.map((q) => [q, '']));
    }
  }

  async function resolve(date) {
    error = '';
    const draft = drafts[date] ?? {};
    const rates = {};
    for (const [q, v] of Object.entries(draft)) {
      const n = Number(v);
      if (v !== '' && Number.isFinite(n) && n > 0) rates[q] = n;
    }
    if (Object.keys(rates).length === 0) {
      error = $t('journal.pending.errNoRate');
      return;
    }
    saving = true;
    try {
      await journalApi.fxResolve(date, rates);
      openDate = null;
      delete drafts[date];
      await load();
      onchanged();
    } catch (e) {
      error = e.message ?? $t('journal.pending.errSave');
    } finally {
      saving = false;
    }
  }
</script>

<div class="pending">
  <p class="intro">
    {@html $t('journal.pending.intro')}
  </p>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if pending.length === 0}
    <div class="empty">{$t('journal.pending.empty')}</div>
  {:else}
    <ul class="list">
      {#each pending as p (p.pending_date)}
        <li class:open={openDate === p.pending_date}>
          <button class="row" onclick={() => toggle(p.pending_date)}>
            <span class="date">{p.pending_date}</span>
            <span class="reason">{p.reason}</span>
            <span class="chev"><Icon name={openDate === p.pending_date ? 'chevron-down' : 'chevron-right'} size={13} /></span>
          </button>
          {#if openDate === p.pending_date}
            <div class="editor">
              <div class="rate-grid">
                {#each quotes as q}
                  <label class="rate">
                    <span>{$t('journal.pending.usdTo', { currency: q })}</span>
                    <input
                      type="number"
                      step="any"
                      placeholder="0.0"
                      bind:value={drafts[p.pending_date][q]}
                    />
                  </label>
                {/each}
              </div>
              {#if error}<p class="err" title={$t('journal.pending.clickToCopy')} use:copyLog={error}>{error}</p>{/if}
              <div class="actions">
                <button class="ghost" onclick={() => (openDate = null)}>{$t('common.cancel')}</button>
                <button class="primary" disabled={saving} onclick={() => resolve(p.pending_date)}>
                  {saving ? $t('common.saving') : $t('journal.pending.saveRates')}
                </button>
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .pending {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 720px;
  }
  .intro {
    color: var(--muted);
    font-size: 0.85rem;
    line-height: 1.5;
  }
  .empty {
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-8);
    text-align: center;
    color: var(--muted);
    font-size: 0.9rem;
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .list li {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .list li.open {
    border-color: var(--accent);
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--surface);
    border: none;
    color: var(--text);
    padding: var(--space-3) var(--space-4);
    cursor: pointer;
    text-align: left;
  }
  .row:hover {
    background: var(--surface-2, var(--surface));
  }
  .date {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .reason {
    color: var(--muted);
    font-size: 0.82rem;
    flex: 1;
  }
  .chev {
    color: var(--muted);
  }
  .editor {
    padding: var(--space-4);
    background: var(--surface-2, var(--surface));
    border-top: 1px solid var(--border);
  }
  .rate-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
    gap: var(--space-3);
  }
  .rate {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .rate > span {
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .err {
    color: var(--red);
    font-size: 0.8rem;
    margin-top: var(--space-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
