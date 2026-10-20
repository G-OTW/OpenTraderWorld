<script>
  // Price alerts for one watchlist symbol.
  //
  // The modal is built around one sentence the user assembles left to right — "notify me
  // when BTC moves ±5% from now" — because that is how the question is actually asked. The
  // controls below the sentence are the qualifiers (window, repeat, destinations) and stay
  // out of the way until they matter: a price threshold hides the window entirely, and the
  // cooldown only appears once "repeat" is on.
  //
  // Alerts are evaluated server-side inside the refresh loop, so they fire with this page
  // closed. That promise is the one thing the footer states outright.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import {
    watchlistsApi,
    ALERT_METRICS,
    ALERT_WINDOWS,
    ALERT_COOLDOWNS,
    directionsFor,
    newAlert,
    alertPayload,
    alertError,
    alertSummary,
    windowLabel,
    fmtQuote,
    trimNum
  } from './api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    item = null,
    alerts = [],
    onchanged = () => {}
  } = $props();

  let draft = $state(newAlert());
  let editing = $state(null); // alert id being edited, null = creating
  let channels = $state([]);
  let busy = $state(false);
  let error = $state('');

  const mine = $derived(alerts.filter((a) => a.item_id === item?.id));
  const price = $derived(item?.quote?.price_usd);
  const isPrice = $derived(draft.metric === 'price');
  const invalid = $derived(alertError(draft));

  // Channels are configured once in the shared broker (Settings → Notifications); here
  // we only pick among the ones granted to Watchlists. An empty selection means "every
  // enabled channel", which is what the hint says.
  const enabledChannels = $derived(channels.filter((c) => c.enabled));

  function loadChannels() {
    channelsApi
      .list('watchlists')
      .then((c) => (channels = c ?? []))
      .catch(() => {});
  }
  $effect(() => {
    if (!open) return;
    loadChannels();
  });

  // Opening for a different symbol resets whatever was half-typed for the previous one.
  $effect(() => {
    item?.id;
    reset();
  });

  function reset() {
    draft = newAlert();
    editing = null;
    error = '';
  }

  function pickMetric(id) {
    draft.metric = id;
    // Directions don't overlap between the two families — keep a valid one.
    const dirs = directionsFor(id).map((d) => d.id);
    if (!dirs.includes(draft.direction)) draft.direction = dirs[dirs.length - 1];
    if (id === 'price') {
      draft.basis = 'anchor';
      draft.window_secs = 0;
      if (price) draft.threshold = Number(trimNum(price));
    } else if (draft.threshold > 100) {
      draft.threshold = 5;
    }
  }

  // A rolling window is the look-back itself, so "no deadline" isn't offered there —
  // switching to it has to land on a real window rather than an empty select.
  function pickBasis(basis) {
    draft.basis = basis;
    if (basis === 'rolling' && !(draft.window_secs > 0)) draft.window_secs = 86400;
  }

  function toggleChannel(id) {
    draft.channel_ids = draft.channel_ids.includes(id)
      ? draft.channel_ids.filter((c) => c !== id)
      : [...draft.channel_ids, id];
  }

  function edit(a) {
    editing = a.id;
    draft = {
      metric: a.metric,
      direction: a.direction,
      threshold: a.threshold,
      basis: a.basis,
      window_secs: a.window_secs,
      repeat: a.repeat,
      cooldown_secs: a.cooldown_secs,
      enabled: a.enabled,
      channel_ids: [...(a.channel_ids ?? [])],
      note: a.note ?? ''
    };
    error = '';
  }

  async function save() {
    if (invalid || busy) return;
    busy = true;
    error = '';
    try {
      const body = alertPayload(draft);
      if (editing) await watchlistsApi.updateAlert(editing, body);
      else await watchlistsApi.addAlert(item.id, body);
      reset();
      onchanged();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function toggleEnabled(a) {
    try {
      await watchlistsApi.updateAlert(a.id, { ...alertPayload(a), enabled: !a.enabled });
      onchanged();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(a) {
    try {
      await watchlistsApi.removeAlert(a.id);
      if (editing === a.id) reset();
      onchanged();
    } catch (e) {
      error = e.message;
    }
  }

  /** Channel names an alert targets, or the "all" wording when it picked none. A pick
   *  the user has since un-granted from Watchlists resolves to nothing — say so rather
   *  than render an empty destination line. */
  function destinations(a) {
    if (!a.channel_ids?.length) return $t('watchlists.alerts.allChannels');
    const names = channels
      .filter((c) => a.channel_ids.includes(c.id))
      .map((c) => c.name || c.kind);
    return names.length ? names.join(', ') : $t('watchlists.alerts.staleChannels');
  }
</script>

<Modal
  bind:open
  size="lg"
  title={$t('watchlists.alerts.title', { symbol: item?.symbol ?? '' })}
  onclose={reset}
>
  <div class="wrap">
    <!-- The sentence. Everything here reads left to right as one clause. -->
    <div class="builder">
      <div class="sentence">
        <span class="lead">{$t('watchlists.alerts.notifyWhen')}</span>

        <div class="seg" role="group" aria-label={$t('watchlists.alerts.metricAria')}>
          {#each ALERT_METRICS as m (m.id)}
            <button
              class="chip"
              class:on={draft.metric === m.id}
              onclick={() => pickMetric(m.id)}
              type="button"
            >
              {$t(m.key)}
            </button>
          {/each}
        </div>

        <div class="inline">
          <Dropdown
            bind:value={draft.direction}
            ariaLabel={$t('watchlists.alerts.dirAria')}
            options={directionsFor(draft.metric).map((d) => ({ value: d.id, label: $t(d.key) }))}
          />
        </div>

        <span class="amount">
          {#if !isPrice && draft.metric === 'usd'}<span class="unit">$</span>{/if}
          <input
            class="num"
            type="number"
            step="any"
            min="0"
            bind:value={draft.threshold}
            aria-label={$t('watchlists.alerts.amountAria')}
          />
          {#if draft.metric === 'pct'}<span class="unit">%</span>{/if}
          {#if isPrice}<span class="unit">USD</span>{/if}
        </span>

        {#if !isPrice}
          <div class="inline">
            <Dropdown
              value={draft.basis}
              onpick={pickBasis}
              ariaLabel={$t('watchlists.alerts.basisAria')}
              options={[
                { value: 'anchor', label: $t('watchlists.alerts.basis.anchor') },
                { value: 'rolling', label: $t('watchlists.alerts.basis.rolling') }
              ]}
            />
          </div>

          <div class="inline">
            <Dropdown
              bind:value={draft.window_secs}
              ariaLabel={$t('watchlists.alerts.windowAria')}
              options={[
                ...(draft.basis === 'anchor'
                  ? [{ value: 0, label: $t('watchlists.alerts.window.none') }]
                  : []),
                ...ALERT_WINDOWS.map((w) => ({ value: w.secs, label: $t(w.key) }))
              ]}
            />
          </div>
        {/if}
      </div>

      <p class="explain">
        {#if isPrice}
          {$t('watchlists.alerts.help.price', {
            price: price != null ? fmtQuote(price) : '—'
          })}
        {:else if draft.basis === 'anchor'}
          {$t('watchlists.alerts.help.anchor', {
            price: price != null ? fmtQuote(price) : '—'
          })}
        {:else}
          {$t('watchlists.alerts.help.rolling', { window: windowLabel(draft.window_secs, $t) })}
        {/if}
      </p>
      {#if invalid}<p class="invalid">{$t(invalid)}</p>{/if}
    </div>

    <!-- Qualifiers -->
    <div class="opts">
      <label class="opt">
        <input type="checkbox" bind:checked={draft.repeat} />
        <span>
          {$t('watchlists.alerts.repeat')}
          <em>{$t('watchlists.alerts.repeatHint')}</em>
        </span>
      </label>

      {#if draft.repeat}
        <label class="opt inline-opt">
          <span class="lbl">{$t('watchlists.alerts.cooldown')}</span>
          <div class="inline">
            <Dropdown
              bind:value={draft.cooldown_secs}
              ariaLabel={$t('watchlists.alerts.cooldown')}
              options={ALERT_COOLDOWNS.map((c) => ({ value: c.secs, label: $t(c.key) }))}
            />
          </div>
        </label>
      {/if}

      <div class="opt block">
        <div class="lblrow">
          <span class="lbl">{$t('watchlists.alerts.sendTo')}</span>
          <ChannelButton module="watchlists" size="sm" onchanged={loadChannels} />
        </div>
        {#if enabledChannels.length === 0}
          <p class="none">{$t('watchlists.alerts.noChannels')}</p>
        {:else}
          <div class="chans">
            {#each enabledChannels as c (c.id)}
              <button
                type="button"
                class="chan"
                class:on={draft.channel_ids.includes(c.id)}
                onclick={() => toggleChannel(c.id)}
              >
                <Icon
                  name={draft.channel_ids.includes(c.id) ? 'check-square' : 'square'}
                  size={12}
                />
                {c.name || c.kind}
                {#if c.last_ok === false}
                  <span class="warn" title={c.last_error ?? ''}>
                    <Icon name="alert-triangle" size={11} />
                  </span>
                {/if}
              </button>
            {/each}
          </div>
          <p class="none">
            {draft.channel_ids.length === 0
              ? $t('watchlists.alerts.allChannelsHint')
              : $t('watchlists.alerts.someChannelsHint')}
          </p>
        {/if}
      </div>

      <input
        class="note"
        type="text"
        maxlength="200"
        placeholder={$t('watchlists.alerts.notePlaceholder')}
        bind:value={draft.note}
      />
    </div>

    <ErrorText error={error} />

    <div class="actions">
      {#if editing}
        <Button size="sm" onclick={reset}>{$t('common.cancel')}</Button>
      {/if}
      <Button variant="primary" size="sm" icon={editing ? 'check' : 'plus'} loading={busy} onclick={save}>
        {editing ? $t('watchlists.alerts.saveEdit') : $t('watchlists.alerts.add')}
      </Button>
    </div>

    <!-- Existing alerts on this symbol -->
    {#if mine.length > 0}
      <div class="list">
        <h4>{$t('watchlists.alerts.existing')}</h4>
        {#each mine as a (a.id)}
          <div class="row" class:off={!a.enabled}>
            <div class="cond">
              <span class="sum">{alertSummary(a, $t)}</span>
              <span class="dest">
                <Icon name="send" size={10} />
                {destinations(a)}
                {#if a.repeat}
                  · {$t('watchlists.alerts.repeatingEvery', {
                    window: windowLabel(a.cooldown_secs, $t)
                  })}
                {/if}
              </span>
              {#if a.note}<span class="dest note-line">{a.note}</span>{/if}
            </div>
            <div class="state">
              {#if a.fire_count > 0}
                <Badge tone="success">{$t('watchlists.alerts.fired', { n: a.fire_count })}</Badge>
              {:else if !a.enabled}
                <Badge tone="neutral">{$t('watchlists.alerts.off')}</Badge>
              {:else}
                <Badge tone="accent">{$t('watchlists.alerts.armed')}</Badge>
              {/if}
            </div>
            <div class="rowacts">
              <button
                class="iconbtn"
                onclick={() => toggleEnabled(a)}
                aria-label={a.enabled ? $t('watchlists.alerts.disable') : $t('watchlists.alerts.enable')}
                title={a.enabled ? $t('watchlists.alerts.disable') : $t('watchlists.alerts.enable')}
              >
                <Icon name={a.enabled ? 'pause' : 'play'} size={12} />
              </button>
              <button
                class="iconbtn"
                onclick={() => edit(a)}
                aria-label={$t('common.edit')}
                title={$t('common.edit')}
              >
                <Icon name="pencil" size={12} />
              </button>
              <button
                class="iconbtn danger"
                onclick={() => remove(a)}
                aria-label={$t('common.delete')}
                title={$t('common.delete')}
              >
                <Icon name="trash" size={12} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <p class="promise"><Icon name="check-circle" size={11} /> {$t('watchlists.alerts.serverSide')}</p>
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  /* ── The sentence ── */
  .builder {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
  }
  .sentence {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .lead {
    color: var(--muted);
  }
  .seg {
    display: inline-flex;
    border: var(--hairline) solid var(--border-control);
  }
  .chip {
    background: transparent;
    border: none;
    padding: 0 var(--space-2);
    height: 26px;
    font-size: 0.78rem;
    color: var(--muted);
    cursor: pointer;
  }
  .chip + .chip {
    border-left: var(--hairline) solid var(--border-control);
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.on {
    background: var(--surface);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .inline {
    min-width: 120px;
  }
  .amount {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .num {
    width: 90px;
    height: 26px;
    padding: 0 var(--space-2);
    font-size: 0.78rem;
    font-family: var(--mono);
    border: var(--hairline) solid var(--border-control);
    background: transparent;
    border-radius: 0;
    color: var(--text);
  }
  .unit {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .explain {
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.4;
  }
  .invalid {
    font-size: var(--text-xs);
    color: var(--red);
  }

  /* ── Qualifiers ── */
  .opts {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .opt {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .opt.block {
    flex-direction: column;
    gap: var(--space-1);
    cursor: default;
  }
  .inline-opt {
    align-items: center;
    padding-left: 23px; /* aligns under the repeat label, past its checkbox */
  }
  .opt em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  /* Label on the left, "Channels" shortcut pushed to the right of the same line. */
  .lblrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .chans {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .chan {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 var(--space-2);
    font-size: 0.78rem;
    border: var(--hairline) solid var(--border-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .chan:hover {
    color: var(--text);
  }
  .chan.on {
    color: var(--text);
    background: var(--surface-2);
  }
  .warn {
    color: var(--amber);
    display: inline-flex;
  }
  .none {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .note {
    height: 26px;
    padding: 0 var(--space-2);
    font-size: 0.78rem;
    border: var(--hairline) solid var(--border-control);
    background: transparent;
    border-radius: 0;
    color: var(--text);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  /* ── Existing alerts ── */
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    border-top: var(--hairline) solid var(--border);
    padding-top: var(--space-3);
  }
  h4 {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: var(--space-1);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row.off {
    opacity: 0.55;
  }
  .cond {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }
  .sum {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .dest {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .note-line {
    font-style: italic;
  }
  .state {
    flex: none;
  }
  .rowacts {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .iconbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .iconbtn:hover {
    color: var(--text);
  }
  .iconbtn.danger:hover {
    color: var(--red);
  }

  .promise {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
