<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Time Tracker module. Timer cards with play/stop (server-side running state), live
  // elapsed, budget alerts, and hourly-rate value. A resume prompt offers to revert a
  // timer that kept running while the browser was closed. Breakdown by day/week/month.
  import { onMount, onDestroy } from 'svelte';
  import {
    timeApi,
    fmtDuration,
    fmtHours,
    fmtMoney,
    budgetLevel,
    CURRENCIES
  } from '$lib/modules/time/api.js';
  import {
    fmtDateTime,
    entrySeconds,
    localInputToRfc3339
  } from '$lib/modules/time/api.js';
  import ProjectForm from '$lib/modules/time/ProjectForm.svelte';
  import BucketChart from '$lib/modules/time/BucketChart.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { t } from '$lib/i18n';

  let projects = $state([]);
  let state = $state({ display_currency: 'USD', any_running: false, last_seen_at: null });
  let loading = $state(true);
  let view = $state('timers'); // timers | breakdown

  // A live clock that ticks every second so running cards update; `now` drives elapsed.
  let now = $state(Date.now());
  // Client timestamp captured each time project data is loaded. `tracked_seconds` is the
  // truth at this instant, so running cards tick forward smoothly as `now - loadedAt`.
  let loadedAt = $state(Date.now());
  let tick;
  let heartbeatTimer;

  // Resume prompt: shown when a timer was still running and the app was away a while.
  let showResume = $state(false);

  let showForm = $state(false);
  let editing = $state(null);

  // Selected timer + its saved ranges (shown in the details section below the cards).
  let selectedId = $state(null);
  let entries = $state([]);
  let entriesLoading = $state(false);
  const selected = $derived(projects.find((p) => p.id === selectedId) ?? null);

  // Add-range form. Either an explicit end, or a duration (hours/minutes) from the start.
  let arStart = $state('');
  let arEnd = $state('');
  let arHours = $state('');
  let arMinutes = $state('');
  let arMode = $state('end'); // 'end' | 'duration'
  let arNote = $state('');
  let arError = $state('');
  let arSaving = $state(false);

  async function selectProject(p) {
    if (selectedId === p.id) {
      selectedId = null; // clicking the open card collapses it
      return;
    }
    selectedId = p.id;
    resetAddRange();
    await loadEntries();
  }

  async function loadEntries() {
    if (!selectedId) return;
    entriesLoading = true;
    entries = await timeApi.listEntries(selectedId, 200);
    entriesLoading = false;
  }

  function resetAddRange() {
    arStart = '';
    arEnd = '';
    arHours = '';
    arMinutes = '';
    arNote = '';
    arError = '';
  }

  async function addRange() {
    arError = '';
    const started_at = localInputToRfc3339(arStart);
    if (!started_at) {
      arError = $t('time.details.errPickStart');
      return;
    }
    const body = { started_at, note: arNote || undefined };
    if (arMode === 'end') {
      const ended_at = localInputToRfc3339(arEnd);
      if (!ended_at) {
        arError = $t('time.details.errPickEnd');
        return;
      }
      if (new Date(ended_at) <= new Date(started_at)) {
        arError = $t('time.details.errEndAfterStart');
        return;
      }
      body.ended_at = ended_at;
    } else {
      const secs = (Number(arHours) || 0) * 3600 + (Number(arMinutes) || 0) * 60;
      if (!(secs > 0)) {
        arError = $t('time.details.errDurationPositive');
        return;
      }
      body.duration_seconds = secs;
    }
    arSaving = true;
    try {
      await timeApi.createEntry(selectedId, body);
      resetAddRange();
      await Promise.all([loadEntries(), load()]); // refresh entries + the card's total
    } catch (e) {
      arError = e.message ?? $t('time.details.errAddRange');
    } finally {
      arSaving = false;
    }
  }

  // Confirm modals (no native alerts).
  let confirmEntry = $state(null); // entry pending deletion
  let confirmProject = $state(null); // project pending deletion

  function delEntry(en) {
    confirmEntry = en;
  }
  async function confirmDelEntry() {
    const en = confirmEntry;
    confirmEntry = null;
    await timeApi.deleteEntry(en.id);
    await Promise.all([loadEntries(), load()]);
  }

  // Breakdown state.
  let bd = $state(null);
  let bBucket = $state('day');
  let bProject = $state('');
  let bCategory = $state('');
  let bSince = $state('');
  let bUntil = $state('');

  const categories = $derived([
    ...new Set(projects.map((p) => p.category).filter(Boolean))
  ]);

  // ── UI state (persisted in localStorage, per-browser) ──
  // Active tab, the open timer card, and the breakdown filters survive a refresh.
  const PREFS_KEY = 'otw.time.prefs.v1';
  let prefsLoaded = false; // gate the save effect until after we restore

  function loadPrefs() {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (p.view === 'timers' || p.view === 'breakdown') view = p.view;
      if (['day', 'week', 'month'].includes(p.bBucket)) bBucket = p.bBucket;
      if (typeof p.bProject === 'string') bProject = p.bProject;
      if (typeof p.bCategory === 'string') bCategory = p.bCategory;
      if (typeof p.bSince === 'string') bSince = p.bSince;
      if (typeof p.bUntil === 'string') bUntil = p.bUntil;
      if (Number.isInteger(p.selectedId)) return p.selectedId; // open timer card
    } catch {
      /* corrupt prefs — ignore, use defaults */
    }
    return null;
  }

  // Persist on any change once initial load is done.
  $effect(() => {
    const snapshot = { view, bBucket, bProject, bCategory, bSince, bUntil, selectedId };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snapshot));
    } catch {
      /* quota / unavailable — non-fatal */
    }
  });

  onMount(async () => {
    const restoredId = loadPrefs();
    await load();
    loading = false;
    // Reopen the previously selected card if it still exists.
    if (restoredId != null && projects.some((p) => p.id === restoredId)) {
      selectedId = restoredId;
      loadEntries();
    }
    prefsLoaded = true;
    // If a timer is running and we were away > 2 min, offer to revert.
    if (state.any_running && state.last_seen_at) {
      const awayMs = Date.now() - new Date(state.last_seen_at).getTime();
      if (awayMs > 2 * 60 * 1000) showResume = true;
    }
    await timeApi.heartbeat();
    tick = setInterval(() => (now = Date.now()), 1000);
    // Heartbeat every 60s while open so "last seen" stays current for the revert flow.
    heartbeatTimer = setInterval(() => timeApi.heartbeat(), 60 * 1000);
  });

  onDestroy(() => {
    clearInterval(tick);
    clearInterval(heartbeatTimer);
  });

  async function load() {
    [projects, state] = await Promise.all([timeApi.listProjects(), timeApi.getState()]);
    // Anchor live timers to this load: tracked_seconds is the truth as of now.
    loadedAt = Date.now();
  }

  // ── Drag & drop reorder ──
  // dragId = card being dragged; dropId = card currently hovered as drop target.
  let dragId = $state(null);
  let dropId = $state(null);

  function onDragStart(e, p) {
    dragId = p.id;
    e.dataTransfer.effectAllowed = 'move';
  }
  function onDragOver(e, p) {
    if (!dragId || dragId === p.id) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    dropId = p.id;
  }
  function onDragEnd() {
    dragId = null;
    dropId = null;
  }
  async function onDrop(e, target) {
    e.preventDefault();
    const from = dragId;
    dragId = null;
    dropId = null;
    if (!from || from === target.id) return;

    // Reorder locally, then persist the moved card's new position as the midpoint
    // between its new neighbours (halved below the first / +1 above the last).
    const list = [...projects];
    const fromIdx = list.findIndex((p) => p.id === from);
    const toIdx = list.findIndex((p) => p.id === target.id);
    if (fromIdx < 0 || toIdx < 0) return;
    const [moved] = list.splice(fromIdx, 1);
    list.splice(toIdx, 0, moved);
    projects = list; // optimistic

    const i = list.findIndex((p) => p.id === from);
    const prev = list[i - 1];
    const next = list[i + 1];
    let pos;
    if (!prev) pos = (next?.position ?? 1) - 1;
    else if (!next) pos = (prev.position ?? 0) + 1;
    else pos = (prev.position + next.position) / 2;

    try {
      await timeApi.setPosition(from, pos);
    } finally {
      await load();
    }
  }

  // Elapsed seconds for a card. `tracked_seconds` (closed + running, server-computed) is
  // accurate as of `loadedAt`; for a running timer we simply tick forward by the elapsed
  // wall-clock since load. Using a single fixed anchor (no per-render Date.now()) keeps the
  // display smooth and monotonic.
  function liveSeconds(p) {
    if (!p.running) return p.tracked_seconds;
    return p.tracked_seconds + Math.max(now - loadedAt, 0) / 1000;
  }

  async function toggle(p) {
    if (p.running) await timeApi.stop(p.id);
    else await timeApi.start(p.id);
    await load();
  }

  async function resumeKeep() {
    showResume = false;
  }
  async function resumeRevert() {
    await timeApi.revert();
    showResume = false;
    await load();
  }

  function openAdd() {
    editing = null;
    showForm = true;
  }
  function openEdit(p) {
    editing = p;
    showForm = true;
  }
  async function save(payload) {
    if (editing) await timeApi.updateProject(editing.id, payload);
    else await timeApi.addProject(payload);
    showForm = false;
    await load();
  }
  function del(p) {
    confirmProject = p;
  }
  async function confirmDelProject() {
    const p = confirmProject;
    confirmProject = null;
    if (selectedId === p.id) selectedId = null;
    await timeApi.deleteProject(p.id);
    await load();
  }

  // Breakdown fetch.
  const bFilter = $derived({
    bucket: bBucket,
    project_id: bProject,
    category: bCategory,
    since: bSince,
    until: bUntil
  });
  $effect(() => {
    if (view !== 'breakdown') return;
    const f = bFilter;
    timeApi.breakdown(f).then((d) => (bd = d));
  });

  const cur = $derived(state.display_currency);
  async function setCurrency(e) {
    state = await timeApi.updateSettings({ display_currency: e.target.value });
    if (view === 'breakdown') bd = await timeApi.breakdown(bFilter);
  }

  function budgetPct(p) {
    if (!p.time_budget_hours) return null;
    return Math.min((liveSeconds(p) / 3600 / p.time_budget_hours) * 100, 100);
  }
  function rawPct(p) {
    if (!p.time_budget_hours) return 0;
    return (liveSeconds(p) / 3600 / p.time_budget_hours) * 100;
  }
  function rateValue(p) {
    if (!p.hourly_rate) return null;
    return (liveSeconds(p) / 3600) * p.hourly_rate;
  }
  // Cost of a single entry at the selected project's hourly rate.
  function entryCost(en) {
    if (!selected?.hourly_rate) return null;
    return (entrySeconds(en) / 3600) * selected.hourly_rate;
  }
  // Show the Cost column only when the selected project has an hourly rate.
  const showCost = $derived(!!selected?.hourly_rate);
</script>

<div class="time">
  <header class="head">
    <div class="title">
      <h1>{$t('time.title')}</h1>
      <nav class="tabs">
        <button class:active={view === 'timers'} onclick={() => (view = 'timers')}>{$t('time.tabs.timers')}</button>
        <button class:active={view === 'breakdown'} onclick={() => (view = 'breakdown')}>{$t('time.tabs.breakdown')}</button>
      </nav>
    </div>
    <div class="head-right">
      <label class="cur-select">
        <span>{$t('time.display')}</span>
        <select value={cur} onchange={setCurrency}>
          {#each CURRENCIES as c}<option value={c}>{c}</option>{/each}
        </select>
      </label>
      <QuickReminderButton title={$t('time.addReminder')} />
      {#if view === 'timers'}
        <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('time.addProject')}</button>
      {/if}
    </div>
  </header>

  {#if showResume}
    <div class="resume">
      <span>
        <Icon name="clock" size={14} /> {$t('time.resume.body', { time: new Date(state.last_seen_at).toLocaleString() })}
      </span>
      <div class="resume-actions">
        <button class="ghost" onclick={resumeKeep}>{$t('time.resume.keep')}</button>
        <button class="primary" onclick={resumeRevert}>{$t('time.resume.revert')}</button>
      </div>
    </div>
  {/if}

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if view === 'timers'}
    {#if projects.length === 0}
      <div class="empty">{$t('time.empty')}</div>
    {:else}
      <div class="cards">
        {#each projects as p (p.id)}
          {@const level = budgetLevel(liveSeconds(p) / 3600, p.time_budget_hours)}
          <div
            class="card"
            class:selected={selectedId === p.id}
            class:dragging={dragId === p.id}
            class:drop-target={dropId === p.id}
            style:border-top-color={p.color || 'var(--accent)'}
            role="button"
            tabindex="0"
            draggable="true"
            ondragstart={(e) => onDragStart(e, p)}
            ondragover={(e) => onDragOver(e, p)}
            ondrop={(e) => onDrop(e, p)}
            ondragend={onDragEnd}
            onclick={() => selectProject(p)}
            onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectProject(p)}
          >
            <div class="card-head">
              <div class="names">
                <span class="pname">{p.name}</span>
                {#if p.category}<span class="cat">{p.category}</span>{/if}
              </div>
              <div class="card-actions">
                <button class="icon" title={$t('time.edit')} onclick={(e) => (e.stopPropagation(), openEdit(p))}><Icon name="pencil" size={14} /></button>
                <button class="icon" title={$t('time.delete')} onclick={(e) => (e.stopPropagation(), del(p))}><Icon name="trash" size={14} /></button>
              </div>
            </div>

            <div class="elapsed {p.running ? 'running' : ''}">{fmtDuration(liveSeconds(p))}</div>

            <button
              class="toggle {p.running ? 'stop' : 'play'}"
              onclick={(e) => (e.stopPropagation(), toggle(p))}
            >
              {#if p.running}<Icon name="pause" size={13} /> {$t('time.stop')}{:else}<Icon name="play" size={13} /> {$t('time.start')}{/if}
            </button>

            {#if p.time_budget_hours}
              <div class="budget {level}">
                <div class="bar"><div class="fill" style:width={`${budgetPct(p)}%`}></div></div>
                <span class="budget-label">
                  {fmtHours(liveSeconds(p) / 3600)} / {fmtHours(p.time_budget_hours)}
                  ({rawPct(p).toFixed(0)}%)
                  {#if level === 'over'}— {$t('time.budget.over')}{:else if level === 'high'}— {$t('time.budget.high')}{:else if level === 'warn'}— {$t('time.budget.warn')}{/if}
                </span>
              </div>
            {/if}

            <div class="meta">
              {#if p.hourly_rate}
                <span class="rate">{fmtMoney(rateValue(p), p.rate_currency)}</span>
              {/if}
              {#if p.planned_end}
                <span class="due">{$t('time.due', { date: p.planned_end })}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if selected}
      <section class="details">
        <div class="details-head">
          <h2>
            <span class="dot" style:background={selected.color || 'var(--accent)'}></span>
            {$t('time.details.title', { name: selected.name })}
          </h2>
          <button class="ghost close" onclick={() => (selectedId = null)} title={$t('time.details.close')}><Icon name="x" size={13} /></button>
        </div>

        <form class="add-range" onsubmit={(e) => (e.preventDefault(), addRange())}>
          <div class="ar-row">
            <label class="ar-field">
              <span>{$t('time.details.start')}</span>
              <input type="datetime-local" bind:value={arStart} />
            </label>

            <div class="ar-mode">
              <div class="seg" role="group" aria-label={$t('time.details.rangeEndMode')}>
                <button type="button" class:active={arMode === 'end'} onclick={() => (arMode = 'end')}>{$t('time.details.endTime')}</button>
                <button type="button" class:active={arMode === 'duration'} onclick={() => (arMode = 'duration')}>{$t('time.details.duration')}</button>
              </div>
              {#if arMode === 'end'}
                <label class="ar-field">
                  <span>{$t('time.details.end')}</span>
                  <input type="datetime-local" bind:value={arEnd} />
                </label>
              {:else}
                <div class="ar-dur">
                  <label class="ar-field sm"><span>{$t('time.details.hours')}</span><input type="number" min="0" step="1" bind:value={arHours} placeholder="3" /></label>
                  <label class="ar-field sm"><span>{$t('time.details.min')}</span><input type="number" min="0" max="59" step="1" bind:value={arMinutes} placeholder="0" /></label>
                </div>
              {/if}
            </div>

            <label class="ar-field grow">
              <span>{$t('time.details.noteOptional')}</span>
              <input type="text" bind:value={arNote} placeholder={$t('time.details.notePlaceholder')} />
            </label>

            <button class="primary add-btn" type="submit" disabled={arSaving}>
              {arSaving ? $t('time.details.adding') : $t('time.details.addRange')}
            </button>
          </div>
          {#if arError}<p class="ar-error">{arError}</p>{/if}
        </form>

        <div class="entries-wrap">
          {#if entriesLoading}
            <p class="muted">{$t('time.details.loadingRanges')}</p>
          {:else if entries.length === 0}
            <p class="muted">{$t('time.details.noRanges')}</p>
          {:else}
            <table class="tbl entries">
              <thead>
                <tr>
                  <th>{$t('time.details.start')}</th>
                  <th>{$t('time.details.end')}</th>
                  <th class="num">{$t('time.details.duration')}</th>
                  {#if showCost}<th class="num">{$t('time.details.cost')}</th>{/if}
                  <th>{$t('time.details.note')}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {#each entries as en (en.id)}
                  <tr>
                    <td>{fmtDateTime(en.started_at)}</td>
                    <td>{en.ended_at ? fmtDateTime(en.ended_at) : $t('time.details.running')}</td>
                    <td class="num">{fmtDuration(entrySeconds(en))}</td>
                    {#if showCost}<td class="num">{fmtMoney(entryCost(en), selected.rate_currency)}</td>{/if}
                    <td class="note">{en.note || '—'}</td>
                    <td class="row-actions">
                      {#if en.ended_at}
                        <button class="icon" title={$t('time.details.deleteRange')} onclick={() => delEntry(en)}><Icon name="trash" size={14} /></button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      </section>
    {/if}
  {:else}
    <section class="filters">
      <select bind:value={bBucket}>
        <option value="day">{$t('time.breakdown.byDay')}</option>
        <option value="week">{$t('time.breakdown.byWeek')}</option>
        <option value="month">{$t('time.breakdown.byMonth')}</option>
      </select>
      <select bind:value={bProject}>
        <option value="">{$t('time.breakdown.allProjects')}</option>
        {#each projects as p}<option value={p.id}>{p.name}</option>{/each}
      </select>
      <select bind:value={bCategory}>
        <option value="">{$t('time.breakdown.allCategories')}</option>
        {#each categories as c}<option value={c}>{c}</option>{/each}
      </select>
      <label class="date">{$t('time.breakdown.from')} <input type="date" bind:value={bSince} /></label>
      <label class="date">{$t('time.breakdown.to')} <input type="date" bind:value={bUntil} /></label>
    </section>

    <section class="bd">
      <div class="card chart-card">
        <h3>{$t('time.breakdown.trackedHours')}</h3>
        <BucketChart points={bd?.points ?? []} />
      </div>
      <div class="rail">
        <div class="stat big">
          <span class="lbl">{$t('time.breakdown.totalHours')}</span>
          <strong>{fmtHours(bd?.total_hours)}</strong>
        </div>
        <div class="stat big">
          <span class="lbl">{$t('time.breakdown.value')} <span class="cur">({cur})</span></span>
          <strong>{fmtMoney(bd?.total_value, cur)}</strong>
        </div>
      </div>
    </section>
  {/if}
</div>

<Modal bind:open={showForm} size="md" title={editing ? $t('time.form.editTitle') : $t('time.form.newTitle')}>
  <ProjectForm initial={editing} {categories} onsubmit={save} oncancel={() => (showForm = false)} />
</Modal>
<Modal open={!!confirmEntry} size="sm" title={$t('time.details.deleteRangeTitle')} onclose={() => (confirmEntry = null)}>
  <div class="confirm">
    <p>{$t('time.details.confirmDeleteRange')}</p>
    <div class="confirm-actions">
      <button class="ghost" onclick={() => (confirmEntry = null)}>{$t('common.cancel')}</button>
      <button class="danger" onclick={confirmDelEntry}>{$t('time.delete')}</button>
    </div>
  </div>
</Modal>

<Modal open={!!confirmProject} size="sm" title={$t('time.confirmDeleteTitle')} onclose={() => (confirmProject = null)}>
  <div class="confirm">
    <p>{@html $t('time.confirmDeleteBody', { name: confirmProject?.name })}</p>
    <div class="confirm-actions">
      <button class="ghost" onclick={() => (confirmProject = null)}>{$t('common.cancel')}</button>
      <button class="danger" onclick={confirmDelProject}>{$t('time.delete')}</button>
    </div>
  </div>
</Modal>

<style>
  .time {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    height: 100%;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-6);
  }
  .title h1 {
    font-size: 1.25rem;
    font-weight: 700;
  }
  .tabs {
    display: flex;
    gap: 2px;
  }
  .tabs button {
    background: transparent;
    border: none;
    color: var(--muted);
    padding: 6px 12px;
    border-radius: var(--radius);
    cursor: pointer;
    font-size: 0.85rem;
  }
  .tabs button.active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
    font-weight: 600;
  }
  .head-right {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .cur-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
  }
  .resume {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--amber) 45%, transparent);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    font-size: 0.85rem;
  }
  .resume-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-4);
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-top: 3px solid var(--accent);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .card.dragging {
    opacity: 0.5;
  }
  .card.drop-target {
    outline: 2px dashed var(--accent);
    outline-offset: -2px;
  }
  .card-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
  }
  .names {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .pname {
    font-weight: 600;
  }
  .cat {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .card-actions {
    display: flex;
    gap: 2px;
    opacity: 0;
  }
  .card:hover .card-actions {
    opacity: 1;
  }
  .icon:hover {
    color: var(--text);
  }
  .elapsed {
    font-size: 1.8rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.02em;
  }
  .elapsed.running {
    color: var(--green);
  }
  .toggle {
    border: none;
    border-radius: var(--radius);
    padding: 8px;
    cursor: pointer;
    font-weight: 600;
    font-size: 0.85rem;
  }
  .toggle.play {
    background: color-mix(in srgb, var(--green) 18%, transparent);
    color: var(--green);
  }
  .toggle.stop {
    background: color-mix(in srgb, var(--red) 18%, transparent);
    color: var(--red);
  }
  .budget {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .bar {
    height: 6px;
    background: var(--surface-2, var(--border));
    border-radius: 999px;
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.5s ease;
  }
  .budget.warn .fill {
    background: var(--amber);
  }
  .budget.high .fill,
  .budget.over .fill {
    background: var(--red);
  }
  .budget-label {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .budget.over .budget-label,
  .budget.high .budget-label {
    color: var(--red);
  }
  .budget.warn .budget-label {
    color: var(--amber);
  }
  .meta {
    display: flex;
    justify-content: space-between;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .rate {
    color: var(--green);
    font-weight: 600;
  }
  /* Breakdown */
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
  }
  .filters .date {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
  }
  .bd {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .card.chart-card h3 {
    font-size: 0.9rem;
    font-weight: 600;
    margin-bottom: var(--space-3);
  }
  .rail {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }
  .stat {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .stat .lbl {
    font-size: 0.7rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .stat strong {
    font-size: 1.05rem;
  }
  .stat.big strong {
    font-size: 1.5rem;
  }
  .cur {
    color: var(--muted);
    font-weight: 400;
    text-transform: none;
  }
  .empty {
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-8);
    text-align: center;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }

  /* Card selection */
  .card {
    cursor: pointer;
  }
  .card.selected {
    box-shadow: 0 0 0 2px var(--accent);
  }

  /* Details section (selected timer's ranges) */
  .details {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .details-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .details-head h2 {
    font-size: 1rem;
    font-weight: 700;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .details-head .dot {
    width: 10px;
    height: 10px;
    border-radius: 999px;
    display: inline-block;
  }
  .close {
    padding: 4px 8px;
  }

  .add-range {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    background: var(--surface-2, var(--surface));
  }
  .ar-row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .ar-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 0.72rem;
    color: var(--muted);
  }
  .ar-field.grow {
    flex: 1;
    min-width: 160px;
  }
  .ar-field.sm {
    width: 72px;
  }
  .ar-mode {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .ar-dur {
    display: flex;
    gap: var(--space-2);
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    width: fit-content;
  }
  .seg button {
    background: transparent;
    border: none;
    color: var(--muted);
    padding: 4px 10px;
    font-size: 0.72rem;
    cursor: pointer;
  }
  .seg button.active {
    background: var(--accent);
    color: #fff;
  }
  .add-btn {
    height: 33px;
  }
  .ar-error {
    color: var(--red);
    font-size: 0.78rem;
    margin-top: var(--space-2);
  }

  .entries-wrap {
    max-height: 320px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  table.entries {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
  }
  table.entries th,
  table.entries td {
    text-align: left;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  table.entries th {
    position: sticky;
    top: 0;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    background: var(--surface);
  }
  table.entries tbody tr:last-child td {
    border-bottom: none;
  }
  table.entries .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  table.entries .note {
    white-space: normal;
    color: var(--muted);
  }
  table.entries .row-actions {
    text-align: right;
  }
  .entries-wrap .muted {
    padding: var(--space-4);
  }

  .confirm p {
    font-size: 0.88rem;
    line-height: 1.5;
  }
  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
