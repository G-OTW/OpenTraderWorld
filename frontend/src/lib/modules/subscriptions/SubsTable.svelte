<script>
  // The subscription list: 40px rows, a neutral monogram tile, name over platform, the
  // category as a hairline tag, price over its cadence, the monthly equivalent in the
  // display currency, the next billing date and how far it is, status tags in words, the
  // latest usage rating as five dots, the yearly-plan saving, and row actions on hover.
  // Optional grouping by category under hairline group headers.
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { fmtDate, fmtSignedMoney } from '$lib/format.js';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import { MASK } from '$lib/ui/charts/palette.js';
  import { money, daysUntil, initial } from './fmt.svelte.js';
  import {
    monthlyOf,
    myPrice,
    isEnded,
    switchSavings,
    cardExpiryState,
    trialDaysLeft,
    shareFactor,
    nextBillingDate,
    categoryOf,
    UNCATEGORIZED
  } from './calc.js';
  import { t } from '$lib/i18n';

  let {
    rows = [],
    cur = 'USD',
    // (amount, currency) -> amount in `cur`, or null with no rate.
    toDisplay = (a) => a,
    sort = $bindable('name'),
    dir = $bindable('asc'),
    grouped = false,
    freqLabel = (f) => f,
    reminderDefaults = () => ({}),
    ondetail = () => {},
    onedit = () => {},
    onend = () => {},
    onresume = () => {},
    ondelete = () => {}
  } = $props();

  const na = $derived($t('subscriptions.page.na'));
  const COLS = 9;
  const catLabel = (n) => (n === UNCATEGORIZED ? $t('subscriptions.page.uncategorized') : n);

  // Converted to the display currency when there is a rate; null otherwise.
  const conv = (s, amount) => (s.currency === cur ? amount : toDisplay(amount, s.currency));

  const groups = $derived.by(() => {
    if (!grouped) return [{ key: '', rows }];
    const map = new Map();
    for (const s of rows) {
      const k = categoryOf(s);
      if (!map.has(k)) map.set(k, []);
      map.get(k).push(s);
    }
    const out = [...map].map(([key, list]) => {
      let sum = 0;
      for (const s of list) if (!isEnded(s) && s.active) sum += conv(s, monthlyOf(s)) ?? 0;
      return { key, rows: list, sum };
    });
    // Largest monthly cost first, uncategorized last.
    return out.sort(
      (a, b) => (a.key === UNCATEGORIZED) - (b.key === UNCATEGORIZED) || b.sum - a.sum || a.key.localeCompare(b.key)
    );
  });

  function sortBy(col) {
    if (sort === col) dir = dir === 'asc' ? 'desc' : 'asc';
    else {
      sort = col;
      dir = col === 'price' || col === 'monthly' || col === 'saves' || col === 'rating' ? 'desc' : 'asc';
    }
  }
  const ariaSort = (col) => (sort !== col ? 'none' : dir === 'asc' ? 'ascending' : 'descending');

  function dueText(d) {
    const n = daysUntil(d);
    if (n === null) return '';
    if (n === 0) return $t('subscriptions.page.dueToday');
    return $t('subscriptions.page.inDays', { count: n });
  }
  function savesText(v, ccy) {
    if (privacy.hidden) return MASK;
    return fmtSignedMoney(v, ccy);
  }
</script>

{#snippet head(col, label, numeric = false)}
  <th class:num={numeric} aria-sort={ariaSort(col)}>
    <button type="button" class="sortable" onclick={() => sortBy(col)}>
      <span>{label}</span>
      {#if sort === col}<Icon name={dir === 'asc' ? 'chevron-up' : 'chevron-down'} size={11} />{/if}
    </button>
  </th>
{/snippet}

{#snippet dots(score, rated)}
  <span
    class="dots"
    role="img"
    aria-label={score ? $t('subscriptions.page.ratingOf', { score }) : $t('subscriptions.page.notRated')}
    title={score ? `${$t('subscriptions.page.ratingOf', { score })} (${fmtDate(rated)})` : $t('subscriptions.page.notRated')}
  >
    {#each [1, 2, 3, 4, 5] as n}<i class:on={score >= n}></i>{/each}
  </span>
{/snippet}

<div class="table-wrap">
  <table class="tbl list">
    <thead>
      <tr>
        {@render head('name', $t('subscriptions.form.name'))}
        {@render head('category', $t('subscriptions.form.category'))}
        {@render head('price', $t('subscriptions.form.price'), true)}
        {@render head('monthly', $t('subscriptions.page.monthlyEq'), true)}
        {@render head('next', $t('subscriptions.page.nextBilling'), true)}
        <th>{$t('subscriptions.page.status')}</th>
        {@render head('rating', $t('subscriptions.page.rating'))}
        {@render head('saves', $t('subscriptions.page.switchSaves'), true)}
        <th class="act-h"><span class="sr">{$t('subscriptions.page.actions')}</span></th>
      </tr>
    </thead>
    <tbody>
      {#each groups as g (g.key)}
        {#if grouped}
          <tr class="group">
            <th colspan={COLS} scope="colgroup">
              <span class="g-name">{catLabel(g.key)}</span>
              <span class="g-meta">{$t('subscriptions.page.groupMeta', { count: g.rows.length, amount: money(g.sum, cur, na) })}</span>
            </th>
          </tr>
        {/if}
        {#each g.rows as s (s.id)}
          {@const ended = isEnded(s)}
          {@const trialLeft = trialDaysLeft(s)}
          {@const card = cardExpiryState(s.card_expiry)}
          {@const saves = switchSavings(s)}
          {@const next = nextBillingDate(s)}
          {@const monthly = conv(s, monthlyOf(s))}
          <tr class:dim={!s.active || ended}>
            <td class="name-cell">
              <div class="name-wrap">
              <span class="mono-tile" aria-hidden="true">{initial(s.name)}</span>
              <span class="name-text">
                {#if s.url}
                  <a class="name" href={s.url} target="_blank" rel="noreferrer" title={s.name}>{s.name}</a>
                {:else}
                  <span class="name" title={s.name}>{s.name}</span>
                {/if}
                {#if s.platform || s.payment_method}
                  <span class="sub" title={[s.platform, s.payment_method].filter(Boolean).join(' · ')}>
                    {[s.platform, s.payment_method].filter(Boolean).join(' · ')}
                  </span>
                {/if}
              </span>
              </div>
            </td>
            <td data-label={$t('subscriptions.form.category')}>
              {#if s.category}<Badge>{s.category}</Badge>{:else}<span class="faint">{catLabel(UNCATEGORIZED)}</span>{/if}
            </td>
            <td class="num" data-label={$t('subscriptions.form.price')}>
              <span class="fig">{money(myPrice(s), s.currency, na)}</span>
              <span class="under">{freqLabel(s.frequency)}</span>
            </td>
            <td class="num" data-label={$t('subscriptions.page.monthlyEq')} title={s.currency !== cur ? money(monthlyOf(s), s.currency, na) : undefined}>
              {#if monthly === null || monthly === undefined}
                <span class="fig">{money(monthlyOf(s), s.currency, na)}</span>
                <span class="under">{$t('subscriptions.page.noRate')}</span>
              {:else}
                <span class="fig">{money(monthly, cur, na)}</span>
                {#if s.currency !== cur}<span class="under">{money(monthlyOf(s), s.currency, na)}</span>{/if}
              {/if}
            </td>
            <td class="num" data-label={$t('subscriptions.page.nextBilling')}>
              {#if next}
                <span class="fig">{fmtDate(next)}</span>
                <span class="under">{dueText(next)}</span>
              {:else}
                <span class="faint">{na}</span>
              {/if}
            </td>
            <td class="status" data-label={$t('subscriptions.page.status')}>
              <span class="badges">
                {#if ended}<Badge>{$t('subscriptions.page.endedOn', { date: fmtDate(s.ended_at) })}</Badge>{/if}
                {#if !s.active && !ended}<Badge>{$t('subscriptions.page.paused')}</Badge>{/if}
                {#if trialLeft !== null}<Badge tone="warn">{$t('subscriptions.page.trialBadge', { days: trialLeft })}</Badge>{/if}
                {#if card === 'expired'}<Badge tone="danger">{$t('subscriptions.page.cardExpired')}</Badge>{/if}
                {#if card === 'soon'}<Badge tone="warn">{$t('subscriptions.page.cardSoon')}</Badge>{/if}
                {#if shareFactor(s.my_share_pct) < 1}<Badge>{$t('subscriptions.page.shareBadge', { pct: Math.round(s.my_share_pct) })}</Badge>{/if}
              </span>
            </td>
            <td data-label={$t('subscriptions.page.rating')}>{@render dots(s.last_score, s.last_rated_at)}</td>
            <td class="num" data-label={$t('subscriptions.page.switchSaves')}>
              {#if saves === null}
                <span class="faint">{na}</span>
              {:else}
                <span class="fig" class:pos={saves > 0} class:neg={saves < 0}>{savesText(saves, s.currency)}</span>
                <span class="under">{$t('subscriptions.page.perYear')}</span>
              {/if}
            </td>
            <td class="actions">
              <span class="act">
                <button class="icon" aria-label={$t('subscriptions.page.details')} title={$t('subscriptions.page.details')} onclick={() => ondetail(s)}><Icon name="history" size={14} /></button>
                {#if next}
                  <QuickReminderButton variant="row" title={$t('subscriptions.page.remindBilling')} defaults={reminderDefaults(s)} />
                {/if}
                {#if s.url}
                  <!-- A real <a>, not a button: middle-click and copy-link must work. -->
                  <a class="icon" href={s.url} target="_blank" rel="noreferrer" aria-label={$t('subscriptions.page.openLink')} title={$t('subscriptions.page.openLink')}><Icon name="external-link" size={14} /></a>
                {/if}
                <button class="icon" aria-label={$t('subscriptions.page.edit')} title={$t('subscriptions.page.edit')} onclick={() => onedit(s)}><Icon name="pencil" size={14} /></button>
                {#if ended}
                  <button class="icon" aria-label={$t('subscriptions.page.resume')} title={$t('subscriptions.page.resume')} onclick={() => onresume(s)}><Icon name="rotate-ccw" size={14} /></button>
                {:else}
                  <button class="icon" aria-label={$t('subscriptions.page.cancelSub')} title={$t('subscriptions.page.cancelSub')} onclick={() => onend(s)}><Icon name="stop" size={14} /></button>
                {/if}
                <button class="icon danger-hover" aria-label={$t('subscriptions.page.delete')} title={$t('subscriptions.page.delete')} onclick={() => ondelete(s)}><Icon name="trash" size={14} /></button>
              </span>
            </td>
          </tr>
        {/each}
      {/each}
      {#if rows.length === 0}
        <tr><td colspan={COLS} class="empty-row">{$t('subscriptions.page.emptySearch')}</td></tr>
      {/if}
    </tbody>
  </table>
</div>

<style>
  .table-wrap {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    overflow-x: auto;
  }
  .list {
    font-size: 13px;
  }
  .list th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
  }
  .list td {
    height: 40px;
    padding-top: 2px;
    padding-bottom: 2px;
  }
  .list tbody tr {
    transition: background-color 160ms ease;
  }
  .list tbody tr:not(.group):hover {
    background: var(--surface-hover);
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tr.dim {
    opacity: 0.6;
  }
  .sortable {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    color: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }
  th.num .sortable {
    flex-direction: row-reverse;
  }
  .sortable:hover {
    color: var(--text);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  /* Group header: a hairline label row, no fill. */
  tr.group th {
    position: static;
    height: 30px;
    padding: var(--space-3) var(--space-3) var(--space-1);
    text-align: left;
    border-bottom: var(--hairline) solid var(--border);
  }
  .g-name {
    color: var(--text);
    font-size: 11px;
  }
  .g-meta {
    margin-left: var(--space-2);
    color: var(--dim);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-transform: none;
    letter-spacing: 0;
  }

  .name-cell {
    min-width: 200px;
    max-width: 320px;
  }
  .name-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .mono-tile {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .name-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  a.name:hover {
    text-decoration: underline;
  }
  .sub {
    font-size: 11px;
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  td.num {
    white-space: nowrap;
  }
  .fig {
    display: block;
    line-height: 1.25;
  }
  .under {
    display: block;
    font-family: var(--font);
    font-size: 10px;
    line-height: 1.2;
    color: var(--dim);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .faint {
    color: var(--faint);
    font-size: 12px;
  }
  .badges {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .dots {
    display: inline-flex;
    gap: 3px;
    vertical-align: middle;
  }
  .dots i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--surface-3);
    box-sizing: border-box;
    border: 1px solid var(--border-control);
  }
  .dots i.on {
    background: var(--accent);
    border-color: var(--accent);
  }

  .actions {
    width: 1%;
    white-space: nowrap;
    text-align: right;
  }
  .act {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    opacity: 0;
    transition: opacity 160ms ease;
  }
  tr:hover .act,
  .act:focus-within {
    opacity: 1;
  }
  .act :global(button.icon),
  .act :global(a.icon) {
    width: 26px;
    height: 26px;
  }
  .empty-row {
    text-align: center;
    color: var(--muted);
    padding: var(--space-6);
  }

  @media (hover: none) {
    .act {
      opacity: 1;
    }
  }

  /* Narrow: each row becomes a record card, the sort headers a chip row. */
  @media (max-width: 900px) {
    .table-wrap {
      overflow-x: visible;
    }
    .list,
    .list thead,
    .list tbody {
      display: block;
    }
    .list thead tr {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      padding: var(--space-2);
      border-bottom: var(--hairline) solid var(--border);
    }
    .list thead th {
      position: static;
      display: block;
      height: auto;
      padding: 0;
      border: none;
    }
    .list thead th:not(:has(.sortable)) {
      display: none;
    }
    .sortable {
      min-height: 44px;
      padding: 0 var(--space-3);
      border: var(--hairline) solid var(--border-control);
      border-radius: var(--radius-sm);
    }
    th.num .sortable {
      flex-direction: row;
    }
    .list tbody tr {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--space-2) var(--space-3);
      padding: var(--space-3);
      border-bottom: var(--hairline) solid var(--border);
    }
    .list tbody tr.group {
      display: block;
      padding: 0;
    }
    tr.group th {
      display: block;
    }
    .list tbody td {
      display: block;
      height: auto;
      min-width: 0;
      padding: 0;
      border: none;
      text-align: left;
    }
    .name-cell {
      max-width: none;
    }
    .list tbody td.name-cell,
    .list tbody td.status,
    .list tbody td.actions,
    .list tbody td.empty-row {
      grid-column: 1 / -1;
    }
    .list tbody td[data-label]::before {
      content: attr(data-label);
      display: block;
      font-size: 10px;
      color: var(--dim);
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }
    .list tbody td.status:not(:has(:global(.badge))) {
      display: none;
    }
    .actions {
      width: auto;
    }
    .act {
      opacity: 1;
      justify-content: flex-end;
      width: 100%;
    }
    .act :global(button.icon),
    .act :global(a.icon) {
      width: 44px;
      height: 44px;
    }
  }
</style>
