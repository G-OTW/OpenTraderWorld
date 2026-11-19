<script>
  // Monthly cap per category, in the display currency, against what each category costs a
  // month today. A category above its cap notifies once per month through the channels
  // picked here (none picked = every channel granted to Subscriptions).
  import { untrack } from 'svelte';
  import { subsApi } from './api.js';
  import { money } from './fmt.svelte.js';
  import Badge from '$lib/ui/Badge.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { t } from '$lib/i18n';

  let {
    settings,
    categories = [], // known category names
    totals = [], // [{ category, amount }] monthly, display currency
    onsaved = () => {},
    oncancel = () => {}
  } = $props();

  // The panel edits a copy taken when it opens; saving hands the new settings back.
  const init = untrack(() => {
    const names = [
      ...new Set([...categories, ...totals.map((c) => c.category), ...Object.keys(settings.category_budgets ?? {})])
    ].sort((a, b) => a.localeCompare(b));
    return {
      cur: settings.display_currency,
      names,
      caps: Object.fromEntries(names.map((n) => [n, settings.category_budgets?.[n] ?? null])),
      picked: [...(settings.budget_channels ?? [])]
    };
  });
  const cur = init.cur;
  const names = init.names;
  let caps = $state(init.caps);
  let picked = $state(init.picked);
  let channels = $state([]);
  let error = $state('');
  let saving = $state(false);

  async function loadChannels() {
    try {
      channels = await channelsApi.list('subscriptions');
    } catch {
      channels = [];
    }
  }
  loadChannels();
  const channelOptions = $derived(channels.map((c) => ({ value: c.id, label: `${c.name} (${c.kind})` })));
  const spent = (n) => totals.find((c) => c.category === n)?.amount ?? 0;

  async function save() {
    error = '';
    const category_budgets = {};
    for (const [n, v] of Object.entries(caps)) {
      if (v === null || v === undefined || v === '') continue;
      const num = Number(v);
      if (!Number.isFinite(num) || num <= 0) {
        error = $t('subscriptions.budgets.invalid', { category: n });
        return;
      }
      category_budgets[n] = num;
    }
    saving = true;
    try {
      const s = await subsApi.updateSettings({ category_budgets, budget_channels: picked });
      onsaved(s);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<div class="budgets">
  <p class="hint">{$t('subscriptions.budgets.hint', { cur })}</p>
  {#if names.length === 0}
    <p class="hint">{$t('subscriptions.budgets.noCategories')}</p>
  {:else}
    <div class="rows" role="table" aria-label={$t('subscriptions.budgets.title')}>
      <div class="row head" role="row">
        <span role="columnheader">{$t('subscriptions.form.category')}</span>
        <span role="columnheader">{$t('subscriptions.budgets.thisMonth')}</span>
        <span role="columnheader" class="r">{$t('subscriptions.budgets.cap', { cur })}</span>
      </div>
      {#each names as n (n)}
        {@const cap = Number(caps[n])}
        {@const hasCap = caps[n] !== null && caps[n] !== '' && cap > 0}
        {@const ratio = hasCap ? spent(n) / cap : 0}
        {@const level = !hasCap ? 'none' : ratio > 1 ? 'over' : ratio >= 0.8 ? 'near' : 'ok'}
        <div class="row" role="row">
          <span class="cat" role="cell" title={n}>{n}</span>
          <span class="use" role="cell">
            <span class="figs">
              <span class="fig">{money(spent(n), cur)}</span>
              {#if hasCap}
                <span class="of">{$t('subscriptions.budgets.ofCap', { pct: Math.round(ratio * 100) })}</span>
                {#if level === 'over'}<Badge tone="danger">{$t('subscriptions.budgets.overBadge')}</Badge>{/if}
              {/if}
            </span>
            {#if hasCap}
              <span class="meter {level}" role="meter" aria-valuemin="0" aria-valuemax={cap} aria-valuenow={spent(n)} aria-label={n}>
                <span class="fill" style:width="{Math.min(ratio, 1) * 100}%"></span>
              </span>
            {/if}
          </span>
          <span class="r" role="cell">
            <input type="number" min="0" step="any" bind:value={caps[n]} placeholder={$t('subscriptions.budgets.noCap')} aria-label={$t('subscriptions.budgets.capFor', { category: n })} />
          </span>
        </div>
      {/each}
    </div>
  {/if}
  <div class="channels">
    <div class="field grow">
      <span>{$t('subscriptions.form.channels')}</span>
      <MultiSelect bind:value={picked} options={channelOptions} allLabel={$t('subscriptions.form.allChannels')} width="100%" />
    </div>
    <ChannelButton module="subscriptions" size="sm" onchanged={loadChannels} />
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="button" class="primary" disabled={saving} onclick={save}>{$t('common.save')}</button>
  </div>
</div>

<style>
  .budgets {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .hint {
    color: var(--dim);
    font-size: 12px;
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(96px, 1fr) minmax(0, 2fr) 128px;
    align-items: center;
    gap: var(--space-4);
    min-height: 48px;
    padding: var(--space-2) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:last-child {
    border-bottom: none;
  }
  .row.head {
    min-height: 0;
    padding: 0 0 var(--space-2);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .r {
    text-align: right;
  }
  .cat {
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .use {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .figs {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 13px;
  }
  .of {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    color: var(--dim);
  }
  /* Thin meter: the fill carries the severity, the track is a light step of it. */
  .meter {
    --tone: var(--accent);
    position: relative;
    display: block;
    height: 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--tone) 16%, var(--surface-2));
    overflow: hidden;
  }
  .meter.near {
    --tone: var(--amber);
  }
  .meter.over {
    --tone: var(--red);
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 2px;
    background: var(--tone);
  }
  .r input {
    width: 100%;
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .channels {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    padding-top: var(--space-2);
  }
  .field.grow {
    flex: 1;
  }
  .error {
    color: var(--red);
    font-size: 12px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  @media (max-width: 767px) {
    .row {
      grid-template-columns: minmax(0, 1fr) 112px;
    }
    .row .use {
      grid-column: 1 / -1;
      grid-row: 2;
    }
    .row.head span:nth-child(2) {
      display: none;
    }
    .channels {
      flex-wrap: wrap;
    }
    .actions button {
      min-height: 44px;
    }
  }
</style>
