<script>
  // One signal, four builders. Signal reuses the backtest condition editor (the same operands
  // and ops a strategy and a chart alert use); Event, Pattern and Time are small forms. "Test
  // on item" judges the draft on one item's stored bars and lists the bars where it held, so
  // a signal can be checked before an alert notifies anyone with it.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import SignalGroupEditor from '$lib/modules/backtest/SignalGroupEditor.svelte';
  import ColorSwatches from './ColorSwatches.svelte';
  import { scannerApi, BUILDERS, EVENT_KINDS, PATTERN_KINDS, KIND_FIELDS, defaultConfig, chartUrl } from './api.js';
  import { nextSlot } from './board.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), signal = null, signals = [], items = [], onsaved } = $props();

  let name = $state('');
  let color = $state('');
  let builder = $state('signal');
  let configs = $state({});
  let error = $state('');
  let saving = $state(false);
  let library = $state([]);
  let sessions = $state([]);
  let timezones = $state([]);
  let testItem = $state('');
  let testing = $state(false);
  let result = $state(null);

  const WEEKDAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
  const DIRS = ['up', 'down', 'either'];

  let seeded = false;
  $effect(() => {
    if (!open) {
      seeded = false;
      return;
    }
    if (seeded) return;
    seeded = true;
    name = signal?.name ?? '';
    color = signal?.color ?? nextSlot(signals.map((s) => s.color));
    builder = signal?.builder ?? 'signal';
    configs = { [builder]: signal ? structuredClone(signal.config) : defaultConfig(builder) };
    error = '';
    result = null;
    testItem = testItem || items[0]?.id || '';
    indicatorLib
      .list()
      .then((rows) => (library = rows.map((r) => ({ id: r.id, name: r.name, kind: r.kind ?? 'dag', meta: r.meta?.script ?? null }))))
      .catch(() => {
        /* the library is optional: built-in indicators need none */
      });
    settingsApi
      .marketSessions()
      .then((r) => {
        sessions = r.sessions ?? [];
        timezones = r.timezones ?? [];
      })
      .catch(() => {
        /* no sessions: the time builder still takes a timezone window */
      });
  });

  function pick(b) {
    builder = b;
    if (!configs[b]) configs[b] = defaultConfig(b);
    result = null;
  }

  const cfg = $derived(configs[builder]);

  function setKind(kind) {
    configs[builder] = { kind, ...KIND_FIELDS[kind] };
  }

  function toggleDay(i) {
    const c = configs.time;
    const w = c.weekdays ?? 0;
    const next = w ^ (1 << i);
    c.weekdays = next || null;
  }

  function toggleSession(id) {
    const c = configs.time;
    c.sessions = c.sessions.includes(id) ? c.sessions.filter((s) => s !== id) : [...c.sessions, id];
  }

  /** The config as the API takes it: empty time fields dropped, numbers as numbers. */
  function payload() {
    const c = $state.snapshot(cfg);
    if (builder === 'time') {
      for (const k of ['from', 'to', 'at']) if (!c[k]) delete c[k];
      if (!c.weekdays) delete c.weekdays;
    }
    if (builder === 'event' || builder === 'pattern') {
      for (const k of ['pct', 'bars', 'period', 'factor', 'max_range_pct', 'count']) {
        if (k in c) c[k] = Number(c[k]);
      }
      if (c.session_id === '') delete c.session_id;
      if (c.timezone === '') delete c.timezone;
    }
    return c;
  }

  async function save() {
    saving = true;
    try {
      const body = { name, color, builder, config: payload() };
      if (signal) await scannerApi.saveSignal(signal.id, body);
      else await scannerApi.createSignal(body);
      open = false;
      onsaved?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function test() {
    testing = true;
    try {
      result = await scannerApi.testDraft(testItem, builder, payload());
      error = '';
    } catch (e) {
      result = null;
      error = e.message;
    } finally {
      testing = false;
    }
  }

  const testedItem = $derived(items.find((i) => i.id === testItem));
  const opts = (list, prefix) => list.map((v) => ({ value: v, label: $t(`${prefix}.${v}`) }));
  const fieldNames = $derived(Object.keys(KIND_FIELDS[cfg?.kind] ?? {}).filter((k) => k !== 'direction'));
</script>

<Modal bind:open size="lg" title={signal ? $t('scanner.signals.edit') : $t('scanner.signals.new')}>
  <div class="form">
    <div class="form-grid">
      <Input label={$t('scanner.name')} bind:value={name} required />
      <div class="field">
        <span>{$t('scanner.color')}</span>
        <div class="sw-row"><ColorSwatches bind:value={color} inherit={false} /></div>
      </div>
    </div>

    <div class="builders" role="tablist" aria-label={$t('scanner.signals.builder')}>
      {#each BUILDERS as b (b)}
        <button type="button" role="tab" class="chip" class:active={builder === b} aria-selected={builder === b} onclick={() => pick(b)}>
          {$t(`scanner.builder.${b}`)}
        </button>
      {/each}
    </div>
    <p class="hint">{$t(`scanner.builder.${builder}Hint`)}</p>

    {#if cfg}
      {#if builder === 'signal'}
        <SignalGroupEditor bind:group={configs.signal} title={$t('scanner.signals.conditions')} customIndicators={library} float />
      {:else if builder === 'event' || builder === 'pattern'}
        <div class="form-grid">
          <label class="field">
            <span>{$t('scanner.signals.kind')}</span>
            <Dropdown
              value={cfg.kind}
              float
              onpick={setKind}
              options={opts(builder === 'event' ? EVENT_KINDS : PATTERN_KINDS, `scanner.${builder}`)}
            />
          </label>
          {#if 'direction' in (KIND_FIELDS[cfg.kind] ?? {})}
            <label class="field">
              <span>{$t('scanner.signals.direction')}</span>
              <Dropdown bind:value={configs[builder].direction} float options={opts(DIRS, 'scanner.dir')} />
            </label>
          {/if}
          {#each fieldNames as f (f)}
            <Input type="number" label={$t(`scanner.field.${f}`)} bind:value={configs[builder][f]} />
          {/each}
          {#if cfg.kind === 'move_from_open'}
            <label class="field">
              <span>{$t('scanner.signals.openOf')}</span>
              <Dropdown
                value={cfg.session_id ?? ''}
                float
                onpick={(v) => (configs.event.session_id = v || undefined)}
                options={[{ value: '', label: $t('scanner.signals.dayIn') }, ...sessions.map((s) => ({ value: s.id, label: s.name }))]}
              />
            </label>
            {#if !cfg.session_id}
              <label class="field">
                <span>{$t('scanner.signals.timezone')}</span>
                <Dropdown
                  value={cfg.timezone ?? 'UTC'}
                  float
                  searchable
                  onpick={(v) => (configs.event.timezone = v)}
                  options={(timezones.length ? timezones : ['UTC']).map((z) => ({ value: z, label: z }))}
                />
              </label>
            {/if}
          {/if}
        </div>
        <p class="hint">{$t(`scanner.${builder}.${cfg.kind}Hint`)}</p>
      {:else}
        <div class="form-grid">
          <label class="field">
            <span>{$t('scanner.signals.timezone')}</span>
            <Dropdown
              value={cfg.timezone ?? 'UTC'}
              float
              searchable
              onpick={(v) => (configs.time.timezone = v)}
              options={(timezones.length ? timezones : ['UTC']).map((z) => ({ value: z, label: z }))}
            />
          </label>
          <label class="field">
            <span>{$t('scanner.signals.from')}</span>
            <input type="time" bind:value={configs.time.from} />
          </label>
          <label class="field">
            <span>{$t('scanner.signals.to')}</span>
            <input type="time" bind:value={configs.time.to} />
          </label>
          <label class="field">
            <span>{$t('scanner.signals.at')}</span>
            <input type="time" bind:value={configs.time.at} />
          </label>
        </div>
        <div class="field">
          <span>{$t('scanner.signals.days')}</span>
          <div class="days">
            {#each WEEKDAYS as d, i (d)}
              <button type="button" class="chip" class:active={(cfg.weekdays ?? 0) & (1 << i)} aria-pressed={!!((cfg.weekdays ?? 0) & (1 << i))} onclick={() => toggleDay(i)}>
                {$t(`routines.weekdayShort.${d}`)}
              </button>
            {/each}
          </div>
        </div>
        {#if sessions.length}
          <div class="field">
            <span>{$t('scanner.signals.sessions')}</span>
            <div class="days">
              {#each sessions as s (s.id)}
                <button type="button" class="chip" class:active={cfg.sessions.includes(s.id)} aria-pressed={cfg.sessions.includes(s.id)} onclick={() => toggleSession(s.id)}>
                  {s.name}
                </button>
              {/each}
            </div>
          </div>
        {/if}
        <p class="hint">{$t('scanner.time.hint')}</p>
      {/if}
    {/if}

    {#if error}<ErrorText {error} />{/if}

    <div class="test">
      <span class="lbl">{$t('scanner.signals.testOn')}</span>
      {#if items.length}
        <div class="dd">
          <Dropdown bind:value={testItem} float searchable options={items.map((i) => ({ value: i.id, label: `${i.ticker} ${i.timeframe}` }))} />
        </div>
        <Button size="sm" icon="play" loading={testing} disabled={!testItem} onclick={test}>{$t('scanner.signals.test')}</Button>
      {:else}
        <span class="hint">{$t('scanner.signals.testNoItems')}</span>
      {/if}
    </div>
    {#if result}
      <div class="result">
        <p>{$t('scanner.signals.testResult', { n: result.held_count, bars: result.bars, ticker: result.ticker })}</p>
        {#if result.held.length}
          <ul>
            {#each result.held.slice(-12).reverse() as ts (ts)}
              <li><a href={testedItem ? chartUrl(testedItem) : '/histviz'}>{new Date(ts).toLocaleString()}</a></li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" loading={saving} disabled={!name.trim()} onclick={save}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<style>
  /* Swatches sit on the Name input's center line. */
  .sw-row {
    display: flex;
    align-items: center;
    min-height: var(--control-h);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .builders,
  .days {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .test {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    border-top: var(--hairline) solid var(--border);
    padding-top: var(--space-3);
  }
  .lbl {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .dd {
    min-width: 180px;
  }
  .result {
    font-size: var(--text-sm);
  }
  .result ul {
    list-style: none;
    margin: var(--space-2) 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
</style>
