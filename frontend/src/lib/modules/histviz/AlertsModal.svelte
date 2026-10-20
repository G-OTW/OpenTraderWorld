<script>
  // Chart alerts: one series crossing another or a fixed level, and where the notification
  // goes. The list is the interesting half, because an alert is something you set once and
  // then need to be able to trust: each row says when it last fired, what it last saw, and
  // why it could not run when a connector or a quota got in the way.
  //
  // Two scopes: this chart's instrument (with the form), or every alert (the global list,
  // filtered, with pause-all). Both edit, pause, re-arm and delete in place, and the global
  // list opens an alert's instrument in the active chart.
  //
  // Things stated in the UI because they are decisions, not details:
  //   - a crossing is not a state ("above" waits for the series to come *through* the other
  //     one, so an alert placed under the price does not fire the instant it is created);
  //   - each side is a backtest operand (price, indicator, library indicator or script plot,
  //     metric, fixed value), the same vocabulary and engine as a strategy condition;
  //   - where it is watched: live while the app is open, on closed bars in the background,
  //     or live in the background (a three-notch slider, the live notches only when the
  //     provider streams this instrument at this timeframe);
  //   - how often it may fire, and until when (never expires by default).
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import OperandEditor from '$lib/modules/backtest/OperandEditor.svelte';
  import ScriptInputs from '$lib/script/ScriptInputs.svelte';
  import { t } from '$lib/i18n';
  import { histvizApi } from './api.js';
  import { conditionLabel, isExpired, OP_KEYS } from './alertText.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { channelsApi } from '$lib/notifications/api.js';
  import { settingsApi } from '$lib/settings/api.js';

  let {
    open = $bindable(false),
    /** The pane's instrument; null = only the global list. */
    coords = null,
    /** Price the pane suggests: the selected horizontal line, or the last close. */
    suggested = null,
    connectorId = null,
    /** The provider streams this instrument at this timeframe: the live notches are open. */
    canLive = false,
    onchanged
  } = $props();

  let alerts = $state([]);
  /** Library rows `{ id, name, kind, meta }`, `meta` = a script's compiled ScriptMeta. */
  let library = $state([]);
  let channels = $state([]);
  let sessions = $state([]);
  let error = $state('');
  let busy = $state(false);
  let scope = $state('chart');
  let filter = $state('all');

  // Draft of the alert being created.
  let lhs = $state({ kind: 'price', field: 'close' });
  let rhs = $state({ kind: 'const', value: 0 });
  let inputs = $state({}); // script inputs by indicator id
  let op = $state('above');
  let name = $state('');
  let modeIdx = $state(1); // index into MODES; background by default
  let frequency = $state('once');
  let cooldown = $state(3600);
  let sessionId = $state('');
  let picked = $state([]); // channel ids; empty = every channel the chart is granted
  let forever = $state(true);
  let expiry = $state(''); // datetime-local value
  /** The alert the form edits; null = the form creates one on the pane's instrument. */
  let editing = $state(null);
  let formEl = $state(null);

  // The suggestion seeds the level once, when the modal opens. It is a live price, so
  // re-reading it would rewrite the field under the user's fingers every tick. And once the
  // level has been changed it is the user's: closing and reopening the form keeps it, until
  // the alert is created and the draft starts over from the price.
  let seeded = false;
  let touched = false;
  $effect(() => {
    if (!open) {
      seeded = false;
      return;
    }
    if (seeded) return;
    seeded = true;
    untrack(() => {
      scope = coords ? 'chart' : 'all';
      if (!touched) rhs = { kind: 'const', value: Number(suggested ?? 0) };
      load();
    });
  });

  async function load() {
    busy = true;
    try {
      alerts = await histvizApi.alerts(scope === 'chart' ? coords : null);
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
    indicatorLib
      .list()
      .then((rows) => (library = rows.map((r) => ({ id: r.id, name: r.name, kind: r.kind ?? 'dag', meta: r.meta?.script ?? null }))))
      .catch(() => {
        /* the library is optional: price alerts do not need it */
      });
    settingsApi
      .marketSessions()
      .then((r) => {
        sessions = r.sessions;
        if (!sessionId && sessions.length) sessionId = sessions[0].id;
      })
      .catch(() => {
        /* no session list: every other frequency still works */
      });
    channelsApi
      .list('histviz')
      .then((rows) => (channels = rows))
      .catch(() => {
        /* no channels granted: the in-app notification still fires */
      });
  }

  function setScope(s) {
    if (s === scope || (s === 'chart' && !coords)) return;
    scope = s;
    filter = 'all';
    load();
  }

  /** A date as a `datetime-local` value, in the browser's own time zone. */
  function localInput(d) {
    d.setSeconds(0, 0);
    return new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  }
  /** A month from now, the default when "never" is unticked. */
  const nextMonth = () => localInput(new Date(Date.now() + 30 * 86400000));
  $effect(() => {
    if (!forever && !expiry) expiry = nextMonth();
  });

  /** The scripts either side reads: their inputs are set here, once per script. */
  const scripts = $derived(
    [lhs, rhs]
      .filter((o) => o?.kind === 'custom_indicator')
      .map((o) => library.find((r) => r.id === o.id && r.kind === 'script'))
      .filter((r, i, all) => r && all.findIndex((x) => x?.id === r.id) === i)
  );

  /** Load an alert into the form. Saving it arms it again: an edit is a fresh start. */
  function edit(a) {
    editing = a;
    lhs = structuredClone(a.lhs);
    rhs = structuredClone(a.rhs);
    inputs = structuredClone(a.inputs ?? {});
    op = a.op;
    name = a.name ?? '';
    modeIdx = Math.max(0, MODES.indexOf(a.mode));
    frequency = a.frequency;
    cooldown = a.cooldown_secs || 3600;
    sessionId = a.session_id ?? sessionId;
    picked = channelIds(a);
    forever = !a.expires_at;
    expiry = a.expires_at ? localInput(new Date(a.expires_at)) : '';
    touched = true;
    error = '';
    queueMicrotask(() => formEl?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
  }

  function stopEditing() {
    editing = null;
    lhs = { kind: 'price', field: 'close' };
    rhs = { kind: 'const', value: Number(suggested ?? 0) };
    inputs = {};
    name = '';
    picked = [];
    forever = true;
    expiry = '';
    touched = false;
  }

  /** The alert list's link: the alert's instrument in the active chart. */
  function openChart(a) {
    open = false;
    goto(`/histviz?alert=${a.id}`);
  }

  async function create() {
    const at = editing ?? (coords ? { ...coords, connector_id: connectorId } : null);
    if (!at) return;
    busy = true;
    try {
      const used = Object.fromEntries(scripts.map((r) => [r.id, inputs[r.id] ?? {}]));
      const body = {
        name: name.trim(),
        provider: at.provider,
        asset_type: at.asset_type,
        ticker: at.ticker,
        timeframe: at.timeframe,
        connector_id: at.connector_id ?? null,
        lhs: $state.snapshot(lhs),
        rhs: $state.snapshot(rhs),
        inputs: used,
        op,
        channels: picked,
        mode: MODES[modeIdx],
        frequency,
        cooldown_secs: frequency === 'cooldown' ? Number(cooldown) || 0 : 0,
        session_id: frequency === 'session' ? sessionId || null : null,
        expires_at: forever || !expiry ? null : new Date(expiry).toISOString(),
        enabled: true
      };
      if (editing) await histvizApi.saveAlert(editing.id, body);
      else await histvizApi.createAlert(body);
      error = '';
      stopEditing();
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  /** Re-arm a fired alert, or park a live one. Everything else about it is left alone. */
  async function toggle(a) {
    try {
      await histvizApi.saveAlert(a.id, { ...a, enabled: !a.enabled, channels: channelIds(a) });
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(a) {
    try {
      await histvizApi.deleteAlert(a.id);
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function pauseAll() {
    try {
      await histvizApi.pauseAllAlerts();
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  const channelIds = (a) => (Array.isArray(a.channels) ? a.channels : []);

  function togglePick(id) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  const MODES = ['app', 'background', 'background_live'];
  const MODE_KEYS = {
    app: 'histviz.alerts.modeApp',
    background: 'histviz.alerts.modeBackground',
    background_live: 'histviz.alerts.modeBackgroundLive'
  };
  const FREQ_KEYS = {
    once: 'histviz.alerts.freqOnce',
    every: 'histviz.alerts.freqEvery',
    bar_close: 'histviz.alerts.freqBarClose',
    cooldown: 'histviz.alerts.freqCooldown',
    session: 'histviz.alerts.freqSession'
  };
  const FILTERS = {
    all: 'histviz.alerts.filterAll',
    armed: 'histviz.alerts.filterArmed',
    paused: 'histviz.alerts.filterPaused',
    expired: 'histviz.alerts.filterExpired',
    error: 'histviz.alerts.filterError'
  };

  // A provider that does not stream this leaves only the background notch. An alert on
  // another instrument is edited with every notch open: the server refuses a live mode its
  // provider cannot stream, with the reason.
  const liveOk = $derived(canLive || (!!editing && !sameInstrument(editing)));
  function sameInstrument(a) {
    return !!coords && a.provider === coords.provider && a.asset_type === coords.asset_type && a.ticker === coords.ticker && a.timeframe === coords.timeframe;
  }
  $effect(() => {
    if (!liveOk && modeIdx !== 1) modeIdx = 1;
  });
  const mode = $derived(MODES[modeIdx]);

  const opOptions = $derived(Object.entries(OP_KEYS).map(([v, k]) => ({ value: v, label: $t(k) })));
  const freqOptions = $derived(
    Object.entries(FREQ_KEYS).map(([v, k]) => ({
      value: v,
      label: $t(k),
      disabled: v === 'session' && !sessions.length
    }))
  );
  const sessionOptions = $derived(sessions.map((s) => ({ value: s.id, label: s.name })));

  // A script side needs a plot it may read; a side left without one cannot be created.
  const incomplete = $derived(
    [lhs, rhs].some((o) => o?.kind === 'custom_indicator' && !o.id) ||
      scripts.some((r) => [lhs, rhs].some((o) => o?.id === r.id && !o.plot))
  );

  const label = (a) => conditionLabel(a, library, $t);
  const when = (iso) => (iso ? new Date(iso).toLocaleString() : '');
  const armed = $derived(alerts.filter((a) => a.enabled).length);
  const matches = (a) =>
    filter === 'armed'
      ? a.enabled && !isExpired(a)
      : filter === 'paused'
        ? !a.enabled && !isExpired(a)
        : filter === 'expired'
          ? isExpired(a)
          : filter === 'error'
            ? !!a.last_error
            : true;
  const shown = $derived(alerts.filter(matches));
  // The global list reads by instrument: one heading per ticker and timeframe.
  const groups = $derived.by(() => {
    const out = new Map();
    for (const a of shown) {
      const key = `${a.provider}|${a.asset_type}|${a.ticker}|${a.timeframe}`;
      if (!out.has(key)) out.set(key, { key, title: `${a.ticker} · ${a.timeframe}`, sub: a.provider, rows: [] });
      out.get(key).rows.push(a);
    }
    return [...out.values()];
  });
</script>

<Modal bind:open title={$t('histviz.alerts.title')} size="lg">
  <ErrorText {error} />

  <div class="scopes" role="tablist">
    <button type="button" role="tab" class:on={scope === 'chart'} aria-selected={scope === 'chart'} disabled={!coords} onclick={() => setScope('chart')}>
      {coords ? `${coords.ticker} · ${coords.timeframe}` : $t('histviz.alerts.scopeChart')}
    </button>
    <button type="button" role="tab" class:on={scope === 'all'} aria-selected={scope === 'all'} onclick={() => setScope('all')}>
      {$t('histviz.alerts.scopeAll')}
    </button>
  </div>

  {#if editing || (scope === 'chart' && coords)}
    <p class="rule">{$t('histviz.alerts.rule')}</p>
    {#if editing}
      <p class="editing">
        <Icon name="pencil" size={12} />
        {$t('histviz.alerts.editing', { name: editing.name || editing.ticker, ticker: `${editing.ticker} · ${editing.timeframe}` })}
        <button type="button" class="chip" onclick={stopEditing}>{$t('common.cancel')}</button>
      </p>
    {/if}

    <form
      bind:this={formEl}
      class="new"
      onsubmit={(e) => {
        e.preventDefault();
        create();
      }}
    >
      <!-- `float` on every picker: the modal body scrolls its own overflow, and a scrolling
           ancestor clips an absolutely positioned menu down to a sliver. -->
      <div class="cond">
        <span class="lbl">{$t('histviz.alerts.condition')}</span>
        <div class="side">
          <OperandEditor bind:operand={lhs} customIndicators={library} float />
        </div>
        <div class="side">
          <Dropdown bind:value={op} options={opOptions} ariaLabel={$t('histviz.alerts.op')} float />
        </div>
        <div class="side" oninput={() => (touched = true)}>
          <OperandEditor bind:operand={rhs} customIndicators={library} float />
        </div>
      </div>

      {#each scripts as row (row.id)}
        <details class="inputs" open={scripts.length === 1}>
          <summary>{$t('histviz.alerts.scriptInputs', { name: row.name })}</summary>
          <ScriptInputs
            inputs={row.meta?.inputs ?? []}
            bind:values={() => inputs[row.id] ?? {}, (v) => (inputs = { ...inputs, [row.id]: v })}
          />
        </details>
      {/each}
      {#if scripts.length}
        <p class="hint">{$t('histviz.alerts.scriptAtClose')}</p>
      {/if}

      <div class="modes">
        <span class="lbl">{$t('histviz.alerts.mode')}</span>
        <input
          type="range"
          min="0"
          max="2"
          step="1"
          bind:value={modeIdx}
          disabled={!liveOk}
          aria-label={$t('histviz.alerts.mode')}
          aria-valuetext={$t(MODE_KEYS[mode])}
        />
        <div class="notches">
          {#each MODES as m, i (m)}
            <button
              type="button"
              class="notch"
              class:on={modeIdx === i}
              disabled={!liveOk && i !== 1}
              onclick={() => (modeIdx = i)}
            >
              {$t(MODE_KEYS[m])}
            </button>
          {/each}
        </div>
        <p class="hint">
          {liveOk ? $t(`${MODE_KEYS[mode]}Hint`) : $t('histviz.alerts.noLive')}
        </p>
      </div>

      <div class="row">
        <div class="f">
          <span>{$t('histviz.alerts.frequency')}</span>
          <Dropdown
            bind:value={frequency}
            options={freqOptions}
            ariaLabel={$t('histviz.alerts.frequency')}
            float
          />
        </div>
        {#if frequency === 'cooldown'}
          <label class="f">
            {$t('histviz.alerts.cooldown')}
            <input type="number" min="1" step="60" bind:value={cooldown} />
          </label>
        {:else if frequency === 'session'}
          <div class="f">
            <span>
              {$t('histviz.alerts.session')}
              <a class="edit" href="/settings#sessions">{$t('histviz.alerts.sessionsLink')}</a>
            </span>
            <Dropdown
              bind:value={sessionId}
              options={sessionOptions}
              ariaLabel={$t('histviz.alerts.session')}
              float
            />
          </div>
        {/if}
        <div class="f">
          <span>{$t('histviz.alerts.expiry')}</span>
          <span class="expiry">
            <label class="chk">
              <input type="checkbox" bind:checked={forever} />
              {$t('histviz.alerts.expiryNever')}
            </label>
            {#if !forever}
              <input type="datetime-local" bind:value={expiry} aria-label={$t('histviz.alerts.expiry')} />
            {/if}
          </span>
        </div>
        <label class="f wide">
          {$t('histviz.alerts.name')}
          <input bind:value={name} placeholder={editing?.ticker ?? coords?.ticker ?? ''} />
        </label>
        <button
          class="add"
          type="submit"
          disabled={busy ||
            incomplete ||
            (!forever && !expiry) ||
            (frequency === 'session' && !sessionId) ||
            (frequency === 'cooldown' && !(Number(cooldown) > 0))}
        >
          <Icon name={editing ? 'check' : 'bell'} size={13} /> {$t(editing ? 'histviz.alerts.save' : 'histviz.alerts.create')}
        </button>
      </div>

      {#if channels.length}
        <div class="chans">
          <span class="lbl">{$t('histviz.alerts.channels')}</span>
          {#each channels as c (c.id)}
            <label class="chk">
              <input
                type="checkbox"
                checked={picked.includes(c.id)}
                onchange={() => togglePick(c.id)}
              />
              {c.name}
            </label>
          {/each}
          <span class="hint">{$t('histviz.alerts.channelsHint')}</span>
        </div>
      {:else}
        <p class="hint">
          {$t('histviz.alerts.noChannels')}
          <a href="/settings">{$t('histviz.alerts.noChannelsLink')}</a>
        </p>
      {/if}
    </form>
  {/if}

  <div class="head">
    <h4 class="sec">{$t('histviz.alerts.existing', { armed, total: alerts.length })}</h4>
    <div class="filters">
      {#each Object.entries(FILTERS) as [f, k] (f)}
        <button type="button" class="chip" class:on={filter === f} onclick={() => (filter = f)}>{$t(k)}</button>
      {/each}
      {#if scope === 'all' && armed > 0}
        <button type="button" class="chip" onclick={pauseAll}>
          <Icon name="pause" size={11} /> {$t('histviz.alerts.pauseAll')}
        </button>
      {/if}
    </div>
  </div>

  {#snippet item(a)}
    {@const expired = isExpired(a)}
    <li class:off={!a.enabled || expired}>
      <span class="dot" class:on={a.enabled && !expired}></span>
      <span class="what">
        <strong>{a.name || a.ticker}</strong>
        <span class="cond-text">{label(a)}</span>
        {#if scope === 'chart'}<span class="tf">{a.timeframe}</span>{/if}
        <span class="badge" class:live={a.mode !== 'background'}>{$t(MODE_KEYS[a.mode] ?? MODE_KEYS.background)}</span>
        <span class="tf">{$t(FREQ_KEYS[a.frequency] ?? FREQ_KEYS.once)}</span>
        {#if expired}
          <span class="badge">{$t('histviz.alerts.expired')}</span>
        {:else if a.expires_at}
          <span class="tf">{$t('histviz.alerts.expiresAt', { when: when(a.expires_at) })}</span>
        {/if}
      </span>
      <span class="state">
        {#if a.last_error}
          <span class="warn" title={a.last_error}>
            <Icon name="alert-triangle" size={11} /> {$t('histviz.alerts.cannotRun')}
          </span>
        {:else if a.last_fired_at}
          {$t('histviz.alerts.firedAt', { when: when(a.last_fired_at) })}
        {:else if a.last_value != null}
          {$t('histviz.alerts.watching', { value: a.last_value })}
        {:else}
          {$t('histviz.alerts.pending')}
        {/if}
      </span>
      {#if scope === 'all'}
        <button class="icon" title={$t('histviz.alerts.openChart')} onclick={() => openChart(a)}>
          <Icon name="candlestick" size={12} />
        </button>
      {/if}
      <button class="icon" class:on={editing?.id === a.id} title={$t('histviz.alerts.edit')} onclick={() => edit(a)}>
        <Icon name="pencil" size={12} />
      </button>
      <button
        class="icon"
        disabled={expired && !a.enabled}
        title={expired && !a.enabled
          ? $t('histviz.alerts.expiredHint')
          : $t(a.enabled ? 'histviz.alerts.pause' : 'histviz.alerts.arm')}
        onclick={() => toggle(a)}
      >
        <Icon name={a.enabled ? 'pause' : 'play'} size={12} />
      </button>
      <button class="icon danger" title={$t('common.remove')} onclick={() => remove(a)}>
        <Icon name="trash-2" size={12} />
      </button>
    </li>
  {/snippet}

  {#if scope === 'all'}
    {#each groups as g (g.key)}
      <h5 class="group">{g.title} <span class="tf">{g.sub}</span></h5>
      <ul class="list">
        {#each g.rows as a (a.id)}{@render item(a)}{/each}
      </ul>
    {:else}
      <p class="empty">{busy ? $t('common.loading') : $t('histviz.alerts.emptyAll')}</p>
    {/each}
  {:else}
    <ul class="list">
      {#each shown as a (a.id)}
        {@render item(a)}
      {:else}
        <li class="empty">{busy ? $t('common.loading') : $t('histviz.alerts.empty')}</li>
      {/each}
    </ul>
  {/if}
</Modal>

<style>
  .scopes {
    display: flex;
    gap: var(--space-1);
    margin-bottom: var(--space-2);
    border-bottom: 1px solid var(--border);
  }
  .scopes button {
    background: none;
    border: 0;
    border-bottom: 2px solid transparent;
    padding: var(--space-1) var(--space-2);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.78rem;
  }
  .scopes button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .scopes button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .cond {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: var(--space-1) var(--space-2);
    align-items: center;
  }
  .cond .lbl {
    grid-row: span 3;
    align-self: start;
    padding-top: var(--space-1);
    font-size: 0.72rem;
    color: var(--muted);
  }
  .side {
    display: flex;
    min-width: 0;
  }
  .inputs summary {
    cursor: pointer;
    font-size: 0.74rem;
    color: var(--muted);
  }
  .expiry {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px var(--space-2);
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 0.7rem;
    cursor: pointer;
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .group {
    margin: var(--space-3) 0 0;
    font-size: 0.78rem;
  }
  .empty {
    color: var(--muted);
    font-size: 0.76rem;
  }
  .editing {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-2);
    font-size: 0.76rem;
  }
  .icon.on {
    color: var(--accent);
    border-color: var(--accent);
  }
  .rule {
    margin: 0 0 var(--space-2);
    font-size: 0.74rem;
    color: var(--muted);
  }
  .new {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: flex-end;
  }
  .f {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 0.72rem;
    color: var(--muted);
    min-width: 90px;
  }
  .f.wide {
    flex: 1;
    min-width: 160px;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.74rem;
    color: var(--muted);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
  }
  .add:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .add:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .modes {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .modes input[type='range'] {
    width: 100%;
    accent-color: var(--accent);
  }
  .notches {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
  }
  .notch {
    background: none;
    border: 0;
    padding: 0;
    font-size: 0.72rem;
    color: var(--muted);
    cursor: pointer;
  }
  .notch:nth-child(1) {
    text-align: left;
  }
  .notch:nth-child(3) {
    text-align: right;
  }
  .notch.on {
    color: var(--text);
    font-weight: 600;
  }
  .notch:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .modes .lbl {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .modes .hint {
    margin: 0;
  }
  .edit {
    margin-left: var(--space-1);
    font-size: 0.68rem;
  }
  .badge {
    font-size: 0.66rem;
    padding: 0 var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .badge.live {
    color: var(--accent);
    border-color: var(--accent);
  }
  .chans {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .chans .lbl,
  .hint {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .sec {
    margin: var(--space-3) 0 var(--space-1);
    font-size: 0.8rem;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--border);
    font-size: 0.76rem;
  }
  .list li.off {
    opacity: 0.6;
  }
  .list li.empty {
    color: var(--muted);
    border: 0;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.on {
    background: var(--green);
  }
  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .cond-text {
    color: var(--muted);
  }
  .tf {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .state {
    color: var(--muted);
    font-size: 0.7rem;
    white-space: nowrap;
  }
  .warn {
    color: var(--amber);
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .icon {
    display: inline-flex;
    padding: 3px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border);
  }
  .icon:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .icon.danger:hover {
    color: var(--red);
    border-color: var(--red);
  }
</style>
