<script>
  // The `syminfo.*` facts of one instrument (tick size, exchange timezone, session, …), stored
  // per provider + asset type + ticker and read by every script on it. An empty field is
  // unknown: a script that reads it stops and names it, nothing is guessed.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import { instrumentMeta } from './api.js';
  import { instrumentErrors } from './host.js';

  let { open = $bindable(false), coords, onsaved } = $props();

  // A future's margin per contract: what a paper account ties up to hold one, or its full
  // value when ticked. Asked only of futures.
  const future = $derived(coords?.asset_type === 'future' || coords?.asset_type === 'futures');
  let margin = $state('');
  let notional = $state(false);

  const NUMS = ['mintick', 'pointvalue'];
  const TEXTS = ['timezone', 'session', 'kind', 'currency', 'basecurrency', 'description'];
  let form = $state({});
  let error = $state('');
  let saving = $state(false);
  // Fields stay locked until the stored row is in: typed before it lands, they would be
  // overwritten by it.
  let loaded = $state(false);
  // A future's empty facts come from its contract specs: said, so a saved value is known to
  // be the reference one until changed.
  let filled = $state([]);
  let source = $state('');
  let specError = $state('');

  $effect(() => {
    if (!open) return;
    form = {};
    error = '';
    loaded = false;
    filled = [];
    source = '';
    specError = '';
    instrumentMeta
      .info(coords)
      .then((r) => {
        const m = r?.meta;
        form = Object.fromEntries([...NUMS, ...TEXTS].map((f) => [f, m?.[f] ?? '']));
        margin = m?.margin ?? '';
        notional = !!m?.margin_notional;
        filled = r?.filled ?? [];
        source = r?.source ?? '';
        specError = r?.spec_error ?? '';
      })
      .catch((e) => (error = e.message))
      .finally(() => (loaded = true));
  });

  /** The row to store: empty fields are null (unknown). */
  const row = $derived({
    provider: coords?.provider ?? '',
    asset_type: coords?.asset_type ?? '',
    ticker: coords?.ticker ?? '',
    ...Object.fromEntries(NUMS.map((f) => [f, String(form[f] ?? '').trim() === '' ? null : Number(form[f])])),
    ...Object.fromEntries(TEXTS.map((f) => [f, String(form[f] ?? '').trim() || null])),
    margin: future && String(margin ?? '').trim() !== '' ? Number(margin) : null,
    margin_notional: future && notional ? true : null
  });
  const errors = $derived(instrumentErrors(row));

  async function save() {
    if (Object.keys(errors).length) return;
    saving = true;
    try {
      await instrumentMeta.put(row);
      open = false;
      onsaved?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal bind:open size="sm" title={$t('histviz.instrument.title')}>
  <div class="form">
    <p class="hint">{coords?.ticker} · {$t('histviz.instrument.hint')}</p>
    {#if coords?.asset_type === 'crypto'}<p class="hint">{$t('histviz.instrument.cryptoNote')}</p>{/if}
    {#if filled.length}
      <p class="hint">
        {$t('histviz.instrument.fromSpecs', { fields: filled.map((f) => $t(`histviz.instrument.${f}`)).join(', '), source })}
      </p>
    {/if}
    {#if specError}<p class="err">{specError}</p>{/if}
    {#if future}
      <label class="field">
        <span>{$t('histviz.instrument.margin')}</span>
        <input type="number" step="any" min="0" disabled={!loaded || notional} bind:value={margin} />
        {#if errors.margin}<small class="err">{$t(`histviz.instrument.${errors.margin}`)}</small>{/if}
      </label>
      <label class="check">
        <input type="checkbox" disabled={!loaded} bind:checked={notional} />
        <span>{$t('histviz.instrument.marginNotional')}</span>
      </label>
      <p class="hint">{$t('histviz.instrument.marginHint')}</p>
    {/if}
    {#each [...NUMS, ...TEXTS] as f (f)}
      <label class="field">
        <span>{$t(`histviz.instrument.${f}`)}</span>
        <input
          type={NUMS.includes(f) ? 'number' : 'text'}
          step="any"
          spellcheck="false"
          placeholder={f === 'timezone' ? 'America/New_York' : f === 'session' ? '0930-1600:23456' : ''}
          disabled={!loaded}
          bind:value={form[f]}
        />
        {#if errors[f]}<small class="err">{$t(`histviz.instrument.${errors[f]}`)}</small>{/if}
      </label>
    {/each}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={!loaded || saving || Object.keys(errors).length > 0} onclick={save}>{$t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .field > span {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .err {
    margin: 0;
    color: var(--red);
    font-size: var(--text-xs);
  }
</style>
