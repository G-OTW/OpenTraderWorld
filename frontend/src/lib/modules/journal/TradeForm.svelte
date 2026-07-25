<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Trade entry form, driven by a template's field list. Reserved fields map onto the
  // typed trade columns (used for stats); custom fields collect into `fields`.
  // fmtMoney for the fee preview (a cost, always positive); fmtSignedMoney for the
  // PnL preview, which needs its sign in the text.
  import {
    ASSET_CLASSES,
    CURRENCIES,
    UNIT_TYPES,
    computeFee,
    fmtMoney,
    fmtSignedMoney,
    roundDp
  } from './api.js';
  import { uploadFile, pickFile } from '$lib/modules/editor/files-api.js';
  import Button from '$lib/ui/Button.svelte';
  import { t as tr } from '$lib/i18n';

  let {
    template = null, // selected template (or null → ad-hoc with reserved defaults)
    categories = [],
    strategies = [],
    feeSchedules = [], // saved fee schedules, applied as a shortcut
    initial = null, // existing trade for edit, or null for new
    defaultCategoryId = '',
    suggestions = { tickers: [], exchanges: [], signals: [] }, // autocomplete from past trades
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  // Working trade payload. Reserved columns sit at the top level; custom values in `fields`.
  let t = $state(blank());

  function blank() {
    const base = {
      category_id: defaultCategoryId || categories[0]?.id || '',
      template_id: template?.id ?? null,
      strategy_id: null,
      ticker: '',
      asset_class: 'stock',
      exchange: '',
      side: 'long',
      currency: 'USD',
      unit_type: 'unit',
      // Pre-select the template's default fee schedule (overridable per trade).
      fee_schedule_id: template?.default_fee_schedule_id ?? null,
      entry_at: '',
      exit_at: '',
      entry_price: null,
      exit_price: null,
      quantity: null,
      fees: 0,
      leverage: 1,
      multiplier: 1,
      signal_name: '',
      feedback: '',
      images: [],
      fields: {},
      // Multi-leg (advanced) mode.
      advanced: false,
      cost_basis_method: 'avg',
      entries: [],
      exits: [],
      brackets: []
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        // datetime-local needs 'YYYY-MM-DDTHH:mm'
        entry_at: toLocal(initial.entry_at),
        exit_at: toLocal(initial.exit_at),
        images: initial.images ?? [],
        fields: initial.fields ?? {},
        advanced: initial.advanced ?? false,
        cost_basis_method: initial.cost_basis_method ?? 'avg',
        entries: (initial.entries ?? []).map(legToLocal),
        exits: (initial.exits ?? []).map(legToLocal),
        brackets: (initial.brackets ?? []).map(legToLocal)
      };
    }
    return base;
  }

  // Legs store dates as ISO; the datetime-local inputs need local 'YYYY-MM-DDTHH:mm'.
  function legToLocal(l) {
    return { ...l, at: l.at ? toLocal(l.at) : '' };
  }

  function toLocal(iso) {
    if (!iso) return '';
    const d = new Date(iso);
    const off = d.getTimezoneOffset() * 60000;
    return new Date(d - off).toISOString().slice(0, 16);
  }

  // The reserved field keys this template exposes (so we only render relevant inputs).
  const reservedKeys = $derived(
    template ? new Set(template.fields.filter((f) => f.reserved).map((f) => f.reserved)) : null
  );
  const customFields = $derived(template ? template.fields.filter((f) => !f.reserved) : []);

  // When no template is chosen, show the full reserved set.
  function showReserved(key) {
    return reservedKeys ? reservedKeys.has(key) : true;
  }

  const selectedStrategy = $derived(strategies.find((s) => s.id === t.strategy_id) ?? null);
  const selectedFeeSchedule = $derived(feeSchedules.find((s) => s.id === t.fee_schedule_id) ?? null);

  // ── Fee schedule auto-compute ──
  // Picking a schedule fills the fee from qty × rate (or notional × pct); the user can
  // still type over it. `feeOverridden` tracks a manual edit so we don't clobber it; it
  // resets when a (different) schedule is chosen.
  let feeOverridden = $state(false);

  // Total entry qty and avg entry price the schedule charges against (works in both modes).
  const feeBasisQty = $derived(
    t.advanced ? t.entries.reduce((s, l) => s + (Number(l.qty) || 0), 0) : Number(t.quantity) || 0
  );
  const feeBasisPrice = $derived.by(() => {
    if (t.advanced) {
      const q = feeBasisQty;
      if (q <= 0) return 0;
      return t.entries.reduce((s, l) => s + Number(l.price || 0) * Number(l.qty || 0), 0) / q;
    }
    return Number(t.entry_price) || 0;
  });

  function applyFeeSchedule(id) {
    t.fee_schedule_id = id || null;
    feeOverridden = false;
    recomputeFee();
  }
  function recomputeFee() {
    if (!selectedFeeSchedule || feeOverridden) return;
    // computeFee already rounds (4 dp currency / 6 dp pct); assign the number directly
    // so we never store a stringified value or floating-point noise.
    t.fees = computeFee(selectedFeeSchedule, feeBasisQty, feeBasisPrice);
  }
  // Keep the fee in sync as qty/price change, unless the user took it over.
  $effect(() => {
    // Track deps explicitly so the effect re-runs on qty/price/schedule changes.
    void [t.fee_schedule_id, feeBasisQty, feeBasisPrice];
    recomputeFee();
  });

  const feePreview = $derived(
    selectedFeeSchedule ? computeFee(selectedFeeSchedule, feeBasisQty, feeBasisPrice) : null
  );

  async function addImage() {
    if (t.images.length >= 2) return;
    const file = await pickFile('image/*');
    if (!file) return;
    const up = await uploadFile(file);
    t.images = [...t.images, up.url];
  }
  function removeImage(i) {
    t.images = t.images.filter((_, idx) => idx !== i);
  }

  function num(v) {
    return v === '' || v === null || v === undefined ? null : Number(v);
  }

  // Convert a datetime-local string to ISO. Accepts a bare date ('YYYY-MM-DD', when the
  // user fills only the date and leaves the time blank) and defaults it to 16:00 (4 PM)
  // local time. Returns null for an empty value.
  function toIso(v) {
    if (!v) return null;
    const s = /^\d{4}-\d{2}-\d{2}$/.test(v) ? `${v}T16:00` : v;
    const d = new Date(s);
    return isNaN(d) ? null : d.toISOString();
  }

  // Round prices/qty/fees so stored numbers match what the user typed (no float noise):
  // prices 6 dp, fees 4 dp, quantity 8 dp.
  const price = (v) => (num(v) == null ? null : roundDp(num(v), 6));
  const qtyR = (v) => (num(v) == null ? null : roundDp(num(v), 8));
  const feeR = (v) => (num(v) == null ? null : roundDp(num(v), 4));

  // Split/merge for the date + optional-time pair. The stored value is a single
  // 'YYYY-MM-DDTHH:mm' (datetime-local) string; the date is required to keep a value,
  // the time is optional and defaults to 16:00 (4 PM) on submit via `toIso`.
  const datePart = (v) => (v ? String(v).slice(0, 10) : '');
  const timePart = (v) => (v && String(v).length >= 16 ? String(v).slice(11, 16) : '');
  // Set the date half of `obj[key]`, preserving any time already entered.
  function setDate(obj, key, date) {
    if (!date) {
      obj[key] = '';
      return;
    }
    obj[key] = `${date}T${timePart(obj[key]) || '16:00'}`;
  }
  // Set the time half; no-op until a date exists.
  function setTime(obj, key, time) {
    const d = datePart(obj[key]);
    if (!d) return;
    obj[key] = `${d}T${time || '16:00'}`;
  }

  // ── Leg management (advanced mode) ──
  let legId = 0;
  function newLeg() {
    return { id: `l${Date.now()}_${legId++}`, price: null, qty: null, at: '', fees: 0, signal: '' };
  }
  function newBracket(kind) {
    return { id: `b${Date.now()}_${legId++}`, kind, price: null, qty: null, at: '', triggered: false, note: '' };
  }
  function addEntry() {
    t.entries = [...t.entries, newLeg()];
  }
  function addExit() {
    t.exits = [...t.exits, newLeg()];
  }
  function addBracket(kind) {
    t.brackets = [...t.brackets, newBracket(kind)];
  }
  const removeFrom = (arr, i) => arr.filter((_, idx) => idx !== i);

  // FIFO-matched gross PnL: exits consume entry lots oldest-first (mirrors the server).
  function fifoGross(E, exits, mult, dir) {
    const lots = E.map((l) => ({ price: Number(l.price), qty: Number(l.qty) || 0 }));
    let i = 0;
    let gross = 0;
    for (const x of exits) {
      let need = Number(x.qty) || 0;
      while (need > 1e-12 && i < lots.length) {
        const take = Math.min(need, lots[i].qty);
        gross += (Number(x.price) - lots[i].price) * take * mult * dir;
        lots[i].qty -= take;
        need -= take;
        if (lots[i].qty <= 1e-12) i++;
      }
    }
    return gross;
  }

  // ── Live PnL preview (mirrors the backend cost-basis math, avg or FIFO) ──
  const preview = $derived.by(() => {
    const E = t.advanced ? t.entries : t.entry_price != null ? [{ price: t.entry_price, qty: t.quantity, fees: 0 }] : [];
    const X = t.advanced ? t.exits : t.exit_price != null ? [{ price: t.exit_price, qty: t.quantity, fees: 0 }] : [];
    // Fold triggered brackets into exits for the preview, mirroring the server.
    const entryQty = E.reduce((s, l) => s + (Number(l.qty) || 0), 0);
    let exits = X.map((l) => ({ ...l }));
    if (t.advanced) {
      let used = exits.reduce((s, l) => s + (Number(l.qty) || 0), 0);
      for (const b of t.brackets) {
        if (!b.triggered || b.price == null) continue;
        const remaining = Math.max(entryQty - used, 0);
        const q = b.qty != null ? Math.min(Number(b.qty), remaining) : remaining;
        if (q <= 0) continue;
        used += q;
        // Mirror the server: a triggered bracket's fill is auto-feed by the selected
        // schedule (qty × bracket price), so the previewed Net PnL matches what's stored.
        const fees = selectedFeeSchedule ? computeFee(selectedFeeSchedule, q, Number(b.price)) : 0;
        exits.push({ price: b.price, qty: q, fees });
      }
    }
    if (entryQty <= 0) return null;
    const avg = E.reduce((s, l) => s + Number(l.price) * Number(l.qty), 0) / entryQty;
    const exitQty = exits.reduce((s, l) => s + (Number(l.qty) || 0), 0);
    const dir = t.side === 'short' ? -1 : 1;
    const mult = Number(t.multiplier) || 1;
    if (exits.length === 0) return { avg, openQty: entryQty, gross: null, net: null };
    const gross =
      t.cost_basis_method === 'fifo'
        ? fifoGross(E, exits, mult, dir)
        : exits.reduce((s, l) => s + (Number(l.price) - avg) * Number(l.qty) * mult * dir, 0);
    const legFees =
      E.reduce((s, l) => s + (Number(l.fees) || 0), 0) + exits.reduce((s, l) => s + (Number(l.fees) || 0), 0);
    const net = gross - legFees - (Number(t.fees) || 0);
    return { avg, openQty: Math.max(entryQty - exitQty, 0), gross, net };
  });

  function legPayload(l) {
    return {
      id: l.id,
      price: price(l.price) ?? 0,
      qty: qtyR(l.qty) ?? 0,
      at: toIso(l.at),
      fees: feeR(l.fees) ?? 0,
      signal: l.signal || null
    };
  }
  function bracketPayload(b) {
    return {
      id: b.id,
      kind: b.kind,
      price: price(b.price) ?? 0,
      qty: qtyR(b.qty),
      at: toIso(b.at),
      triggered: !!b.triggered,
      note: b.note || null
    };
  }

  let saving = $state(false);

  async function submit() {
    if (saving) return;
    const payload = {
      category_id: t.category_id,
      template_id: t.template_id,
      strategy_id: t.strategy_id || null,
      ticker: t.ticker,
      asset_class: t.asset_class,
      exchange: t.exchange || null,
      side: t.side,
      currency: t.currency,
      unit_type: t.unit_type,
      fee_schedule_id: t.fee_schedule_id || null,
      entry_at: toIso(t.entry_at),
      exit_at: toIso(t.exit_at),
      entry_price: price(t.entry_price),
      exit_price: price(t.exit_price),
      quantity: qtyR(t.quantity),
      fees: feeR(t.fees) ?? 0,
      leverage: num(t.leverage) ?? 1,
      multiplier: num(t.multiplier) ?? 1,
      signal_name: t.signal_name || null,
      feedback: t.feedback || null,
      images: t.images,
      fields: t.fields,
      advanced: t.advanced,
      cost_basis_method: t.cost_basis_method,
      entries: t.advanced ? t.entries.map(legPayload) : [],
      exits: t.advanced ? t.exits.map(legPayload) : [],
      brackets: t.advanced ? t.brackets.map(bracketPayload) : []
    };
    // `onsubmit` hits the network. Awaiting it (and gating the button on `saving`)
    // is what stops a double-click from logging the trade twice.
    saving = true;
    try {
      await onsubmit(payload);
    } finally {
      saving = false;
    }
  }
</script>

<form
  class="trade-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <!-- Date + optional time. Leaving the time blank defaults to 4 PM on submit. -->
  {#snippet dateTime(obj, key)}
    <span class="dt">
      <input
        type="date"
        value={datePart(obj[key])}
        onchange={(e) => setDate(obj, key, e.target.value)}
      />
      <input
        type="time"
        value={timePart(obj[key])}
        title={$tr('journal.tradeForm.timeOptionalHint')}
        onchange={(e) => setTime(obj, key, e.target.value)}
      />
    </span>
  {/snippet}

  <!-- Autocomplete pools from previously-logged trades. -->
  <datalist id="sg-tickers">
    {#each suggestions.tickers as v}<option value={v}></option>{/each}
  </datalist>
  <datalist id="sg-exchanges">
    {#each suggestions.exchanges as v}<option value={v}></option>{/each}
  </datalist>
  <datalist id="sg-signals">
    {#each suggestions.signals as v}<option value={v}></option>{/each}
  </datalist>

  <div class="row">
    <label class="field">
      <span>{$tr('journal.tradeForm.category')}</span>
      <select bind:value={t.category_id}>
        {#each categories as c}<option value={c.id}>{c.name}</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>{$tr('journal.tradeForm.strategy')}</span>
      <select bind:value={t.strategy_id}>
        <option value={null}>—</option>
        {#each strategies as s}<option value={s.id}>{s.name}</option>{/each}
      </select>
    </label>
  </div>

  {#if showReserved('entry_price')}
    <label class="advanced-toggle">
      <input type="checkbox" bind:checked={t.advanced} />
      <span>{$tr('journal.tradeForm.advancedToggle')}</span>
    </label>
  {/if}

  <div class="grid">
    {#if showReserved('ticker')}
      <label class="field"
        ><span>{$tr('journal.tradeForm.ticker')}</span><input bind:value={t.ticker} list="sg-tickers" autocomplete="off" /></label
      >
    {/if}
    {#if showReserved('asset_class')}
      <label class="field">
        <span>{$tr('journal.tradeForm.assetClass')}</span>
        <select bind:value={t.asset_class}>
          {#each ASSET_CLASSES as a}<option value={a.id}>{a.label}</option>{/each}
        </select>
      </label>
    {/if}
    {#if showReserved('exchange')}
      <label class="field"
        ><span>{$tr('journal.tradeForm.exchange')}</span><input bind:value={t.exchange} list="sg-exchanges" autocomplete="off" /></label
      >
    {/if}
    {#if showReserved('side')}
      <label class="field">
        <span>{$tr('journal.tradeForm.side')}</span>
        <select bind:value={t.side}>
          <option value="long">{$tr('journal.side.long')}</option>
          <option value="short">{$tr('journal.side.short')}</option>
        </select>
      </label>
    {/if}
    {#if showReserved('currency')}
      <label class="field">
        <span>{$tr('journal.tradeForm.currency')}</span>
        <select bind:value={t.currency}>
          {#each CURRENCIES as c}<option value={c.id}>{c.id}</option>{/each}
        </select>
      </label>
    {/if}
    {#if showReserved('unit_type')}
      <label class="field">
        <span>{$tr('journal.tradeForm.unitType')}</span>
        <select bind:value={t.unit_type}>
          {#each UNIT_TYPES as u}<option value={u.id}>{u.label}</option>{/each}
        </select>
      </label>
    {/if}
    {#if showReserved('entry_at') && !t.advanced}
      <label class="field"><span>{$tr('journal.tradeForm.entryTime')}</span>{@render dateTime(t, 'entry_at')}</label>
    {/if}
    {#if showReserved('exit_at') && !t.advanced}
      <label class="field"><span>{$tr('journal.tradeForm.exitTime')}</span>{@render dateTime(t, 'exit_at')}</label>
    {/if}
    {#if showReserved('entry_price') && !t.advanced}
      <label class="field"
        ><span>{$tr('journal.tradeForm.entryPrice')}</span><input type="number" step="any" bind:value={t.entry_price} /></label
      >
    {/if}
    {#if showReserved('exit_price') && !t.advanced}
      <label class="field"
        ><span>{$tr('journal.tradeForm.exitPrice')}</span><input type="number" step="any" bind:value={t.exit_price} /></label
      >
    {/if}
    {#if showReserved('quantity') && !t.advanced}
      <label class="field"
        ><span>{$tr('journal.tradeForm.quantity')}</span><input type="number" step="any" bind:value={t.quantity} /></label
      >
    {/if}
    {#if feeSchedules.length > 0}
      <label class="field">
        <span>{$tr('journal.tradeForm.feeSchedule')}</span>
        <select value={t.fee_schedule_id ?? ''} onchange={(e) => applyFeeSchedule(e.target.value)}>
          <option value="">{$tr('journal.templates.none')}</option>
          {#each feeSchedules as s}<option value={s.id}>{s.name}</option>{/each}
        </select>
      </label>
    {/if}
    {#if showReserved('fees')}
      <label class="field">
        <span>{$tr('journal.tradeForm.fees')}{#if selectedFeeSchedule && !feeOverridden}<span class="auto"> · {$tr('journal.tradeForm.auto')}</span>{/if}</span>
        <input
          type="number"
          step="any"
          bind:value={t.fees}
          oninput={() => (feeOverridden = true)}
        />
        {#if selectedFeeSchedule && feePreview != null}
          <span class="fee-hint">
            {selectedFeeSchedule.name}: {fmtMoney(feePreview, t.currency)}
            {#if feeOverridden}<button type="button" class="relink" onclick={() => { feeOverridden = false; recomputeFee(); }}>{$tr('journal.tradeForm.reset')}</button>{/if}
          </span>
        {/if}
      </label>
    {/if}
    {#if showReserved('leverage')}
      <label class="field"
        ><span>{$tr('journal.tradeForm.leverage')}</span><input type="number" step="any" bind:value={t.leverage} /></label
      >
    {/if}
    {#if showReserved('multiplier')}
      <label class="field"
        ><span>{$tr('journal.tradeForm.multiplier')}</span><input type="number" step="any" bind:value={t.multiplier} /></label
      >
    {/if}
    {#if showReserved('signal_name')}
      <label class="field">
        <span>{$tr('journal.tradeForm.signalName')}</span>
        {#if selectedStrategy?.signals?.length}
          <select bind:value={t.signal_name}>
            <option value="">—</option>
            {#each selectedStrategy.signals as sig}<option value={sig}>{sig}</option>{/each}
          </select>
        {:else}
          <input bind:value={t.signal_name} list="sg-signals" autocomplete="off" />
        {/if}
      </label>
    {/if}

    <!-- Custom template fields -->
    {#each customFields as f (f.key)}
      <label class="field" class:wide={f.type === 'textarea'}>
        <span>{f.label}</span>
        {#if f.type === 'textarea'}
          <textarea bind:value={t.fields[f.key]}></textarea>
        {:else if f.type === 'number'}
          <input type="number" step="any" bind:value={t.fields[f.key]} />
        {:else if f.type === 'checkbox'}
          <input type="checkbox" bind:checked={t.fields[f.key]} />
        {:else if f.type === 'date'}
          <input type="date" bind:value={t.fields[f.key]} />
        {:else if f.type === 'datetime'}
          <input type="datetime-local" bind:value={t.fields[f.key]} />
        {:else if f.type === 'select'}
          <select bind:value={t.fields[f.key]}>
            <option value="">—</option>
            {#each f.options?.choices ?? [] as c}<option value={c}>{c}</option>{/each}
          </select>
        {:else}
          <input bind:value={t.fields[f.key]} />
        {/if}
      </label>
    {/each}
  </div>

  {#if t.advanced}
    <div class="legs">
      <!-- Entries -->
      <section class="leg-group">
        <header>
          <h4>{$tr('journal.tradeForm.entries.title')} <span class="muted">{$tr('journal.tradeForm.entries.subtitle')}</span></h4>
          <button type="button" class="add-leg" onclick={addEntry}>{$tr('journal.tradeForm.entries.add')}</button>
        </header>
        {#if t.entries.length === 0}<p class="muted small">{$tr('journal.tradeForm.entries.empty')}</p>{/if}
        {#each t.entries as leg, i (leg.id)}
          <div class="leg-row">
            <input class="lp" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.price')} bind:value={leg.price} />
            <input class="lq" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.qty')} bind:value={leg.qty} />
            <span class="ld">{@render dateTime(leg, 'at')}</span>
            <input class="lf" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.fees')} bind:value={leg.fees} />
            <input class="ls" placeholder={$tr('journal.tradeForm.leg.signal')} list="sg-signals" bind:value={leg.signal} />
            <button type="button" class="rm-leg" onclick={() => (t.entries = removeFrom(t.entries, i))}><Icon name="x" size={13} /></button>
          </div>
        {/each}
      </section>

      <!-- Exits -->
      <section class="leg-group">
        <header>
          <h4>{$tr('journal.tradeForm.exits.title')} <span class="muted">{$tr('journal.tradeForm.exits.subtitle')}</span></h4>
          <button type="button" class="add-leg" onclick={addExit}>{$tr('journal.tradeForm.exits.add')}</button>
        </header>
        {#each t.exits as leg, i (leg.id)}
          <div class="leg-row" class:synthetic={String(leg.id).startsWith('bracket:')}>
            <input class="lp" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.price')} bind:value={leg.price} />
            <input class="lq" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.qty')} bind:value={leg.qty} />
            <span class="ld">{@render dateTime(leg, 'at')}</span>
            <input class="lf" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.fees')} bind:value={leg.fees} />
            <input class="ls" placeholder={$tr('journal.tradeForm.leg.signal')} list="sg-signals" bind:value={leg.signal} />
            <button type="button" class="rm-leg" onclick={() => (t.exits = removeFrom(t.exits, i))}><Icon name="x" size={13} /></button>
          </div>
        {/each}
      </section>

      <!-- SL / TP brackets -->
      <section class="leg-group">
        <header>
          <h4>{$tr('journal.tradeForm.brackets.title')} <span class="muted">{$tr('journal.tradeForm.brackets.subtitle')}</span></h4>
          <span class="bracket-add">
            <button type="button" class="add-leg" onclick={() => addBracket('sl')}>{$tr('journal.tradeForm.brackets.addSl')}</button>
            <button type="button" class="add-leg" onclick={() => addBracket('tp')}>{$tr('journal.tradeForm.brackets.addTp')}</button>
          </span>
        </header>
        {#each t.brackets as b, i (b.id)}
          <div class="leg-row bracket">
            <select class="bk" bind:value={b.kind}>
              <option value="sl">{$tr('journal.tradeForm.brackets.sl')}</option>
              <option value="tp">{$tr('journal.tradeForm.brackets.tp')}</option>
            </select>
            <input class="lp" type="number" step="any" placeholder={$tr('journal.tradeForm.leg.price')} bind:value={b.price} />
            <input class="lq" type="number" step="any" placeholder={$tr('journal.tradeForm.brackets.qtyPlaceholder')} bind:value={b.qty} />
            <span class="ld">{@render dateTime(b, 'at')}</span>
            <label class="trig" title={$tr('journal.tradeForm.brackets.triggeredHint')}>
              <input type="checkbox" bind:checked={b.triggered} /> {$tr('journal.tradeForm.brackets.hit')}
            </label>
            <button type="button" class="rm-leg" onclick={() => (t.brackets = removeFrom(t.brackets, i))}><Icon name="x" size={13} /></button>
          </div>
        {/each}
      </section>

      <!-- Live PnL preview -->
      {#if preview}
        <div class="pnl-preview">
          <span>{$tr('journal.tradeForm.preview.avgEntry')} <strong class="num">{preview.avg?.toFixed(4) ?? '—'}</strong></span>
          {#if preview.openQty > 0}<span class="open">{$tr('journal.tradeForm.preview.openQty', { qty: preview.openQty })}</span>{/if}
          {#if preview.net != null}
            <!-- Signed + currency-formatted: the +/− is in the text, so the color is a
                 second channel rather than the only one. -->
            <span>
              {$tr('journal.tradeForm.preview.netPnl')}
              <strong class="num {preview.net >= 0 ? 'pos' : 'neg'}">
                {fmtSignedMoney(preview.net, t.currency)}
              </strong>
            </span>
          {:else}
            <span class="muted">{$tr('journal.tradeForm.preview.noExits')}</span>
          {/if}
        </div>
      {/if}

      <label class="field cost-basis">
        <span>{$tr('journal.tradeForm.costBasis.label')}</span>
        <select bind:value={t.cost_basis_method}>
          <option value="avg">{$tr('journal.tradeForm.costBasis.avg')}</option>
          <option value="fifo">{$tr('journal.tradeForm.costBasis.fifo')}</option>
        </select>
      </label>
    </div>
  {/if}

  {#if showReserved('feedback')}
    <label class="field wide"
      ><span>{$tr('journal.tradeForm.feedback')}</span><textarea bind:value={t.feedback}></textarea></label
    >
  {/if}

  {#if showReserved('images')}
    <div class="field wide">
      <span>{$tr('journal.tradeForm.images.label')}</span>
      <div class="images">
        {#each t.images as img, i (img)}
          <div class="thumb">
            <img src={img} alt={$tr('journal.tradeForm.images.alt')} />
            <!-- Icon-only: it needs a name, or a screen reader announces "button". -->
            <button
              type="button"
              class="rm"
              aria-label={$tr('common.close')}
              onclick={() => removeImage(i)}
            >
              <Icon name="x" size={13} />
            </button>
          </div>
        {/each}
        {#if t.images.length < 2}
          <button type="button" class="add-img" onclick={addImage}>{$tr('journal.tradeForm.images.add')}</button>
        {/if}
      </div>
    </div>
  {/if}

  <div class="actions">
    <Button variant="ghost" onclick={oncancel} disabled={saving}>{$tr('common.cancel')}</Button>
    <Button variant="primary" type="submit" loading={saving}>
      {initial ? $tr('journal.tradeForm.saveTrade') : $tr('journal.tradeForm.logTrade')}
    </Button>
  </div>
</form>

<style>
  .trade-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .advanced-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: 0;
  }
  .advanced-toggle input {
    width: 16px;
  }
  .legs {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-3);
    background: var(--surface-2);
  }
  .leg-group header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  /* No uppercase + letter-spacing: it shouts, and these are section labels inside a
     dense form, not banners. */
  .leg-group h4 {
    font-size: var(--text-sm);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .leg-group h4 .muted {
    font-weight: var(--fw-normal);
  }
  .add-leg {
    background: transparent;
    border: 0.5px solid var(--border-control);
    color: var(--text);
    border-radius: 0;
    padding: 3px 10px;
    cursor: pointer;
    font-size: var(--text-xs);
    transition: background-color var(--dur-fast) var(--ease);
  }
  .add-leg:hover {
    background: var(--surface-2);
  }
  .bracket-add {
    display: inline-flex;
    gap: 4px;
  }
  /* Entry/exit rows: Price | Qty | Date(stretch) | Fees | Signal | ✕.
     Bracket rows: Kind | Price | Qty | Date(stretch) | hit | ✕.
     Grid columns let the date flex and the rest shrink (min-width:0) so the row never
     overflows the modal; it collapses to two columns on very narrow widths. */
  .leg-row {
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 0.9fr) minmax(0, 1.6fr) minmax(0, 0.9fr) minmax(0, 1.2fr) auto;
    gap: 6px;
    align-items: center;
    margin-bottom: 5px;
  }
  .leg-row.bracket {
    grid-template-columns: 64px minmax(0, 1fr) minmax(0, 1fr) minmax(0, 1.6fr) auto auto;
  }
  .leg-row input,
  .leg-row select {
    min-width: 0;
    width: 100%;
  }
  @media (max-width: 540px) {
    .leg-row,
    .leg-row.bracket {
      grid-template-columns: 1fr 1fr;
    }
    .leg-row .ld {
      grid-column: 1 / -1;
    }
  }
  .leg-row.synthetic {
    opacity: 0.85;
  }
  .leg-row.synthetic .lp {
    border-color: var(--muted);
  }
  .trig {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .rm-leg {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 2px 4px;
  }
  .rm-leg:hover {
    color: var(--red);
  }
  .pnl-preview {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 0;
    font-size: var(--text-sm);
  }
  .pnl-preview .open {
    color: var(--amber);
  }
  /* Color is the second channel; fmtSignedMoney already put the +/− in the text. */
  .pnl-preview .pos {
    color: var(--green);
  }
  .pnl-preview .neg {
    color: var(--red);
  }
  .cost-basis {
    max-width: 220px;
  }
  .small {
    font-size: var(--text-xs);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .field.wide {
    grid-column: 1 / -1;
  }
  .auto {
    color: var(--dim);
  }
  /* Date + optional-time pair: date takes the room, time stays compact. */
  .dt {
    display: flex;
    gap: 6px;
    min-width: 0;
  }
  .dt input[type='date'] {
    flex: 1 1 auto;
    min-width: 0;
  }
  .dt input[type='time'] {
    flex: 0 0 auto;
    width: 88px;
  }
  /* In the compact leg grid the pair stacks date over time to fit the column. */
  .leg-row .ld .dt {
    flex-wrap: wrap;
  }
  .leg-row .ld .dt input[type='time'] {
    width: 100%;
  }
  .fee-hint {
    font-size: var(--text-xs);
    color: var(--muted);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .relink {
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  input[type='checkbox'] {
    width: 18px;
  }
  .images {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .thumb {
    position: relative;
  }
  .thumb img {
    width: 120px;
    height: 80px;
    object-fit: cover;
    border-radius: 0;
    border: 0.5px solid var(--border);
  }
  /* White on a fixed dark scrim, not a token: this sits over an arbitrary
     screenshot, so it can't follow the theme's text color. */
  .thumb .rm {
    position: absolute;
    top: 4px;
    right: 4px;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    border: none;
    border-radius: 0;
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 2px 5px;
    display: inline-flex;
  }
  .add-img {
    width: 120px;
    height: 80px;
    border: 0.5px dashed var(--border-control);
    border-radius: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    transition:
      color var(--dur-fast) var(--ease),
      background-color var(--dur-fast) var(--ease);
  }
  .add-img:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
