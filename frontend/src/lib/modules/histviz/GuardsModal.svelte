<script>
  // The chart's guards: rules over the chart session's own trading and what each does when it
  // is reached, the session of each account (stopped or not, reset), and when a session ends
  // on its own. Read and fired on the server (`/api/brokers/chart-guards`).
  //
  // Two tabs: the guards (a list, or one guard being written in its place) and the sessions.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import SegPick from './SegPick.svelte';
  import { brokersApi } from '$lib/brokers/api.js';
  import { t } from '$lib/i18n';
  import { ACTIONS, RULE_KINDS, blankDraft, draftOf, guardOf, ruleText } from './guards.js';

  let {
    open = $bindable(false),
    /** The accounts the charts trade on: [{ id, name }]. */
    accounts = [],
    /** The last read (`chartGuards()`), shared with the page. */
    data = null,
    onchanged
  } = $props();

  let tab = $state('guards');
  let error = $state('');
  let busy = $state('');
  /** The guard being written, and the id it replaces (null for a new one). */
  let draft = $state(null);
  let editing = $state(null);

  const browserZone = (() => {
    try {
      return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
    } catch {
      return 'UTC';
    }
  })();
  let zones = ['UTC'];
  try {
    zones = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : ['UTC'];
  } catch {
    zones = ['UTC'];
  }

  let settings = $state({ reset: 'day', timezone: browserZone });
  let seenSettings = '';
  $effect(() => {
    const s = data?.settings;
    if (!s) return;
    const key = `${s.reset}|${s.timezone}|${s.updated_at ?? ''}`;
    if (key === seenSettings) return;
    seenSettings = key;
    // Never saved: the browser's zone, written on Save.
    settings = { reset: s.reset, timezone: s.updated_at ? s.timezone : browserZone };
  });
  const settingsDirty = $derived(!!data?.settings && (settings.reset !== data.settings.reset || settings.timezone !== data.settings.timezone));

  const guards = $derived(data?.guards ?? []);
  const sessions = $derived(data?.sessions ?? []);
  const stoppedCount = $derived(sessions.filter((s) => s.stopped).length);
  const tabs = $derived([
    { id: 'guards', label: `${$t('histviz.guards.rules')}${guards.length ? ` (${guards.length})` : ''}` },
    { id: 'sessions', label: `${$t('histviz.guards.sessions')}${stoppedCount ? ` (${stoppedCount} ${$t('histviz.guards.stopped')})` : ''}` }
  ]);

  const nameOf = (id) => accounts.find((a) => a.id === id)?.name ?? '';
  const scopeOptions = $derived([{ value: '', label: $t('histviz.guards.scopeAll') }, ...accounts.map((a) => ({ value: a.id, label: a.name }))]);
  const kindOptions = $derived(RULE_KINDS.map((k) => ({ value: k, label: $t(`histviz.guards.kind.${k}`) })));
  const zoneOptions = $derived((zones.includes(settings.timezone) ? zones : [settings.timezone, ...zones]).map((z) => ({ value: z, label: z })));

  const ACTION_ICON = { alert: 'bell', acknowledge: 'check-square', stop: 'pause', kill_switch: 'stop' };
  const ACTION_TONE = { alert: 'accent', acknowledge: 'amber', stop: 'amber', kill_switch: 'red' };

  const when = (iso) => new Date(iso).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });

  async function run(tag, fn) {
    busy = tag;
    error = '';
    try {
      await fn();
      await onchanged?.();
      return true;
    } catch (e) {
      error = e.message;
      return false;
    } finally {
      busy = '';
    }
  }

  function add() {
    draft = blankDraft();
    editing = null;
    error = '';
  }
  function edit(g) {
    draft = draftOf(g);
    editing = g.id;
    error = '';
  }
  function back() {
    draft = null;
    editing = null;
    error = '';
  }
  async function save() {
    if (await run('save', () => brokersApi.saveChartGuard(guardOf(draft), editing))) back();
  }
  const remove = (g) => run(`del:${g.id}`, () => brokersApi.deleteChartGuard(g.id));
  const toggle = (g) => run(`on:${g.id}`, () => brokersApi.saveChartGuard({ ...guardOf(draftOf(g)), enabled: !g.enabled }, g.id));
  const reset = (id = null) => run(`reset:${id ?? 'all'}`, () => brokersApi.resetChartSession(id));
  const saveSettings = () => run('settings', () => brokersApi.setChartGuardSettings(settings));

  const needsValue = $derived(draft && ['drawdown', 'trade_loss', 'fees'].includes(draft.kind));
  const draftOk = $derived(
    !!draft && (needsValue ? Number(draft.value) > 0 : Number(draft.n) >= 1) && (draft.kind !== 'trades' || Number(draft.windowValue) > 0)
  );
</script>

<Modal bind:open title={$t('histviz.guards.title')} size="md">
  <div class="gm">
    {#if !draft}
      <Tabs {tabs} bind:value={tab} ariaLabel={$t('histviz.guards.title')} />
    {/if}

    {#if draft}
      <!-- One guard being written, in place of the list. -->
      <div class="ehead">
        <button type="button" class="back" onclick={back}><Icon name="chevron-left" size={14} />{$t('histviz.guards.rules')}</button>
        <span class="etitle">{editing == null ? $t('histviz.guards.add') : $t('histviz.guards.editTitle')}</span>
      </div>

      <div class="form">
        <div class="row">
          <span class="lab">{$t('histviz.guards.scope')}</span>
          <div class="ctl"><Dropdown bind:value={draft.account_id} options={scopeOptions} ariaLabel={$t('histviz.guards.scope')} /></div>
        </div>

        <div class="row">
          <span class="lab">{$t('histviz.guards.rule')}</span>
          <div class="ctl"><Dropdown bind:value={draft.kind} options={kindOptions} ariaLabel={$t('histviz.guards.rule')} /></div>
        </div>

        {#if draft.kind === 'losing_trades'}
          <div class="row">
            <span class="lab">{$t('histviz.guards.count')}</span>
            <div class="ctl inline">
              <input class="num" type="number" min="1" step="1" bind:value={draft.n} aria-label={$t('histviz.guards.count')} />
              <SegPick
                value={draft.consecutive ? 'row' : 'session'}
                options={[
                  { value: 'row', label: $t('histviz.guards.inARow') },
                  { value: 'session', label: $t('histviz.guards.overSession') }
                ]}
                onpick={(v) => (draft.consecutive = v === 'row')}
                ariaLabel={$t('histviz.guards.counting')}
              />
            </div>
          </div>
        {:else if draft.kind === 'trades'}
          <div class="row">
            <span class="lab">{$t('histviz.guards.count')}</span>
            <div class="ctl inline">
              <input class="num" type="number" min="1" step="1" bind:value={draft.n} aria-label={$t('histviz.guards.count')} />
              <span class="muted">{$t('histviz.guards.tradesWithin')}</span>
              <input class="num" type="number" min="1" step="1" bind:value={draft.windowValue} aria-label={$t('histviz.guards.window')} />
              <SegPick
                bind:value={draft.windowUnit}
                options={[
                  { value: 'min', label: $t('histviz.guards.unit.min') },
                  { value: 'h', label: $t('histviz.guards.unit.h') }
                ]}
                ariaLabel={$t('histviz.guards.window')}
              />
            </div>
          </div>
          <div class="row">
            <span class="lab">{$t('histviz.guards.windowKind')}</span>
            <div class="ctl">
              <SegPick
                value={draft.rolling ? 'rolling' : 'fixed'}
                options={[
                  { value: 'rolling', label: $t('histviz.guards.rolling') },
                  { value: 'fixed', label: $t('histviz.guards.fixed') }
                ]}
                onpick={(v) => (draft.rolling = v === 'rolling')}
                ariaLabel={$t('histviz.guards.windowKind')}
              />
            </div>
          </div>
        {:else}
          <div class="row">
            <span class="lab">{$t(`histviz.guards.valueOf.${draft.kind}`)}</span>
            <div class="ctl inline">
              <input class="num wide" type="number" min="0" step="any" bind:value={draft.value} aria-label={$t(`histviz.guards.valueOf.${draft.kind}`)} />
              {#if draft.kind !== 'fees'}
                <SegPick
                  value={draft.pct ? 'pct' : 'money'}
                  options={[
                    { value: 'money', label: $t('histviz.guards.inMoney') },
                    { value: 'pct', label: '%' }
                  ]}
                  onpick={(v) => (draft.pct = v === 'pct')}
                  ariaLabel={$t(`histviz.guards.valueOf.${draft.kind}`)}
                />
              {/if}
            </div>
          </div>
          {#if draft.kind === 'trade_loss' && draft.pct}
            <div class="row">
              <span class="lab">{$t('histviz.guards.ofLabel')}</span>
              <div class="ctl">
                <SegPick
                  bind:value={draft.of}
                  options={[
                    { value: 'notional', label: $t('histviz.guards.of.notional') },
                    { value: 'equity', label: $t('histviz.guards.of.equity') }
                  ]}
                  ariaLabel={$t('histviz.guards.ofLabel')}
                />
              </div>
            </div>
          {/if}
          {#if draft.kind === 'drawdown' || draft.kind === 'fees'}
            <div class="row">
              <span class="lab"></span>
              <p class="note">{$t(draft.kind === 'drawdown' ? 'histviz.guards.drawdownHint' : 'histviz.guards.feesHint')}</p>
            </div>
          {/if}
        {/if}

        <div class="row top">
          <span class="lab">{$t('histviz.guards.then')}</span>
          <div class="ctl">
            <div class="tiles" role="radiogroup" aria-label={$t('histviz.guards.then')}>
              {#each ACTIONS as a (a)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={draft.action === a}
                  class="tile {ACTION_TONE[a]}"
                  class:on={draft.action === a}
                  onclick={() => (draft.action = a)}
                >
                  <span class="tname"><Icon name={ACTION_ICON[a]} size={13} />{$t(`histviz.guards.action.${a}`)}</span>
                  <span class="thint">{$t(`histviz.guards.actionShort.${a}`)}</span>
                </button>
              {/each}
            </div>
            {#if draft.action === 'stop'}
              <label class="check">
                <input type="checkbox" bind:checked={draft.close_positions} />
                <span>{$t('histviz.guards.closePositions')}</span>
              </label>
            {/if}
          </div>
        </div>
      </div>

      <ErrorText {error} />
      <div class="foot">
        <Button variant="ghost" onclick={back}>{$t('common.cancel')}</Button>
        <Button variant="primary" onclick={save} loading={busy === 'save'} disabled={!draftOk}>{$t('common.save')}</Button>
      </div>
    {:else if tab === 'guards'}
      <div class="bar">
        <p class="hint">{$t('histviz.guards.intro')}</p>
        <Button variant="primary" size="sm" icon="plus" onclick={add}>{$t('histviz.guards.add')}</Button>
      </div>
      {#if guards.length}
        <ul class="list">
          {#each guards as g (g.id)}
            <li class:off={!g.enabled}>
              <button
                type="button"
                class="sw"
                role="switch"
                aria-checked={g.enabled}
                aria-label={$t('histviz.guards.enabled')}
                title={$t('histviz.guards.enabled')}
                onclick={() => toggle(g)}
                disabled={busy === `on:${g.id}`}
              ></button>
              <span class="main">
                <span class="name">{ruleText(g.rule, $t)}</span>
                <span class="sub">{g.account_id ? nameOf(g.account_id) : $t('histviz.guards.scopeAll')}</span>
              </span>
              <span class="pill {ACTION_TONE[g.action]}">
                <Icon name={ACTION_ICON[g.action]} size={11} />
                {$t(`histviz.guards.action.${g.action}`)}{g.close_positions ? ` + ${$t('histviz.guards.closesPositions')}` : ''}
              </span>
              <span class="acts">
                <button type="button" class="ibtn" aria-label={$t('common.edit')} title={$t('common.edit')} onclick={() => edit(g)}><Icon name="pencil" size={13} /></button>
                <button type="button" class="ibtn danger" aria-label={$t('common.delete')} title={$t('common.delete')} onclick={() => remove(g)} disabled={busy === `del:${g.id}`}>
                  <Icon name="trash" size={13} />
                </button>
              </span>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="empty">
          <Icon name="shield" size={22} />
          <span>{$t('histviz.guards.none')}</span>
        </div>
      {/if}
      <ErrorText {error} />
    {:else}
      {#if sessions.length}
        <ul class="list">
          {#each sessions as s (s.account_id)}
            <li>
              <span class="dot" class:stopped={!!s.stopped}></span>
              <span class="main">
                <span class="name">{s.name}</span>
                <span class="sub">
                  {s.stopped ? $t('histviz.guards.stoppedSince', { time: when(s.stopped.at) }) : $t('histviz.guards.since', { time: when(s.start) })}
                </span>
              </span>
              <span class="pill {s.stopped ? 'amber' : 'green'}">{s.stopped ? $t('histviz.guards.stopped') : $t('histviz.guards.active')}</span>
              <Button size="sm" variant={s.stopped ? 'primary' : 'secondary'} onclick={() => reset(s.account_id)} loading={busy === `reset:${s.account_id}`}>
                {$t('histviz.guards.reset')}
              </Button>
            </li>
          {/each}
        </ul>
        {#if sessions.length > 1}
          <div class="right">
            <Button size="sm" variant="ghost" icon="refresh-cw" onclick={() => reset()} loading={busy === 'reset:all'}>{$t('histviz.guards.resetAll')}</Button>
          </div>
        {/if}
      {:else}
        <div class="empty">
          <Icon name="plug" size={22} />
          <span>{$t('histviz.guards.noAccount')}</span>
        </div>
      {/if}

      <div class="form boxed">
        <div class="row">
          <span class="lab">{$t('histviz.guards.resetLabel')}</span>
          <div class="ctl">
            <SegPick
              bind:value={settings.reset}
              options={[
                { value: 'day', label: $t('histviz.guards.reset.day') },
                { value: 'manual', label: $t('histviz.guards.reset.manual') }
              ]}
              ariaLabel={$t('histviz.guards.resetLabel')}
            />
          </div>
        </div>
        {#if settings.reset === 'day'}
          <div class="row">
            <span class="lab">{$t('brokers.risk.timezone')}</span>
            <div class="ctl">
              <Dropdown bind:value={settings.timezone} options={zoneOptions} ariaLabel={$t('brokers.risk.timezone')} searchPlaceholder={$t('brokers.risk.timezone')} />
            </div>
          </div>
        {/if}
        {#if settingsDirty}
          <div class="row">
            <span class="lab"></span>
            <div class="ctl"><Button size="sm" variant="primary" onclick={saveSettings} loading={busy === 'settings'}>{$t('common.save')}</Button></div>
          </div>
        {/if}
      </div>
      <ErrorText {error} />
    {/if}
  </div>
</Modal>

<style>
  .gm {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-height: 320px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .bar {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .bar > :global(.btn) {
    flex: none;
  }

  /* One frame for the list, rows split by a hairline. */
  .list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 52px;
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
  }
  .list li + li {
    border-top: 1px solid var(--border);
  }
  .list li:hover {
    background: var(--surface-2);
  }
  .list li.off .main,
  .list li.off .pill {
    opacity: 0.5;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 2px var(--space-2);
    border-radius: 999px;
    font-size: var(--text-xs);
    white-space: nowrap;
    color: var(--muted);
    background: var(--surface-2);
  }
  .pill.accent {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .pill.amber {
    color: var(--amber);
    background: color-mix(in srgb, var(--amber) 12%, transparent);
  }
  .pill.red {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 12%, transparent);
  }
  .pill.green {
    color: var(--green);
    background: color-mix(in srgb, var(--green) 12%, transparent);
  }
  .acts {
    display: inline-flex;
    gap: 2px;
    opacity: 0.35;
    transition: opacity 0.12s;
  }
  .list li:hover .acts,
  .acts:focus-within {
    opacity: 1;
  }
  .ibtn {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .ibtn:hover {
    background: var(--surface);
    color: var(--text);
  }
  .ibtn.danger:hover {
    color: var(--red);
  }
  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--green);
  }
  .dot.stopped {
    background: var(--amber);
  }
  .sw {
    position: relative;
    width: 30px;
    height: 17px;
    flex: none;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    cursor: pointer;
  }
  .sw::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 0.15s;
  }
  .sw[aria-checked='true'] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .sw[aria-checked='true']::after {
    transform: translateX(13px);
    background: var(--surface);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-8) var(--space-4);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .right {
    display: flex;
    justify-content: flex-end;
  }

  /* The editor: a label column and a control column. */
  .ehead {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .back:hover {
    color: var(--text);
  }
  .etitle {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .form.boxed {
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  .row {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr);
    align-items: center;
    gap: var(--space-3);
  }
  .row.top {
    align-items: start;
  }
  .row.top .lab {
    padding-top: var(--space-2);
  }
  .lab {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .ctl {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    max-width: 360px;
  }
  .ctl.inline {
    flex-direction: row;
    align-items: center;
    flex-wrap: wrap;
    max-width: none;
  }
  .num {
    width: 72px;
  }
  .num.wide {
    width: 120px;
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: 1.4;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-2);
    width: 100%;
  }
  .row.top .ctl {
    max-width: none;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    text-align: left;
    cursor: pointer;
  }
  .tile:hover {
    border-color: color-mix(in srgb, var(--text) 25%, var(--border));
  }
  .tname {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .thint {
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.35;
  }
  .tile.on.accent {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .tile.on.amber {
    border-color: var(--amber);
    background: color-mix(in srgb, var(--amber) 8%, transparent);
  }
  .tile.on.red {
    border-color: var(--red);
    background: color-mix(in srgb, var(--red) 8%, transparent);
  }
  .tile.on.accent .tname {
    color: var(--accent);
  }
  .tile.on.amber .tname {
    color: var(--amber);
  }
  .tile.on.red .tname {
    color: var(--red);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  @media (max-width: 640px) {
    .row {
      grid-template-columns: 1fr;
      gap: var(--space-1);
    }
    .tiles {
      grid-template-columns: 1fr;
    }
    .bar {
      flex-direction: column;
    }
  }
</style>
