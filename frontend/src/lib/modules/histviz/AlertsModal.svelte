<script>
  // Alerts on the pane's instrument: the level, the direction, and where the notification
  // goes. The list is the interesting half, because an alert is something you set once and
  // then need to be able to trust: each row says when it last fired, what it last saw, and
  // why it could not run when a connector or a quota got in the way.
  //
  // Two things are stated in the UI because they are decisions, not details:
  //   - a crossing is not a state ("above" waits for the price to come *through* the level,
  //     so an alert placed under the price does not fire the instant it is created);
  //   - the level is judged on **closed bars** of this pane's timeframe.
  import { untrack } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { t } from '$lib/i18n';
  import { histvizApi } from './api.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { channelsApi } from '$lib/notifications/api.js';

  let {
    open = $bindable(false),
    coords = null,
    /** Price the pane suggests: the selected horizontal line, or the last close. */
    suggested = null,
    connectorId = null,
    onchanged
  } = $props();

  let alerts = $state([]);
  let indicators = $state([]);
  let channels = $state([]);
  let error = $state('');
  let busy = $state(false);

  // Draft of the alert being created.
  let kind = $state('price');
  let source = $state('close');
  let indicatorId = $state('');
  let op = $state('above');
  let value = $state(0);
  let name = $state('');
  let repeat = $state(false);
  let cooldown = $state(0);
  let picked = $state([]); // channel ids; empty = every channel the chart is granted

  // The suggestion seeds the level once, when the modal opens. It is a live price, so
  // re-reading it would rewrite the field under the user's fingers every tick. And once the
  // level has been typed it is the user's: closing and reopening the form keeps it, until
  // the alert is created and the draft starts over from the price.
  let seeded = false;
  let typed = false;
  $effect(() => {
    if (!open) {
      seeded = false;
      return;
    }
    if (seeded || !coords) return;
    seeded = true;
    untrack(() => {
      if (!typed) value = Number(suggested ?? 0);
      load();
    });
  });

  async function load() {
    busy = true;
    try {
      alerts = await histvizApi.alerts(coords);
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
    indicatorLib
      .list()
      .then((rows) => (indicators = rows))
      .catch(() => {
        /* the library is optional: price alerts do not need it */
      });
    channelsApi
      .list('histviz')
      .then((rows) => (channels = rows))
      .catch(() => {
        /* no channels granted: the in-app notification still fires */
      });
  }

  async function create() {
    if (!coords) return;
    busy = true;
    try {
      await histvizApi.createAlert({
        name: name.trim(),
        provider: coords.provider,
        asset_type: coords.asset_type,
        ticker: coords.ticker,
        timeframe: coords.timeframe,
        connector_id: connectorId,
        kind,
        source,
        indicator_id: kind === 'indicator' ? indicatorId || null : null,
        op,
        value: Number(value),
        channels: picked,
        repeat,
        cooldown_secs: Number(cooldown) || 0
      });
      name = '';
      error = '';
      typed = false;
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

  const channelIds = (a) => (Array.isArray(a.channels) ? a.channels : []);

  function togglePick(id) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  const OPS = [
    ['above', 'histviz.alerts.opAbove'],
    ['below', 'histviz.alerts.opBelow'],
    ['crosses', 'histviz.alerts.opCrosses']
  ];
  const SOURCES = ['close', 'open', 'high', 'low'];

  const kindOptions = $derived([
    { value: 'price', label: $t('histviz.alerts.kindPrice') },
    { value: 'indicator', label: $t('histviz.alerts.kindIndicator'), disabled: !indicators.length }
  ]);
  const sourceOptions = SOURCES.map((s) => ({ value: s, label: s }));
  const opOptions = $derived(OPS.map(([v, k]) => ({ value: v, label: $t(k) })));
  const indicatorOptions = $derived(indicators.map((i) => ({ value: i.id, label: i.name })));

  const label = (a) => {
    const what =
      a.kind === 'indicator'
        ? (indicators.find((i) => i.id === a.indicator_id)?.name ?? $t('histviz.alerts.indicator'))
        : a.source;
    return `${what} ${$t(OPS.find(([o]) => o === a.op)?.[1] ?? OPS[0][1])} ${a.value}`;
  };

  const when = (iso) => (iso ? new Date(iso).toLocaleString() : '');
  const armed = $derived(alerts.filter((a) => a.enabled).length);
</script>

<Modal bind:open title={$t('histviz.alerts.title')} size="lg">
  <ErrorText {error} />

  <p class="rule">{$t('histviz.alerts.rule')}</p>

  <form
    class="new"
    onsubmit={(e) => {
      e.preventDefault();
      create();
    }}
  >
    <!-- `float` on every picker: the modal body scrolls its own overflow, and a scrolling
         ancestor clips an absolutely positioned menu down to a sliver. -->
    <div class="row">
      <div class="f">
        <span>{$t('histviz.alerts.watch')}</span>
        <Dropdown
          bind:value={kind}
          options={kindOptions}
          ariaLabel={$t('histviz.alerts.watch')}
          float
        />
      </div>
      {#if kind === 'price'}
        <div class="f">
          <span>{$t('histviz.alerts.source')}</span>
          <Dropdown
            bind:value={source}
            options={sourceOptions}
            ariaLabel={$t('histviz.alerts.source')}
            float
          />
        </div>
      {:else}
        <div class="f wide">
          <span>{$t('histviz.alerts.indicator')}</span>
          <Dropdown
            bind:value={indicatorId}
            options={indicatorOptions}
            placeholder={$t('common.select')}
            ariaLabel={$t('histviz.alerts.indicator')}
            float
          />
        </div>
      {/if}
      <div class="f">
        <span>{$t('histviz.alerts.op')}</span>
        <Dropdown bind:value={op} options={opOptions} ariaLabel={$t('histviz.alerts.op')} float />
      </div>
      <label class="f">
        {$t('histviz.alerts.level')}
        <input type="number" step="any" bind:value oninput={() => (typed = true)} />
      </label>
    </div>

    <div class="row">
      <label class="f wide">
        {$t('histviz.alerts.name')}
        <input bind:value={name} placeholder={coords?.ticker ?? ''} />
      </label>
      <label class="chk">
        <input type="checkbox" bind:checked={repeat} />
        {$t('histviz.alerts.repeat')}
      </label>
      <label class="f">
        {$t('histviz.alerts.cooldown')}
        <input type="number" min="0" step="60" bind:value={cooldown} />
      </label>
      <button class="add" type="submit" disabled={busy || (kind === 'indicator' && !indicatorId)}>
        <Icon name="bell" size={13} /> {$t('histviz.alerts.create')}
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

  <h4 class="sec">{$t('histviz.alerts.existing', { armed, total: alerts.length })}</h4>
  <ul class="list">
    {#each alerts as a (a.id)}
      <li class:off={!a.enabled}>
        <span class="dot" class:on={a.enabled}></span>
        <span class="what">
          <strong>{a.name || a.ticker}</strong>
          <span class="cond">{label(a)}</span>
          <span class="tf">{a.timeframe}</span>
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
        <button class="icon" title={$t(a.enabled ? 'histviz.alerts.pause' : 'histviz.alerts.arm')} onclick={() => toggle(a)}>
          <Icon name={a.enabled ? 'pause' : 'play'} size={12} />
        </button>
        <button class="icon danger" title={$t('common.remove')} onclick={() => remove(a)}>
          <Icon name="trash-2" size={12} />
        </button>
      </li>
    {:else}
      <li class="empty">{busy ? $t('common.loading') : $t('histviz.alerts.empty')}</li>
    {/each}
  </ul>
</Modal>

<style>
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
  .cond {
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
  .icon:hover {
    color: var(--text);
    border-color: var(--border);
  }
  .icon.danger:hover {
    color: var(--red);
    border-color: var(--red);
  }
</style>
