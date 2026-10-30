<script>
  // One stress scenario, created or edited: a set of factor shocks, or a window to replay.
  //
  // The server owns validation and its message is shown as is; the checks here only keep
  // the Save button off while the form is visibly incomplete. A shipped scenario can be
  // reshaped but not removed, so Delete only exists for the user's own.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import DatePicker from '$lib/ui/DatePicker.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { portfoliosApi } from '../api.js';

  // `scenario`: the row to edit, or null for a new one.
  let { scenario = null, onclose, onsaved } = $props();

  const DEFAULT_DURATION = 7.5;

  // A rates leg is quoted in basis points and moves its proxy through a duration; the
  // server also takes it in percent, so the unit is kept per leg rather than per factor.
  function toRow(l) {
    const bps = l.shock_bps != null;
    return {
      factor: l.factor,
      unit: bps ? 'bps' : 'pct',
      shock: bps ? l.shock_bps : (l.shock_pct ?? ''),
      duration: l.duration ?? DEFAULT_DURATION
    };
  }

  // The form edits a copy taken once on open; the editor is remounted per scenario.
  // svelte-ignore state_referenced_locally
  const init = $state.snapshot(scenario);
  let name = $state(init?.name ?? '');
  let kind = $state(init?.kind ?? 'factor');
  let note = $state(init?.note ?? '');
  let legs = $state(init?.kind === 'factor' && Array.isArray(init.legs) ? init.legs.map(toRow) : []);
  let from = $state(init?.kind === 'historical' ? (init.legs?.from ?? '') : '');
  let to = $state(init?.kind === 'historical' ? (init.legs?.to ?? '') : '');

  let factors = $state([]);
  let saving = $state(false);
  let error = $state(null);
  let confirmDelete = $state(false);

  $effect(() => {
    portfoliosApi
      .factors()
      .then((f) => {
        factors = f;
        if (!legs.length && kind === 'factor') addLeg();
      })
      .catch((e) => (error = e.message));
  });

  const inBps = (id) => factors.find((f) => f.id === id)?.in_bps === true;
  const factorOptions = $derived(
    factors.map((f) => ({ value: f.id, label: $t(`portfolios.analytics.stress.factor.${f.id}`) }))
  );
  const kindOptions = $derived(
    ['factor', 'historical'].map((k) => ({ value: k, label: $t(`portfolios.analytics.stress.kind.${k}`) }))
  );
  const unitOptions = $derived([
    { value: 'pct', label: '%' },
    { value: 'bps', label: $t('portfolios.analytics.stress.unitBps') }
  ]);

  function addLeg() {
    const f = factors.find((x) => !legs.some((l) => l.factor === x.id)) ?? factors[0];
    if (!f) return;
    legs.push({ factor: f.id, unit: f.in_bps ? 'bps' : 'pct', shock: '', duration: DEFAULT_DURATION });
  }
  function removeLeg(i) {
    legs.splice(i, 1);
  }
  // A factor without a rates quote has no bps form, so picking one resets the unit.
  function pickFactor(leg, id) {
    leg.factor = id;
    leg.unit = inBps(id) ? 'bps' : 'pct';
  }

  const num = (v) => (v === '' || v == null ? NaN : Number(v));
  const legOk = (l) =>
    Number.isFinite(num(l.shock)) && (l.unit !== 'bps' || (Number.isFinite(num(l.duration)) && num(l.duration) > 0));
  const valid = $derived(
    name.trim() !== '' &&
      (kind === 'factor' ? legs.length > 0 && legs.every(legOk) : !!from && !!to && from < to)
  );

  function body() {
    const out =
      kind === 'factor'
        ? legs.map((l) =>
            l.unit === 'bps'
              ? { factor: l.factor, shock_bps: num(l.shock), duration: num(l.duration) }
              : { factor: l.factor, shock_pct: num(l.shock) }
          )
        : { from, to };
    return { name: name.trim(), kind, legs: out, note: note.trim() };
  }

  async function save() {
    if (!valid || saving) return;
    saving = true;
    error = null;
    try {
      await portfoliosApi.saveScenario(init?.id ?? null, body());
      onsaved?.();
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function remove() {
    saving = true;
    error = null;
    try {
      await portfoliosApi.deleteScenario(init.id);
      onsaved?.();
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal
  open
  title={$t(init ? 'portfolios.analytics.stress.editScenario' : 'portfolios.analytics.stress.newScenario')}
  size="md"
  {onclose}
>
  <div class="row">
    <Field label={$t('portfolios.analytics.stress.name')}>
      <input type="text" bind:value={name} />
    </Field>
    <Select
      label={$t('portfolios.analytics.stress.kindLabel')}
      options={kindOptions}
      bind:value={kind}
      onpick={(k) => k === 'factor' && !legs.length && addLeg()}
      float
    />
  </div>

  {#if kind === 'factor'}
    <div class="legs">
      <span class="cap">{$t('portfolios.analytics.stress.legs')}</span>
      {#if legs.length}
        <div class="leg head">
          <span>{$t('portfolios.analytics.stress.factorCol')}</span>
          <span>{$t('portfolios.analytics.stress.shock')}</span>
          <span></span>
          <span>{$t('portfolios.analytics.stress.duration')}</span>
          <span></span>
        </div>
      {/if}
      {#each legs as leg, i (i)}
        <div class="leg">
          <Dropdown
            value={leg.factor}
            options={factorOptions}
            ariaLabel={$t('portfolios.analytics.stress.factorCol')}
            onpick={(v) => pickFactor(leg, v)}
            float
          />
          <input
            class="numin"
            type="number"
            step="any"
            bind:value={leg.shock}
            aria-label={$t('portfolios.analytics.stress.shock')}
          />
          {#if inBps(leg.factor)}
            <Dropdown
              bind:value={leg.unit}
              options={unitOptions}
              ariaLabel={$t('portfolios.analytics.stress.unit')}
              float
            />
          {:else}
            <span class="unit">%</span>
          {/if}
          {#if leg.unit === 'bps'}
            <input
              class="numin"
              type="number"
              step="any"
              min="0"
              bind:value={leg.duration}
              aria-label={$t('portfolios.analytics.stress.duration')}
            />
          {:else}
            <span></span>
          {/if}
          <button class="mini" onclick={() => removeLeg(i)} aria-label={$t('common.remove')}>
            <Icon name="trash" size={13} />
          </button>
        </div>
      {/each}
      <button class="chip" onclick={addLeg} disabled={!factors.length}>
        <Icon name="plus" size={12} />
        {$t('portfolios.analytics.stress.addLeg')}
      </button>
    </div>
  {:else}
    <div class="row">
      <Field label={$t('portfolios.analytics.stress.from')}>
        <DatePicker bind:value={from} max={to} ariaLabel={$t('portfolios.analytics.stress.from')} />
      </Field>
      <Field label={$t('portfolios.analytics.stress.to')}>
        <DatePicker bind:value={to} min={from} ariaLabel={$t('portfolios.analytics.stress.to')} />
      </Field>
    </div>
  {/if}

  <Field label={$t('portfolios.analytics.stress.note')}>
    <textarea rows="2" bind:value={note}></textarea>
  </Field>

  {#if init?.builtin}
    <p class="muted">{$t('portfolios.analytics.stress.builtinNote')}</p>
  {/if}

  <ErrorText {error} />

  {#snippet footer()}
    {#if init && !init.builtin}
      <span class="left">
        <Button variant="danger" icon="trash" onclick={() => (confirmDelete = true)} disabled={saving}>
          {$t('common.delete')}
        </Button>
      </span>
    {/if}
    <Button variant="ghost" onclick={onclose}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={save} disabled={!valid} loading={saving}>
      {saving ? $t('common.saving') : $t('common.save')}
    </Button>
  {/snippet}
</Modal>

<ConfirmModal
  bind:open={confirmDelete}
  title={$t('portfolios.analytics.stress.deleteTitle')}
  message={$t('portfolios.analytics.stress.deleteConfirm', { name: init?.name ?? '' })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={remove}
/>

<style>
  .row {
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: var(--space-3);
  }
  .legs {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: var(--space-2) 0 var(--space-4);
  }
  .cap {
    font-size: 12px;
    color: var(--muted);
  }
  .leg {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 1fr) 72px minmax(0, 1fr) 28px;
    gap: var(--space-2);
    align-items: center;
  }
  .leg.head span {
    font-size: 11px;
    color: var(--muted);
  }
  .numin {
    width: 100%;
    text-align: right;
  }
  .unit {
    font-size: 12px;
    color: var(--muted);
    padding-left: var(--space-2);
  }
  .mini {
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 2px 4px;
  }
  .mini:hover {
    color: var(--text);
  }
  .chip {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px 9px;
    border: 1px dashed var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 12px;
  }
  .chip:hover:not(:disabled) {
    border-style: solid;
    color: var(--text);
  }
  textarea {
    width: 100%;
    resize: vertical;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  .left {
    margin-right: auto;
  }
  @media (max-width: 767px) {
    .row {
      grid-template-columns: 1fr;
    }
  }
</style>
