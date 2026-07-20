<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Pending tasks: dates where no FX source could supply rates, so the breakdown can't
  // convert trades on those days. The user enters USD-based rates by hand to resolve them.
  import { onMount } from 'svelte';
  import { journalApi } from './api.js';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

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
    <Skeleton rows={3} height="52px" />
  {:else if pending.length === 0}
    <!-- Nothing pending is the good outcome here, not a void to fill. -->
    <EmptyState icon="check-circle" description={$t('journal.pending.empty')} compact />
  {:else}
    <ul class="list">
      {#each pending as p (p.pending_date)}
        <li class:open={openDate === p.pending_date}>
          <button
            class="row"
            onclick={() => toggle(p.pending_date)}
            aria-expanded={openDate === p.pending_date}
          >
            <span class="date num">{p.pending_date}</span>
            <span class="reason">{p.reason}</span>
            <span class="chev"><Icon name={openDate === p.pending_date ? 'chevron-down' : 'chevron-right'} size={13} /></span>
          </button>
          {#if openDate === p.pending_date}
            <div class="editor">
              <div class="rate-grid">
                {#each quotes as q (q)}
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
              <ErrorText error={error} copyable />
              <div class="actions">
                <Button variant="ghost" onclick={() => (openDate = null)} disabled={saving}>
                  {$t('common.cancel')}
                </Button>
                <Button variant="primary" loading={saving} onclick={() => resolve(p.pending_date)}>
                  {$t('journal.pending.saveRates')}
                </Button>
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
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .list li {
    border: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
    border-radius: 0;
    overflow: hidden;
  }
  .list li.open {
    border-left-color: var(--accent);
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
    background: var(--surface-2);
  }
  /* .num (theme/default.css) already gives it tabular figures. */
  .date {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .reason {
    color: var(--muted);
    font-size: var(--text-sm);
    flex: 1;
  }
  .chev {
    color: var(--muted);
    display: inline-flex;
  }
  .editor {
    padding: var(--space-4);
    background: var(--surface-2);
    border-top: 0.5px solid var(--border);
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
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
