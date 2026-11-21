<script>
  // One asset: its value at 20px and the change since the revision before, then tabs. The
  // revisions (record an update, edit or delete one), a loan's remaining schedule, the
  // equity and loan-to-value against the debt it is paired with, and the documents kept
  // with it.
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Button from '$lib/ui/Button.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import UpdateForm from './UpdateForm.svelte';
  import { wealthApi } from './api.js';
  import { pairMath, revisionChange } from './engine.js';
  import { money, pct } from './view.js';
  import { fmtDate } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    asset,
    /** Every asset, to find the paired debt. */
    assets = [],
    revisions = [],
    template = null,
    /** (amount, fromCurrency) => amount in `currency`, or null without a rate. */
    toDisplay = (v) => v,
    currency = 'USD',
    onsubmit = () => {},
    onupdate = () => {},
    ondelete = () => {},
    oncancel = () => {}
  } = $props();

  let tab = $state('revisions'); // revisions | schedule | pair | docs

  let schedule = $state(null);
  let scheduleError = $state('');
  let showAll = $state(false);
  let docs = $state([]);
  let docsError = $state('');
  let uploading = $state(false);
  let revisionId = $state('');
  let fileInput = $state(null);

  const MAX = 10 * 1024 * 1024;

  $effect(() => {
    const id = asset?.id;
    if (!id) return;
    tab = 'revisions';
    schedule = null;
    scheduleError = '';
    if (asset.loan) {
      wealthApi.schedule(id).then((s) => (schedule = s)).catch((e) => (scheduleError = e.message));
    }
    loadDocs(id);
  });

  async function loadDocs(id = asset.id) {
    docsError = '';
    try {
      docs = await wealthApi.attachments(id);
    } catch (e) {
      docsError = e.message;
    }
  }

  const debt = $derived(asset?.paired_liability_id ? assets.find((a) => a.id === asset.paired_liability_id) : null);
  // Both sides in the display currency: a house in EUR and its mortgage in CHF still compare.
  const pair = $derived.by(() => {
    if (!debt) return null;
    const own = asset.estimated_value ?? asset.latest_value;
    const a = own == null ? null : toDisplay(own, asset.currency);
    const d = debt.latest_value == null ? null : toDisplay(debt.latest_value, debt.currency);
    return { ...pairMath(a, d), value: a, debt: d };
  });

  const rows = $derived(schedule ? (showAll ? schedule.rows : schedule.rows.slice(0, 12)) : []);

  async function onFile(e) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (!file) return;
    docsError = '';
    if (file.size > MAX) {
      docsError = $t('wealth.docs.tooBig');
      return;
    }
    uploading = true;
    try {
      await wealthApi.upload(asset.id, file, revisionId || null);
      await loadDocs();
    } catch (err) {
      docsError = err.message;
    }
    uploading = false;
  }

  async function download(d) {
    docsError = '';
    try {
      await wealthApi.download(wealthApi.attachmentPath(d.id), d.name);
    } catch (e) {
      docsError = e.message;
    }
  }

  async function remove(d) {
    docsError = '';
    try {
      await wealthApi.deleteAttachment(d.id);
      await loadDocs();
    } catch (e) {
      docsError = e.message;
    }
  }

  const kb = (n) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);
  const revLabel = (id) => revisions.find((r) => r.id === id)?.valued_at ?? '';

  const liability = $derived(asset?.sign < 0);
  const shown = $derived(asset?.estimated_value ?? asset?.latest_value ?? null);
  const signed = $derived(shown == null ? null : liability ? -shown : shown);
  const conv = $derived(signed == null || asset.currency === currency ? null : toDisplay(signed, asset.currency));
  const change = $derived(revisionChange(revisions));
  const principal = $derived(Number(asset?.loan?.principal) || null);
  const tabs = $derived([
    { id: 'revisions', label: $t('wealth.detail.revisions'), n: revisions.length },
    ...(asset?.loan ? [{ id: 'schedule', label: $t('wealth.loan.schedule') }] : []),
    ...(debt ? [{ id: 'pair', label: $t('wealth.detail.pair') }] : []),
    { id: 'docs', label: $t('wealth.docs.title'), n: docs.length }
  ]);
</script>

<div class="detail">
  <div class="summary">
    <div class="sum-main">
      <span class="eyebrow">{asset.estimated_value != null ? $t('wealth.drift.estimated') : $t('wealth.page.colCurrentValue')}{asset.latest_at ? ` · ${asset.latest_at}` : ''}</span>
      <div class="sum-line">
        <strong class="value" class:est={asset.estimated_value != null}>{money(signed, asset.currency, $t('wealth.page.na'))}</strong>
        {#if conv != null}<span class="conv">{money(conv, currency)}</span>{/if}
      </div>
    </div>
    <div class="sum-change">
      {#if change.abs != null}
        <Delta value={liability ? -change.abs : change.abs} format={(v) => money(v, asset.currency)} size="md" sensitive />
        {#if change.pct != null}<Delta value={liability ? -change.pct : change.pct} digits={1} arrow={false} />{/if}
        <span class="since">{$t('wealth.headline.since', { date: change.since })}</span>
      {:else}
        <span class="since">{$t('wealth.detail.noPrevious')}</span>
      {/if}
    </div>
  </div>
  {#if asset.estimated_value != null}
    <p class="note">{$t('wealth.drift.rowNote', { value: money(asset.latest_value, asset.currency) })}</p>
  {/if}
  {#if asset.live_error}
    <p class="warn"><Icon name="alert-triangle" size={13} /> {$t('wealth.live.error', { error: asset.live_error })}</p>
  {/if}

  <div class="dtabs" role="tablist" aria-label={asset.name}>
    {#each tabs as tb (tb.id)}
      <button role="tab" aria-selected={tab === tb.id} class:active={tab === tb.id} onclick={() => (tab = tb.id)}>
        {tb.label}{#if tb.n}<span class="n">{tb.n}</span>{/if}
      </button>
    {/each}
  </div>

  {#if tab === 'revisions'}
    <UpdateForm {asset} {template} {revisions} {onsubmit} {onupdate} {ondelete} {oncancel} />
  {:else if tab === 'schedule'}
    <section class="pane">
      <ErrorText error={scheduleError} />
      {#if schedule}
        <div class="tiles">
          <StatCard label={$t('wealth.loan.payment')} value={money(schedule.payment, schedule.currency)} />
          <StatCard label={$t('wealth.loan.balance')} value={money(schedule.balance, schedule.currency)} hint={principal ? $t('wealth.detail.ofPrincipal', { pct: pct((schedule.balance / principal) * 100) }) : ''} />
          <StatCard label={$t('wealth.loan.interestLeft')} value={money(schedule.interest_left, schedule.currency)} />
          <StatCard label={$t('wealth.detail.paymentsLeft')} value={String(schedule.payments_left)} />
        </div>
        {#if schedule.rows.length}
          <div class="tbl-wrap">
            <table class="tbl sched">
              <thead>
                <tr>
                  <th class="c-n">#</th>
                  <th class="c-due">{$t('wealth.loan.due')}</th>
                  <th class="num">{$t('wealth.loan.payment')}</th>
                  <th class="num">{$t('wealth.loan.interest')}</th>
                  <th class="num">{$t('wealth.loan.principalPart')}</th>
                  <th class="num">{$t('wealth.loan.balance')}</th>
                  <th class="c-meter">{$t('wealth.detail.remaining')}</th>
                </tr>
              </thead>
              <tbody>
                {#each rows as r (r.n)}
                  <tr>
                    <td class="mono dim">{r.n}</td>
                    <td class="mono dim">{r.date}</td>
                    <td class="num">{money(r.payment, schedule.currency)}</td>
                    <td class="num">{money(r.interest, schedule.currency)}</td>
                    <td class="num">{money(r.principal, schedule.currency)}</td>
                    <td class="num">{money(r.balance, schedule.currency)}</td>
                    <td>
                      {#if principal}
                        <span class="meter" title={pct((r.balance / principal) * 100)}><span class="fill" style:width="{Math.max(0, Math.min(100, (r.balance / principal) * 100))}%"></span></span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if schedule.rows.length > 12}
            <button class="link" onclick={() => (showAll = !showAll)}>{showAll ? $t('wealth.loan.showLess') : $t('wealth.loan.showAll', { n: schedule.rows.length })}</button>
          {/if}
        {:else}
          <EmptyState icon="check-circle" description={$t('wealth.loan.paidOff')} compact />
        {/if}
      {/if}
    </section>
  {:else if tab === 'pair' && debt}
    <section class="pane">
      <p class="pair-title">{$t('wealth.pair.title', { name: debt.name })}</p>
      {#if pair.value == null || pair.debt == null}
        <EmptyState icon="link" description={$t('wealth.pair.unavailable')} compact />
      {:else}
        <div class="tiles two">
          <StatCard label={$t('wealth.pair.equity')} value={money(pair.equity, currency)} hint={$t('wealth.detail.pairHint', { value: money(pair.value, currency), debt: money(pair.debt, currency) })} />
          <StatCard label={$t('wealth.pair.ltv')} value={pair.ltv == null ? $t('wealth.pair.noLtv') : pct(pair.ltv)} />
        </div>
      {/if}
    </section>
  {:else}
    <section class="pane">
      <div class="upload">
        {#if revisions.length}
          <div class="rev-pick">
            <Dropdown
              value={revisionId}
              onpick={(v) => (revisionId = v)}
              ariaLabel={$t('wealth.docs.revision')}
              options={[{ value: '', label: $t('wealth.docs.wholeAsset') }, ...revisions.map((r) => ({ value: r.id, label: `${r.valued_at} ${money(r.value, asset.currency)}` }))]}
            />
          </div>
        {/if}
        <input class="hidden" type="file" bind:this={fileInput} onchange={onFile} />
        <Button size="sm" icon="upload" loading={uploading} onclick={() => fileInput?.click()}>{$t('wealth.docs.add')}</Button>
        <span class="limit">{$t('wealth.docs.limit')}</span>
      </div>
      <ErrorText error={docsError} />
      {#if docs.length}
        <table class="tbl docs">
          <tbody>
            {#each docs as d (d.id)}
              <tr>
                <td class="d-name" title={d.name}><Icon name="file-text" size={13} /> <span>{d.name}</span></td>
                <td class="d-rev">{#if d.revision_id}{revLabel(d.revision_id)}{/if}</td>
                <td class="num d-size">{kb(d.size)}</td>
                <td class="mono dim d-date">{fmtDate(d.created_at)}</td>
                <td class="acts">
                  <button class="icon" title={$t('wealth.docs.download')} aria-label="{$t('wealth.docs.download')}: {d.name}" onclick={() => download(d)}><Icon name="download" size={14} /></button>
                  <button class="icon danger-hover" title={$t('wealth.page.deleteTitle')} aria-label="{$t('wealth.page.deleteTitle')}: {d.name}" onclick={() => remove(d)}><Icon name="trash" size={14} /></button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <EmptyState icon="file-text" description={$t('wealth.docs.none')} compact />
      {/if}
    </section>
  {/if}
</div>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .summary {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .sum-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .eyebrow {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .sum-line {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .value {
    font-size: 20px;
    font-weight: 600;
    line-height: 1.2;
    color: var(--text);
  }
  .value.est {
    color: var(--muted);
    text-decoration: underline dashed var(--faint);
    text-underline-offset: 4px;
  }
  .conv {
    color: var(--dim);
    font-family: var(--mono);
    font-size: 12px;
  }
  .sum-change {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .since,
  .note,
  .limit {
    color: var(--dim);
    font-size: 12px;
  }
  .note {
    margin: 0;
  }
  .warn {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--amber-ink);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .dtabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
  }
  .dtabs button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    border-bottom: 1.5px solid transparent;
    border-radius: 0;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    margin-bottom: -1px;
    font-size: 13px;
    cursor: pointer;
    transition: color 160ms ease;
  }
  .dtabs button:hover {
    color: var(--text);
  }
  .dtabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .n {
    color: var(--dim);
    font-family: var(--mono);
    font-size: 11px;
  }
  .pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .tiles.two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .pair-title {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  .tbl-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
  }
  .sched {
    min-width: 640px;
  }
  .c-n {
    width: 40px;
  }
  .c-due {
    width: 100px;
  }
  .c-meter {
    width: 96px;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .dim {
    color: var(--muted);
  }
  /* Remaining principal: a thin meter, the track a lighter step of the same hue. */
  .meter {
    display: block;
    height: 4px;
    border-radius: 2px;
    background: var(--accent-soft);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }
  .link {
    align-self: flex-start;
    background: none;
    border: none;
    padding: 0;
    color: var(--muted);
    cursor: pointer;
    font-size: 12px;
  }
  .link:hover {
    color: var(--text);
  }
  .upload {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .rev-pick {
    min-width: 200px;
  }
  .hidden {
    display: none;
  }
  .docs {
    table-layout: fixed;
  }
  .d-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .d-name :global(svg) {
    vertical-align: -2px;
    color: var(--dim);
  }
  .d-rev {
    width: 110px;
    color: var(--dim);
    font-family: var(--mono);
    font-size: 12px;
  }
  .d-size {
    width: 80px;
  }
  .d-date {
    width: 110px;
  }
  .acts {
    width: 72px;
    text-align: right;
    white-space: nowrap;
  }
  @media (max-width: 767px) {
    .tiles {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .dtabs {
      overflow-x: auto;
    }
    .dtabs button {
      min-height: 44px;
      white-space: nowrap;
    }
    .d-rev,
    .d-date {
      display: none;
    }
    .upload :global(button),
    .acts button {
      min-height: 44px;
      min-width: 44px;
    }
  }
</style>
