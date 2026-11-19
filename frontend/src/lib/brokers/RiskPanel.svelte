<script>
  // Settings, Brokers: the halt policy, the halts in force and the global kill switch
  // (`/api/brokers/risk`). Per account limits live on the account itself (RiskLimitsModal).
  //
  // Changing the policy and lifting a halt ask for the password: the API client raises the
  // prompt and runs the request again (see $lib/reauth.svelte.js). The kill switch asks
  // nothing, like cancelling: it only takes risk off.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { brokersApi } from './api.js';
  import { t } from '$lib/i18n';

  let { onchanged } = $props();

  const RESETS = ['hour', 'day', 'week', 'manual'];
  /** The policy the server answers when none was ever saved. */
  const DEFAULT_POLICY = { reset: 'day', password_lift: true, timezone: 'UTC' };

  let risk = $state(null);
  let accounts = $state([]);
  let error = $state('');
  let draft = $state({ ...DEFAULT_POLICY });
  let saving = $state(false);
  let saved = $state(false);

  const browserZone = (() => {
    try {
      return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
    } catch {
      return 'UTC';
    }
  })();
  let zones = $state(['UTC']);
  try {
    zones = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : ['UTC'];
  } catch {
    zones = ['UTC'];
  }

  /** The policy as the form starts from it: the stored one, except a time zone never chosen
   *  (the default policy in UTC), which starts at this browser's zone, saved only on Save. */
  function seed(p) {
    const untouched = p.reset === DEFAULT_POLICY.reset && p.password_lift === DEFAULT_POLICY.password_lift && p.timezone === 'UTC';
    return { ...p, timezone: untouched ? browserZone : p.timezone };
  }

  async function load() {
    try {
      const [r, a] = await Promise.all([brokersApi.risk(), brokersApi.list()]);
      risk = r;
      accounts = a;
      draft = seed(r.policy);
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  $effect(() => {
    load();
  });

  async function readRisk() {
    try {
      risk = await brokersApi.risk();
    } catch (e) {
      error = e.message;
    }
  }

  const zoneOptions = $derived(
    (zones.includes(draft.timezone) ? zones : [draft.timezone, ...zones]).map((z) => ({ value: z, label: z }))
  );
  const resetOptions = $derived(RESETS.map((r) => ({ value: r, label: $t(`brokers.risk.reset.${r}`) })));
  const manual = $derived(draft.reset === 'manual');
  const dirty = $derived(
    !!risk &&
      (draft.reset !== risk.policy.reset ||
        (manual ? true : draft.password_lift) !== risk.policy.password_lift ||
        draft.timezone !== risk.policy.timezone)
  );

  async function savePolicy() {
    saving = true;
    saved = false;
    error = '';
    try {
      // A halt that never ends on its own is only ever lifted with the password.
      await brokersApi.setRiskPolicy({ reset: draft.reset, password_lift: manual ? true : draft.password_lift, timezone: draft.timezone });
      await readRisk();
      draft = { ...risk.policy };
      saved = true;
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // ── Halts ──
  const nameOf = (id) => accounts.find((a) => a.id === id)?.name ?? id;
  const when = (iso) => new Date(iso).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  const halts = $derived((risk?.halts ?? []).filter((h) => h.until == null || Date.parse(h.until) > Date.now()));
  let lifting = $state(null); // halt id, or 'all'
  async function lift(id = null) {
    lifting = id ?? 'all';
    error = '';
    try {
      await brokersApi.liftHalt(id);
      await readRisk();
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      lifting = null;
    }
  }

  // ── Kill switch ──
  let killOpen = $state(false);
  let killing = $state(false);
  let killResult = $state(null); // [{ id, name, cancelled?, error? }]
  async function killSwitch() {
    killing = true;
    killResult = null;
    error = '';
    try {
      const r = await brokersApi.killSwitch();
      killResult = Object.entries(r.accounts ?? {}).map(([id, v]) => ({ id, name: v.name ?? nameOf(id), ...v }));
      await readRisk();
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      killing = false;
    }
  }
</script>

<section class="risk">
  <div class="rhead">
    <div class="rtitle">
      <h3>{$t('brokers.risk.title')}</h3>
      <p class="hint">{$t('brokers.risk.hint')}</p>
    </div>
    <Button variant="danger" icon="stop" onclick={() => (killOpen = true)} loading={killing} disabled={killing || !risk}>
      {killing ? $t('brokers.risk.killing') : $t('brokers.risk.kill')}
    </Button>
  </div>

  {#if killResult}
    <div class="killout">
      {#if !killResult.length}
        <p class="line"><Icon name="info" size={12} /> {$t('brokers.risk.killNone')}</p>
      {:else}
        <p class="line ok"><Icon name="check" size={12} /> {$t('brokers.risk.killDone')}</p>
        {#each killResult as r (r.id)}
          <p class="line" class:bad={r.error != null}>
            <Icon name={r.error != null ? 'alert-triangle' : 'check'} size={12} />
            <span class="acct">{r.name}</span>
            <span>{r.error != null ? r.error : $t('brokers.risk.killCancelled', { n: r.cancelled ?? 0 })}</span>
          </p>
        {/each}
      {/if}
    </div>
  {/if}

  {#if risk}
    <div class="block">
      <span class="sname">{$t('brokers.risk.halts')}</span>
      {#if !halts.length}
        <p class="hint">{$t('brokers.risk.noHalts')}</p>
      {:else}
        <ul class="halts">
          {#each halts as h (h.id)}
            <li>
              <Icon name="alert-triangle" size={13} />
              <span class="what">
                <strong>{h.account_id == null ? $t('brokers.risk.scopeAll') : nameOf(h.account_id)}</strong>
                <span>{$t(`brokers.risk.reason.${h.reason}`)}</span>
                {#if h.detail}<span class="muted">{h.detail}</span>{/if}
              </span>
              <span class="muted mono">
                {$t('brokers.risk.since', { time: when(h.created_at) })},
                {h.until ? $t('brokers.risk.until', { time: when(h.until) }) : $t('brokers.risk.untilLifted')}
              </span>
              <Button size="sm" icon="unlock" onclick={() => lift(h.id)} loading={lifting === h.id} disabled={lifting != null}>
                {h.reason === 'position_drift' ? $t('brokers.risk.acknowledge') : $t('brokers.risk.lift')}
              </Button>
            </li>
          {/each}
        </ul>
        {#if halts.length > 1}
          <div class="actions">
            <Button size="sm" variant="ghost" onclick={() => lift()} loading={lifting === 'all'} disabled={lifting != null}>
              {$t('brokers.risk.liftAll')}
            </Button>
          </div>
        {/if}
      {/if}
    </div>

    <div class="block">
      <span class="sname">{$t('brokers.risk.policy')}</span>
      <div class="pgrid">
        <div class="fld">
          <span>{$t('brokers.risk.resetLabel')}</span>
          <Dropdown bind:value={draft.reset} options={resetOptions} ariaLabel={$t('brokers.risk.resetLabel')} />
        </div>
        <div class="fld">
          <span>{$t('brokers.risk.timezone')}</span>
          <Dropdown
            bind:value={draft.timezone}
            options={zoneOptions}
            ariaLabel={$t('brokers.risk.timezone')}
            searchPlaceholder={$t('brokers.risk.timezone')}
          />
        </div>
      </div>
      <label class="check" class:off={manual}>
        <input
          type="checkbox"
          checked={manual || draft.password_lift}
          disabled={manual}
          onchange={(e) => (draft.password_lift = e.currentTarget.checked)}
        />
        <span>{$t('brokers.risk.passwordLift')}</span>
      </label>
      <p class="hint">
        {manual ? $t('brokers.risk.passwordLiftManual') : $t('brokers.risk.passwordLiftHint')}
      </p>
      {#if risk.period}
        <p class="hint">{$t('brokers.risk.period', { start: when(risk.period.start), next: when(risk.period.next) })}</p>
      {/if}
      <div class="actions">
        <Button variant="primary" onclick={savePolicy} loading={saving} disabled={!dirty || saving}>{$t('common.save')}</Button>
        {#if saved && !dirty}<span class="okline"><Icon name="check" size={12} /> {$t('brokers.risk.saved')}</span>{/if}
      </div>
    </div>
  {/if}
  <ErrorText {error} />
</section>

<ConfirmModal
  bind:open={killOpen}
  title={$t('brokers.risk.kill')}
  message={$t('brokers.risk.killMessage')}
  confirmLabel={$t('brokers.risk.killConfirm')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={killSwitch}
/>

<style>
  .risk {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding-top: var(--space-4);
    border-top: 1px solid var(--border);
  }
  .rhead {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .rtitle {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .sname {
    font-size: var(--text-base);
    color: var(--muted);
  }
  .halts {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .halts li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    color: var(--amber);
    font-size: var(--text-sm);
  }
  .what {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    color: var(--text);
  }
  .halts li > :global(.btn) {
    margin-left: auto;
  }
  .muted {
    color: var(--muted);
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .pgrid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--space-3);
    max-width: 560px;
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
  }
  .check.off {
    cursor: default;
    opacity: 0.65;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .okline {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--green);
    font-size: var(--text-sm);
  }
  .killout {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .line.ok {
    color: var(--green);
  }
  .line.bad {
    color: var(--amber);
  }
  .acct {
    font-weight: var(--fw-medium);
    color: var(--text);
  }
</style>
