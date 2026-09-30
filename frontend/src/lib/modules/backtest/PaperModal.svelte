<script>
  // Paper trading setup: the whole session on one screen, from the strategy it froze to the
  // wording of the message it will send.
  //
  // The strategy and the instruments are shown, not edited: a session is started from a
  // result the user is looking at, and editing them here would silently change what is being
  // traded forward. Everything else (when, how loud, what the message says) is the point of
  // this form.
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import SchedulePicker from '$lib/modules/automator/SchedulePicker.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { paperApi, NOTIFY_WHEN, CADENCES, FEEDS, MESSAGE_KINDS } from './paper.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    session = $bindable(null),
    tickers = [],
    onsaved = () => {},
    // Hand the frozen strategy back to the editor. Saving here changes the session's own
    // settings (schedule, reporting, wording); the strategy is edited where it is built.
    oneditstrategy = null
  } = $props();

  // The instruments the session actually trades: the filter offers those, never free text.
  const tickerOptions = $derived(tickers.map((tk) => ({ value: tk, label: tk })));
  // The recap message only exists when the session sends one: fills go out on every tick.
  const grouping = $derived(session?.cadence !== 'each');

  let saving = $state(false);
  let error = $state('');
  let channels = $state([]);
  let vars = $state([]);
  let filters = $state([]);
  // Which of the three messages is being written. The preview and the helper list follow
  // it, so what is shown is always the vocabulary of the message on screen.
  let msgKind = $state('exit');
  let preview = $state({ title: '', message: '' });
  let helperOpen = $state(false);
  /** Which template the helper list inserts into, and the DOM node holding the caret. The
   *  node comes from the focus event: the shared Input renders it, so there is no ref to bind. */
  let focused = $state('body');
  let focusedEl = $state(null);

  const editing = $derived(!!session?.id);
  // The summary only exists when a cadence asks for one.
  const kinds = $derived(MESSAGE_KINDS.filter((k) => k.id !== 'summary' || grouping));
  const current = $derived(kinds.find((k) => k.id === msgKind) ?? MESSAGE_KINDS[1]);
  $effect(() => {
    if (!kinds.some((k) => k.id === msgKind)) msgKind = 'exit';
  });

  // Each section collapses to its own answer: the header carries the value, so the form
  // reads as a list of decisions before it is opened as a list of controls.
  const feedLabel = $derived($t(`backtest.paper.feed.${session?.feed ?? 'hist'}`));
  const whenLabel = $derived(
    session?.kind
      ? `${$t(`automator.sched.kind.${session.kind}`)} · ${session.timezone ?? ''}`.trim()
      : ''
  );
  const windowLabel = $derived(
    $t('backtest.paper.windowSummary', { count: session?.window_bars ?? 0 })
  );
  const reportingLabel = $derived.by(() => {
    if (!session) return '';
    const when = $t(`backtest.paper.notify.${session.notify_when}`);
    const n = session.channel_ids?.length ?? 0;
    const where =
      n === 0
        ? $t('backtest.paper.allChannels')
        : $t('backtest.paper.channelCount', { count: n });
    return session.notify_when === 'never' ? when : `${when} · ${where}`;
  });
  const messageLabel = $derived(
    session && MESSAGE_KINDS.some((k) => session[k.title] || session[k.body])
      ? $t('backtest.paper.customWording')
      : $t('backtest.paper.defaultWording')
  );

  const notifyOptions = $derived(
    NOTIFY_WHEN.map((v) => ({ value: v, label: $t(`backtest.paper.notify.${v}`) }))
  );
  const cadenceOptions = $derived(
    CADENCES.map((v) => ({ value: v, label: $t(`backtest.paper.cadence.${v}`) }))
  );
  const channelOptions = $derived(
    channels.map((c) => ({ value: c.id, label: `${c.name} (${c.kind})` }))
  );
  // A recap only means something for a session that speaks at all.
  const digestUseful = $derived(session?.notify_when !== 'never');

  $effect(() => {
    if (!open) return;
    loadChannels();
  });

  // The vocabulary is per message kind, so it is re-read when the tab changes.
  $effect(() => {
    if (!open) return;
    loadVars(current.event);
  });

  async function loadChannels() {
    try {
      channels = await channelsApi.list('backtest');
    } catch {
      channels = [];
    }
  }

  async function loadVars(event) {
    try {
      const r = await paperApi.variables(session?.id ?? null, event);
      vars = r.variables ?? [];
      filters = r.filters ?? [];
    } catch {
      vars = [];
    }
  }

  // The preview is the server's renderer, not a local imitation: what it shows is the
  // message that would be sent, including the error a bad path produces.
  let previewTimer;
  $effect(() => {
    if (!open || !session) return;
    const body = {
      name: session.name,
      entry_title_template: session.entry_title_template,
      entry_template: session.entry_template,
      exit_title_template: session.exit_title_template,
      exit_template: session.exit_template,
      summary_title_template: session.summary_title_template,
      summary_template: session.summary_template,
      event: current.event
    };
    clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      try {
        preview = await paperApi.previewDraft(body);
      } catch (e) {
        preview = { title: '', message: e.message };
      }
    }, 250);
  });

  /** Insert a path at the caret of whichever template of the open tab was last focused. */
  function insert(path) {
    const el = focusedEl;
    const token = `{{${path}}}`;
    const field = focused === 'title' ? current.title : current.body;
    const text = session[field] ?? '';
    const at = el?.selectionStart ?? text.length;
    session[field] = text.slice(0, at) + token + text.slice(el?.selectionEnd ?? at);
    queueMicrotask(() => {
      el?.focus();
      const pos = at + token.length;
      el?.setSelectionRange?.(pos, pos);
    });
  }

  async function save() {
    error = '';
    saving = true;
    try {
      const saved = session.id
        ? await paperApi.update(session.id, session)
        : await paperApi.create(session);
      open = false;
      onsaved(saved);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal
  bind:open
  size="lg"
  title={editing ? $t('backtest.paper.edit') : $t('backtest.paper.start')}
>
  {#if session}
    <div class="form">
      <!-- Identity + what is traded. Frozen at creation: the session must not follow
           later edits of the strategy it came from. -->
      <section class="sec first">
        <div class="grid">
          <div class="span2">
            <Input label={$t('backtest.paper.name')} bind:value={session.name} required />
          </div>
        </div>
        <div class="frozen">
          <span class="fz-label"><Icon name="lock" size={11} />{$t('backtest.paper.frozen')}</span>
          <span class="chips">
            {#each tickers as ticker (ticker)}<span class="chip">{ticker}</span>{/each}
          </span>
          {#if oneditstrategy && editing}
            <button class="fz-edit" onclick={() => oneditstrategy(session)}>
              <Icon name="pencil" size={12} />
              {$t('backtest.paper.editStrategy')}
            </button>
          {/if}
          <span class="fz-hint">{$t('backtest.paper.frozenHint')}</span>
        </div>
      </section>

      <details class="sec" open>
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('backtest.paper.feed')}</h4>
          </span>
          <span class="sec-value">{feedLabel}</span>
        </summary>
        <p class="sec-desc">{$t('backtest.paper.feedHint')}</p>
        <div class="seg" role="group" aria-label={$t('backtest.paper.feed')}>
          {#each FEEDS as f (f)}
            <button
              class:on={(session.feed ?? 'hist') === f}
              disabled={f === 'live'}
              title={f === 'live' ? $t('backtest.paper.feedSoon') : ''}
              onclick={() => (session.feed = f)}
            >
              {$t(`backtest.paper.feed.${f}`)}
            </button>
          {/each}
        </div>
      </details>

      {#if (session.feed ?? 'hist') === 'hist'}
        <details class="sec">
          <summary class="sec-head">
            <span class="sec-title">
              <Icon name="chevron-right" size={12} />
              <h4>{$t('backtest.paper.when')}</h4>
            </span>
            <span class="sec-value">{whenLabel}</span>
          </summary>
          <p class="sec-desc">{$t('backtest.paper.whenHint')}</p>
          <SchedulePicker bind:rule={session} />
        </details>
      {/if}

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('backtest.paper.window')}</h4>
          </span>
          <span class="sec-value">{windowLabel}</span>
        </summary>
        <div class="grid">
          <Input
            label={$t('backtest.paper.windowBars')}
            type="number"
            bind:value={session.window_bars}
            hint={$t('backtest.paper.windowHint')}
          />
        </div>
      </details>

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('backtest.paper.reporting')}</h4>
          </span>
          <span class="sec-value">{reportingLabel}</span>
        </summary>
        <div class="sec-bar">
          <p class="sec-desc">{$t('backtest.paper.channelsHint')}</p>
          <ChannelButton module="backtest" size="sm" onchanged={loadChannels} />
        </div>
        <div class="grid">
          <Select
            label={$t('backtest.paper.notifyWhen')}
            options={notifyOptions}
            bind:value={session.notify_when}
          />
          {#if digestUseful}
            <Select
              label={$t('backtest.paper.cadenceLabel')}
              options={cadenceOptions}
              bind:value={session.cadence}
            />
          {/if}
          <div class="fld">
            <span class="lbl">{$t('backtest.paper.channels')}</span>
            <MultiSelect
              bind:value={session.channel_ids}
              options={channelOptions}
              allLabel={$t('backtest.paper.allChannels')}
              width="100%"
            />
          </div>
          <div class="fld">
            <span class="lbl">{$t('backtest.paper.instruments')}</span>
            <MultiSelect
              bind:value={session.notify_tickers}
              options={tickerOptions}
              allLabel={$t('backtest.paper.allInstruments')}
              width="100%"
            />
          </div>
          <!-- Which fills are worth a message. The log keeps everything either way, so
               this only decides what leaves for a channel. -->
          <div class="fld span2">
            <span class="lbl">{$t('backtest.paper.alertOn')}</span>
            <div class="checks">
              <label class="chk">
                <input type="checkbox" bind:checked={session.notify_open} />
                {$t('backtest.paper.onOpen')}
              </label>
              <label class="chk">
                <input type="checkbox" bind:checked={session.notify_close} />
                {$t('backtest.paper.onClose')}
              </label>
            </div>
          </div>
        </div>
      </details>

      <!-- Three messages, three wordings. Each tab owns its templates, its vocabulary
           and its preview: an entry cannot read the exit's numbers, and the summary
           reads no trade at all. -->
      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('backtest.paper.message')}</h4>
          </span>
          <span class="sec-value">{messageLabel}</span>
        </summary>

        <div class="tabs" role="tablist">
          {#each kinds as k (k.id)}
            <button
              role="tab"
              aria-selected={msgKind === k.id}
              class:on={msgKind === k.id}
              onclick={() => (msgKind = k.id)}
            >
              {$t(`backtest.paper.kind.${k.id}`)}
              {#if session[k.title] || session[k.body]}<span class="mark">•</span>{/if}
            </button>
          {/each}
        </div>

        <div class="sec-bar">
          <p class="sec-desc">{$t(`backtest.paper.kindHint.${msgKind}`)}</p>
          <button class="link" onclick={() => (helperOpen = !helperOpen)}>
            <Icon name={helperOpen ? 'chevron-up' : 'chevron-down'} size={11} />
            {$t('backtest.paper.variables')}
          </button>
        </div>

        {#if helperOpen}
          <div class="helper">
            <p class="hint">{$t('backtest.paper.variablesHint')}</p>
            <div class="vars">
              {#each vars as v (v.path)}
                <button class="var" onclick={() => insert(v.path)} title={String(v.sample)}>
                  <code>{v.path}</code>
                  <span class="type">{v.type}</span>
                </button>
              {/each}
            </div>
            {#if filters.length}
              <p class="hint filters">
                {$t('backtest.paper.filters')}
                {#each filters as f (f)}<code class="f">| {f}</code>{/each}
              </p>
            {/if}
          </div>
        {/if}

        {#key msgKind}
          <div class="grid">
            <div class="span2">
              <Input
                label={$t('backtest.paper.titleTemplate')}
                bind:value={session[current.title]}
                placeholder={$t('backtest.paper.defaultWording')}
                onfocus={(e) => { focused = 'title'; focusedEl = e.currentTarget; }}
              />
            </div>
            <div class="span2">
              <Input
                label={$t('backtest.paper.bodyTemplate')}
                bind:value={session[current.body]}
                placeholder={$t('backtest.paper.defaultWording')}
                multiline
                rows={4}
                onfocus={(e) => { focused = 'body'; focusedEl = e.currentTarget; }}
              />
            </div>
          </div>
        {/key}

        <div class="preview">
          <div class="pv-head">
            <span>{$t('backtest.paper.preview')}</span>
            <span class="pv-kind">{$t(`backtest.paper.kind.${msgKind}`)}</span>
          </div>
          <div class="pv-body">
            <strong>{preview.title}</strong>
            <p>{preview.message}</p>
          </div>
        </div>
      </details>

      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  /* One rhythm for the whole form: sections separated by a filet, a two-column grid
     inside each, and every label carried by the shared Field scaffolding. A control
     never spans the dialog unless its content does (a name, a message body). */
  .form {
    display: flex;
    flex-direction: column;
  }
  .sec {
    padding: var(--space-3) 0;
    border-top: var(--hairline) solid var(--border);
  }
  /* An open section breathes; a closed one is just its header line. */
  details.sec[open] > :global(:not(summary)) {
    margin-top: var(--space-3);
  }
  section.sec {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .sec.first {
    border-top: 0;
    padding-top: 0;
  }
  /* A section is a decision: closed it shows the answer, open it shows the controls.
     <details> carries the state, so nothing here needs script. */
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    cursor: pointer;
    list-style: none;
    user-select: none;
  }
  .sec-head::-webkit-details-marker {
    display: none;
  }
  .sec-title {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--dim);
  }
  .sec-title :global(svg) {
    transition: transform var(--dur-fast) var(--ease);
  }
  details[open] > .sec-head .sec-title :global(svg) {
    transform: rotate(90deg);
  }
  .sec-head:hover .sec-title,
  .sec-head:hover .sec-value {
    color: var(--text);
  }
  h4 {
    margin: 0;
    font-size: var(--text-xs);
    font-weight: var(--fw-normal);
    color: inherit;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  /* The answer, read without opening anything. */
  .sec-value {
    font-size: var(--text-xs);
    color: var(--muted);
    text-align: right;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The sentence that explains the whole section sits under its title, not adrift
     between two controls. */
  .sec-desc {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    max-width: 62ch;
  }
  .sec-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  .span2 {
    grid-column: 1 / -1;
  }
  /* Same shape as Field, for controls that are not Input/Select. */
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .lbl {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
  }

  /* What is traded, read in one line: the lock, the instruments, the caveat. */
  .frozen {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    padding: var(--space-2) var(--space-3);
  }
  .fz-label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  /* The way out of a frozen block: the strategy is edited where it is built, not here. */
  .fz-edit {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: 2px var(--space-2);
    color: var(--text);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .fz-edit:hover {
    background: var(--surface);
  }
  .fz-hint {
    flex: 1 1 100%;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .chip {
    border: var(--hairline) solid var(--border-control);
    padding: 1px var(--space-2);
    font-family: var(--mono);
    font-size: var(--text-xs);
  }

  /* One frame, hairline dividers: a segmented control, not two loose buttons. */
  /* Same track as the message tabs: one frame, an inset pill for the answer. */
  .seg {
    display: inline-flex;
    align-items: stretch;
    gap: 2px;
    padding: 2px;
    border: var(--hairline) solid var(--border-control);
    width: fit-content;
  }
  .seg button {
    background: transparent;
    border: 0;
    border-radius: 0;
    padding: 5px var(--space-4);
    margin: 0;
    line-height: 1;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .seg button:hover:not(.on):not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .seg button.on {
    background: var(--text);
    color: var(--surface);
    font-weight: var(--fw-medium);
  }
  .seg button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .checks {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
    min-height: var(--control-h);
    align-items: center;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  /* One tab per message. The dot says the wording was written, so a session's three
     messages are readable without opening each one. */
  /* A track with a pill inside it: the group owns 2px of padding, so the selected fill
     is inset on every side and can never ride over the frame. */
  .tabs {
    display: inline-flex;
    align-items: stretch;
    gap: 2px;
    padding: 2px;
    border: var(--hairline) solid var(--border-control);
    width: fit-content;
  }
  .tabs button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 0;
    border-radius: 0;
    padding: 5px var(--space-4);
    margin: 0;
    line-height: 1;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .tabs button:hover:not(.on) {
    background: var(--surface-2);
    color: var(--text);
  }
  .tabs button.on {
    background: var(--text);
    color: var(--surface);
    font-weight: var(--fw-medium);
  }
  .tabs button.on .mark {
    color: var(--surface);
  }
  .mark {
    color: var(--accent);
    line-height: 1;
  }
  .pv-kind {
    text-transform: none;
    letter-spacing: 0;
    color: var(--muted);
  }

  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .link {
    background: none;
    border: 0;
    color: var(--accent);
    font-size: var(--text-xs);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .helper {
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .vars {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    max-height: 160px;
    overflow-y: auto;
  }
  .var {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: 2px var(--space-2);
    cursor: pointer;
    font-size: var(--text-xs);
  }
  .var:hover {
    border-color: var(--accent);
  }
  .var .type {
    color: var(--muted);
  }
  .filters {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
    align-items: center;
  }
  .f {
    color: var(--muted);
  }

  /* The message as it will land, not as it is typed. */
  .preview {
    border: var(--hairline) solid var(--border);
  }
  .pv-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) var(--space-2);
    background: var(--surface-2);
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-xs);
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .pv-body {
    padding: var(--space-3);
    font-size: var(--text-sm);
  }
  .pv-body p {
    margin: var(--space-1) 0 0;
    white-space: pre-wrap;
    color: var(--muted);
  }
  .err {
    margin: var(--space-3) 0 0;
    color: var(--red);
    font-size: var(--text-sm);
  }

  @media (max-width: 640px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
