<script>
  // One holding up close: its thesis and review date, the data no quote carries (dividends,
  // sector, country), its tax lots, the trades planned on it, its history, and a transfer
  // to another portfolio. Each section saves on its own.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { portfoliosApi, fmtMoney, fmtSignedMoney, fmtNum, fmtDate, DIV_FREQUENCIES } from './api.js';

  let {
    portfolioId,
    asset,
    currency = 'USD',
    classification = null,
    overLimit = false,
    onclose,
    onchanged,
    onbook = null
  } = $props();

  let section = $state('profile');
  let error = $state(null);
  let saving = $state(false);

  // ── Profile ──
  // svelte-ignore state_referenced_locally
  let form = $state({
    thesis: asset.thesis ?? '',
    review_at: asset.review_at ?? '',
    sector: asset.sector ?? '',
    country: asset.country ?? '',
    div_per_share: asset.div_per_share ?? '',
    div_frequency: asset.div_frequency ?? 'none',
    div_next_ex: asset.div_next_ex ?? '',
    div_next_pay: asset.div_next_pay ?? ''
  });

  async function saveProfile() {
    saving = true;
    error = null;
    try {
      await portfoliosApi.updateAsset(asset.id, {
        thesis: form.thesis,
        review_at: form.review_at || null,
        sector: form.sector,
        country: form.country,
        div_per_share: form.div_per_share === '' ? null : Number(form.div_per_share),
        div_frequency: form.div_frequency,
        div_next_ex: form.div_next_ex || null,
        div_next_pay: form.div_next_pay || null
      });
      toast.ok($t('portfolios.asset.saved'));
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // ── Lots ──
  let lots = $state(null);
  $effect(() => {
    if (section !== 'lots' || lots) return;
    portfoliosApi
      .lots(portfolioId, asset.id)
      .then((r) => (lots = r))
      .catch((e) => (error = e.message));
  });

  // ── Plans ──
  let plans = $state(null);
  let plan = $state({ side: 'buy', price: '', qty: '', note: '', notify: true });
  const assetPlans = $derived((plans ?? []).filter((p) => p.asset_id === asset.id));
  function loadPlans() {
    portfoliosApi
      .plans(portfolioId)
      .then((r) => (plans = r))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    if (section === 'plans' && !plans) loadPlans();
  });
  async function addPlan() {
    if (!(Number(plan.price) > 0)) return;
    error = null;
    try {
      await portfoliosApi.addPlan(portfolioId, {
        asset_id: asset.id,
        side: plan.side,
        price: Number(plan.price),
        qty: plan.qty === '' ? null : Number(plan.qty),
        note: plan.note,
        notify: plan.notify
      });
      plan = { side: 'buy', price: '', qty: '', note: '', notify: true };
      loadPlans();
    } catch (e) {
      error = e.message;
    }
  }
  async function removePlan(p) {
    try {
      await portfoliosApi.deletePlan(portfolioId, p.id);
      loadPlans();
    } catch (e) {
      error = e.message;
    }
  }

  // ── History ──
  let history = $state(null);
  $effect(() => {
    if (section !== 'history' || history) return;
    portfoliosApi
      .assetDetail(asset.id)
      .then((r) => (history = r.operations ?? []))
      .catch((e) => (error = e.message));
  });

  // ── Transfer ──
  let others = $state(null);
  let move = $state({ to: '', qty: '', note: '' });
  $effect(() => {
    if (section !== 'transfer' || others) return;
    portfoliosApi
      .list()
      .then((r) => (others = r.filter((p) => p.id !== portfolioId)))
      .catch((e) => (error = e.message));
  });
  async function doTransfer() {
    if (!move.to || !(Number(move.qty) > 0)) return;
    saving = true;
    error = null;
    try {
      await portfoliosApi.transfer(portfolioId, asset.id, {
        to_portfolio_id: move.to,
        qty: Number(move.qty),
        note: move.note
      });
      toast.ok($t('portfolios.transfer.done'));
      onchanged?.();
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  const SECTIONS = ['profile', 'lots', 'plans', 'history', 'transfer'];
  const corpText = (o) =>
    o.side === 'split' || o.side === 'reverse_split'
      ? `${fmtNum(o.ratio_num, 4)} : ${fmtNum(o.ratio_den, 4)}`
      : o.side === 'spin_off'
        ? `${fmtNum(o.child_qty)} (${fmtNum(o.cost_share_pct, 2)}%)`
        : null;
</script>

<Modal open size="lg" title={`${asset.symbol} ${asset.name ?? ''}`} {onclose}>
  <nav class="secs">
    {#each SECTIONS as s (s)}
      <button class:active={section === s} onclick={() => (section = s)}>{$t(`portfolios.asset.sec.${s}`)}</button>
    {/each}
    {#if overLimit}<Badge tone="warn" icon="alert-triangle">{$t('portfolios.concentration.badge')}</Badge>{/if}
  </nav>

  <ErrorText {error} />

  {#if section === 'profile'}
    <div class="stack">
    <Field label={$t('portfolios.asset.thesis')} hint={$t('portfolios.asset.thesisHint')}>
      <textarea rows="5" bind:value={form.thesis}></textarea>
    </Field>
    <div class="grid">
      <Field label={$t('portfolios.asset.reviewAt')}>
        <input type="date" bind:value={form.review_at} />
      </Field>
      <Field
        label={$t('portfolios.asset.sector')}
        hint={!form.sector && classification?.sector_source === 'fundamentals'
          ? $t('portfolios.asset.fromFundamentals', { value: classification.sector })
          : $t('portfolios.asset.classHint')}
      >
        <input type="text" bind:value={form.sector} />
      </Field>
      <Field
        label={$t('portfolios.asset.country')}
        hint={!form.country && classification?.country_source === 'fundamentals'
          ? $t('portfolios.asset.fromFundamentals', { value: classification.country })
          : ''}
      >
        <input type="text" bind:value={form.country} />
      </Field>
    </div>
    <div class="sechead">
      <p class="sub">{$t('portfolios.asset.dividends')}</p>
      <p class="hint">{$t('portfolios.asset.dividendsHint', { ccy: asset.currency })}</p>
    </div>
    <div class="grid">
      <Field label={$t('portfolios.asset.perShare', { ccy: asset.currency })}>
        <input type="number" step="any" min="0" bind:value={form.div_per_share} />
      </Field>
      <Field label={$t('portfolios.asset.frequency')}>
        <Select
          bind:value={form.div_frequency}
          options={DIV_FREQUENCIES.map((f) => ({ value: f, label: $t(`portfolios.freq.${f}`) }))}
        />
      </Field>
      <Field label={$t('portfolios.asset.nextEx')}>
        <input type="date" bind:value={form.div_next_ex} />
      </Field>
      <Field label={$t('portfolios.asset.nextPay')}>
        <input type="date" bind:value={form.div_next_pay} />
      </Field>
    </div>
    <div class="actions">
      <button class="btn primary" onclick={saveProfile} disabled={saving}>
        {saving ? $t('common.saving') : $t('common.save')}
      </button>
    </div>
    </div>
  {:else if section === 'lots'}
    {#if !lots}
      <p class="hint">{$t('common.loading')}</p>
    {:else}
      <p class="hint">
        {$t('portfolios.lots.method', { method: $t(`portfolios.lots.methods.${lots.rules.method}`) })}
        {#if lots.rules.defaulted}
          {$t('portfolios.lots.defaulted')}
        {:else}
          {$t('portfolios.lots.fromProfile', { name: lots.rules.profile_name })}
        {/if}
        <a href="/taxcalc">{$t('portfolios.lots.change')}</a>
      </p>
      {#if lots.long_after_days}
        <p class="hint">{$t('portfolios.lots.longAfter', { days: lots.long_after_days })}</p>
      {/if}
      {#if lots.lots.length === 0}
        <p class="hint">{$t('portfolios.lots.none')}</p>
      {:else}
        <div class="tscroll">
          <table class="mtable">
            <thead>
              <tr>
                <th class="l">{$t('portfolios.lots.opened')}</th>
                <th>{$t('portfolios.detail.colQty')}</th>
                <th>{$t('portfolios.lots.costUnit')}</th>
                <th>{$t('portfolios.lots.cost')}</th>
                <th>{$t('portfolios.detail.colValue')}</th>
                <th>{$t('portfolios.detail.colUnreal')}</th>
                <th>{$t('portfolios.lots.days')}</th>
                <th class="l">{$t('portfolios.lots.term')}</th>
              </tr>
            </thead>
            <tbody>
              {#each lots.lots as l, i (i)}
                <tr>
                  <td class="l date">{fmtDate(l.opened)}</td>
                  <td class="num">{fmtNum(l.qty)}</td>
                  <td class="num">{fmtMoney(l.cost_per_unit, currency)}</td>
                  <td class="num">{fmtMoney(l.cost, currency)}</td>
                  <td class="num">{l.value == null ? $t('portfolios.lots.noQuote') : fmtMoney(l.value, currency)}</td>
                  <td class="num" class:up={l.unrealized > 0} class:down={l.unrealized < 0}>
                    {l.unrealized == null ? '' : fmtSignedMoney(l.unrealized, currency)}
                  </td>
                  <td class="num">{l.holding_days}</td>
                  <td class="l">{l.term ? $t(`portfolios.lots.terms.${l.term === 'short' ? 'short' : 'long'}`) : $t('portfolios.lots.noTerm')}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  {:else if section === 'plans'}
    <p class="hint">{$t('portfolios.plans.hint')}</p>
    {#if assetPlans.length}
      <ul class="plans">
        {#each assetPlans as p (p.id)}
          <li>
            <Badge tone={p.side === 'buy' ? 'success' : 'danger'}>{$t(`portfolios.kind.${p.side}`)}</Badge>
            <span class="num">{fmtNum(p.price, 4)} {asset.currency}</span>
            {#if p.qty}<span class="num">x {fmtNum(p.qty)}</span>{/if}
            {#if p.hit_at}
              <Badge tone="accent">{$t('portfolios.plans.hit')}</Badge>
            {:else}
              <Badge>{$t('portfolios.plans.waiting')}</Badge>
            {/if}
            {#if p.note}<span class="dim">{p.note}</span>{/if}
            <span class="grow"></span>
            {#if onbook}
              <button class="btn sm" onclick={() => onbook(p.side, p.price, p.qty)}>{$t('portfolios.plans.book')}</button>
            {/if}
            <button class="icon danger-hover" onclick={() => removePlan(p)} aria-label={$t('common.delete')} title={$t('common.delete')}><Icon name="trash" size={14} /></button>
          </li>
        {/each}
      </ul>
    {:else if plans}
      <p class="hint">{$t('portfolios.plans.none')}</p>
    {/if}
    <div class="grid">
      <Field label={$t('portfolios.detail.colSide')}>
        <Select
          bind:value={plan.side}
          options={[
            { value: 'buy', label: $t('portfolios.kind.buy') },
            { value: 'sell', label: $t('portfolios.kind.sell') }
          ]}
        />
      </Field>
      <Field label={$t('portfolios.plans.level', { ccy: asset.currency })}>
        <input type="number" step="any" min="0" bind:value={plan.price} />
      </Field>
      <Field label={$t('portfolios.plans.qty')}>
        <input type="number" step="any" min="0" bind:value={plan.qty} />
      </Field>
      <Field label={$t('portfolios.detail.note')}>
        <input type="text" bind:value={plan.note} />
      </Field>
    </div>
    <label class="check"><input type="checkbox" bind:checked={plan.notify} /> {$t('portfolios.plans.notify')}</label>
    <div class="actions">
      <button class="btn primary" onclick={addPlan} disabled={!(Number(plan.price) > 0)}>{$t('portfolios.plans.add')}</button>
    </div>
  {:else if section === 'history'}
    {#if !history}
      <p class="hint">{$t('common.loading')}</p>
    {:else if history.length === 0}
      <p class="hint">{$t('portfolios.detail.noOperations')}</p>
    {:else}
      <div class="tscroll">
        <table class="mtable">
          <thead>
            <tr>
              <th class="l">{$t('portfolios.detail.colDate')}</th>
              <th class="l">{$t('portfolios.detail.colSide')}</th>
              <th>{$t('portfolios.detail.colQty')}</th>
              <th>{$t('portfolios.detail.colPrice')}</th>
              <th class="l">{$t('portfolios.detail.colNote')}</th>
            </tr>
          </thead>
          <tbody>
            {#each history as o (o.id)}
              <tr>
                <td class="l date">{fmtDate(o.op_date)}</td>
                <td class="l"><Badge>{$t(`portfolios.kind.${o.side}`)}</Badge></td>
                <td class="num">{corpText(o) ?? fmtNum(o.quantity)}</td>
                <td class="num">{corpText(o) ? '' : fmtMoney(o.price, o.currency ?? asset.currency)}</td>
                <td class="l">{o.note}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {:else if section === 'transfer'}
    <p class="hint">{$t('portfolios.transfer.hint', { qty: fmtNum(asset.quantity) })}</p>
    {#if others && others.length === 0}
      <p class="hint">{$t('portfolios.transfer.noOther')}</p>
    {:else}
      <div class="grid">
        <Field label={$t('portfolios.transfer.to')}>
          <Select
            bind:value={move.to}
            options={[{ value: '', label: $t('portfolios.transfer.pick') }].concat(
              (others ?? []).map((p) => ({ value: p.id, label: p.name }))
            )}
          />
        </Field>
        <Field label={$t('portfolios.detail.quantity')}>
          <input type="number" step="any" min="0" bind:value={move.qty} />
        </Field>
        <Field label={$t('portfolios.detail.note')}>
          <input type="text" bind:value={move.note} />
        </Field>
      </div>
      <div class="actions">
        <button class="btn primary" onclick={doTransfer} disabled={saving || !move.to || !(Number(move.qty) > 0)}>
          {$t('portfolios.transfer.go')}
        </button>
      </div>
    {/if}
  {/if}
</Modal>

<style>
  .secs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    padding-bottom: var(--space-2);
    margin-bottom: var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .secs button {
    height: 28px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .secs button:hover {
    color: var(--text);
  }
  .secs button.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .stack .grid {
    margin-bottom: 0;
  }
  /* A sub-section opens on a hairline, not on a second frame. */
  .sechead {
    padding-top: var(--space-4);
    border-top: var(--hairline) solid var(--border);
  }
  .sub {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .hint {
    margin: var(--space-1) 0 var(--space-3);
    font-size: 12px;
    line-height: 1.5;
    color: var(--dim);
  }
  .sechead .hint {
    margin-bottom: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-3);
  }
  .stack .actions {
    margin-top: 0;
  }
  .plans {
    list-style: none;
    margin: 0 0 var(--space-3);
    padding: 0;
    display: grid;
  }
  .plans li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    min-height: 40px;
    border-bottom: var(--hairline) solid var(--border);
    font-size: 12px;
  }
  .grow {
    flex: 1;
  }
  .dim {
    color: var(--dim);
    font-size: 12px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
    margin-top: var(--space-2);
  }
  .tscroll {
    overflow-x: auto;
  }
  .mtable {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  .mtable th,
  .mtable td {
    height: 36px;
    padding: 0 var(--space-3);
    text-align: right;
    white-space: nowrap;
    border-bottom: var(--hairline) solid var(--border);
  }
  .mtable th {
    height: 32px;
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .mtable tbody tr:hover {
    background: var(--surface-hover);
  }
  .mtable .l {
    text-align: left;
  }
  .num,
  .date {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .date {
    color: var(--muted);
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  textarea {
    width: 100%;
  }
</style>
