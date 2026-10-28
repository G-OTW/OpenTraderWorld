<script>
  // One trading account's risk limits, checked before every order leaves (`brokers::risk` on
  // the server). An empty field is that limit off. Saving asks for the password: the API
  // client raises the prompt and sends again (see $lib/reauth.svelte.js).
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { brokersApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), account = null, onsaved } = $props();

  /** The limits in display order: `int` ones are whole numbers, the rest decimals; `pct` is
   *  a percentage below 100. */
  const FIELDS = [
    { key: 'max_order_value' },
    { key: 'max_order_qty' },
    { key: 'max_position_value' },
    { key: 'max_position_qty' },
    { key: 'collar_pct', pct: true },
    { key: 'max_orders_per_min', int: true },
    { key: 'max_open_orders', int: true },
    { key: 'max_loss' },
    { key: 'duplicate_secs', int: true }
  ];

  let draft = $state({});
  let loading = $state(false);
  let saving = $state(false);
  let error = $state('');
  let tried = $state(false);

  async function load(id) {
    loading = true;
    error = '';
    tried = false;
    try {
      const r = await brokersApi.risk();
      const l = r.limits?.[id] ?? {};
      draft = Object.fromEntries(FIELDS.map((f) => [f.key, l[f.key] == null ? '' : String(l[f.key])]));
    } catch (e) {
      draft = Object.fromEntries(FIELDS.map((f) => [f.key, '']));
      error = e.message;
    } finally {
      loading = false;
    }
  }
  // By id: the host re-reads its account list after a change, and a new object for the same
  // account must not wipe what is being typed.
  const accountId = $derived(account?.id ?? null);
  $effect(() => {
    if (open && accountId) load(accountId);
  });

  /** What is wrong with a field as typed, or '' (empty is off, and fine). */
  function problem(f, raw) {
    const s = String(raw ?? '').trim().replace(',', '.');
    if (s === '') return '';
    const n = Number(s);
    if (!Number.isFinite(n) || !(n > 0)) return f.int ? 'whole' : 'positive';
    if (f.int && !Number.isInteger(n)) return 'whole';
    if (f.pct && !(n < 100)) return 'collar';
    return '';
  }
  const problems = $derived(Object.fromEntries(FIELDS.map((f) => [f.key, problem(f, draft[f.key])])));
  const valid = $derived(FIELDS.every((f) => !problems[f.key]));

  async function save() {
    tried = true;
    if (!valid || !account) return;
    saving = true;
    error = '';
    try {
      // Decimals travel as strings, whole numbers as numbers; null turns a limit off.
      const body = Object.fromEntries(
        FIELDS.map((f) => {
          const s = String(draft[f.key] ?? '').trim().replace(',', '.');
          if (s === '') return [f.key, null];
          return [f.key, f.int ? Number(s) : String(Number(s))];
        })
      );
      await brokersApi.setRiskLimits(account.id, body);
      open = false;
      onsaved?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal bind:open title={$t('brokers.risk.limitsTitle', { name: account?.name ?? '' })} size="md">
  <p class="hint">{$t('brokers.risk.limitsHint')}</p>
  {#if loading}
    <Skeleton height="260px" />
  {:else}
    <form
      class="grid"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      {#each FIELDS as f (f.key)}
        {@const bad = problems[f.key]}
        <label class="fld">
          <span class="lbl">{$t(`brokers.risk.field.${f.key}`)}</span>
          <input
            type="text"
            inputmode={f.int ? 'numeric' : 'decimal'}
            bind:value={draft[f.key]}
            placeholder={$t('brokers.risk.off')}
            aria-invalid={!!bad}
            class:bad={!!bad && (tried || draft[f.key] !== '')}
          />
          {#if bad && (tried || draft[f.key] !== '')}
            <span class="err">{$t(`brokers.risk.err.${bad}`)}</span>
          {:else}
            <span class="help">{$t(`brokers.risk.help.${f.key}`)}</span>
          {/if}
        </label>
      {/each}
      <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
    </form>
  {/if}
  <ErrorText {error} />

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={save} loading={saving} disabled={saving || loading || !valid}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .hint {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--space-3) var(--space-4);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .lbl {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .help,
  .err {
    font-size: 11px;
    line-height: 1.4;
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
  input.bad {
    border-color: var(--red);
  }
</style>
