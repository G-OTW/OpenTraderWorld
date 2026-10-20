<script>
  // The `syminfo.*` facts of one instrument (tick size, exchange timezone, session, …), stored
  // per provider + asset type + ticker and read by every script on it. An empty field is
  // unknown: a script that reads it stops and names it, nothing is guessed.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import { instrumentMeta } from './api.js';
  import { instrumentErrors } from './host.js';

  let { open = $bindable(false), coords, onsaved } = $props();

  const NUMS = ['mintick', 'pointvalue'];
  const TEXTS = ['timezone', 'session', 'kind', 'currency', 'basecurrency', 'description'];
  let form = $state({});
  let error = $state('');
  let saving = $state(false);
  // Fields stay locked until the stored row is in: typed before it lands, they would be
  // overwritten by it.
  let loaded = $state(false);

  $effect(() => {
    if (!open) return;
    form = {};
    error = '';
    loaded = false;
    instrumentMeta
      .get(coords)
      .then((m) => {
        form = Object.fromEntries([...NUMS, ...TEXTS].map((f) => [f, m?.[f] ?? '']));
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
    ...Object.fromEntries(TEXTS.map((f) => [f, String(form[f] ?? '').trim() || null]))
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
