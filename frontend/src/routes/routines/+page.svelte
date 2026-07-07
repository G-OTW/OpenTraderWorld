<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  // Trading Routines — a trading-day board. Pick a date (default today); the board shows the
  // routines due that weekday grouped by session (pre-market / in session / post-market /
  // anytime) with per-item checkboxes, a day progress bar and a 14-day consistency strip.
  // Empty board offers starter checklists. One-off tasks belong to the ToDo module.
  import { traderApi, SESSIONS, STARTER_ROUTINES } from '$lib/modules/routines/api.js';
  import RoutineModal from '$lib/modules/routines/RoutineModal.svelte';
  import { t } from '$lib/i18n';

  let date = $state(todayStr());
  let board = $state(null);
  let error = $state('');
  let loading = $state(false);

  let showRoutine = $state(false);
  let editRoutineId = $state(null);

  // Local-date formatting — toISOString() is UTC and lands on the wrong day for any
  // timezone ahead of/behind UTC (e.g. "+1 day" appearing to do nothing in UTC+2).
  function fmtLocal(d) {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }
  function todayStr() {
    return fmtLocal(new Date());
  }
  function shiftDate(days) {
    const d = new Date(date + 'T00:00:00');
    d.setDate(d.getDate() + days);
    date = fmtLocal(d);
  }

  function load() {
    loading = true;
    error = '';
    traderApi
      .board(date)
      .then((r) => (board = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  }
  $effect(() => {
    date; // re-load whenever the date changes
    load();
  });

  // Day progress across every due routine item.
  let progress = $derived.by(() => {
    const items = (board?.routines ?? []).flatMap((r) => r.items);
    const done = items.filter((i) => i.checked).length;
    return { done, total: items.length, pct: items.length ? (done / items.length) * 100 : 0 };
  });

  let routinesBySession = $derived.by(() => {
    const map = new Map(SESSIONS.map((s) => [s.key, []]));
    for (const r of board?.routines ?? []) (map.get(r.session) ?? map.get('any')).push(r);
    return SESSIONS.map((s) => ({ ...s, routines: map.get(s.key) })).filter(
      (s) => s.routines.length > 0
    );
  });

  // 14-day consistency strip ending at the selected date.
  let strip = $derived.by(() => {
    const ticked = new Set(board?.tick_dates ?? []);
    const out = [];
    const end = new Date(date + 'T00:00:00');
    for (let i = 13; i >= 0; i--) {
      const d = new Date(end);
      d.setDate(end.getDate() - i);
      const key = fmtLocal(d);
      out.push({ key, ticked: ticked.has(key), weekend: [0, 6].includes(d.getDay()) });
    }
    return out;
  });

  async function toggleItem(item) {
    item.checked = !item.checked; // optimistic
    try {
      await traderApi.checkItem(item.id, date, item.checked);
    } catch (e) {
      error = e.message;
      load();
    }
  }

  async function addStarters() {
    try {
      for (const r of STARTER_ROUTINES) await traderApi.createRoutine(r);
      load();
    } catch (e) {
      error = e.message;
    }
  }

  function openEdit(id) {
    editRoutineId = id;
    showRoutine = true;
  }
  function openNew() {
    editRoutineId = null;
    showRoutine = true;
  }
</script>

<div class="page">
  <header>
    <h1>{$t('routines.page.title')}</h1>
    <div class="datenav">
      <button class="btn" onclick={() => shiftDate(-1)} aria-label={$t('routines.page.previousDay')}><Icon name="chevron-left" size={14} /></button>
      <input type="date" bind:value={date} />
      <button class="btn" onclick={() => shiftDate(1)} aria-label={$t('routines.page.nextDay')}><Icon name="chevron-right" size={14} /></button>
      {#if date !== todayStr()}
        <button class="btn" onclick={() => (date = todayStr())}>{$t('routines.page.today')}</button>
      {/if}
    </div>
    <div class="spacer"></div>
    <button class="btn primary" onclick={openNew}>{$t('routines.page.newRoutine')}</button>
  </header>

  {#if error}<p class="err" title="click to copy" use:copyLog={error}>{error}</p>{/if}

  {#if board}
    <div class="status">
      <div class="progress">
        <div class="ptext">
          {@html $t('routines.page.itemsDone', { done: progress.done, total: progress.total })}
          {#if progress.total > 0 && progress.done === progress.total}
            <span class="allset">{$t('routines.page.allSet')}</span>
          {/if}
        </div>
        <div class="pbar"><div class="pfill" style="width:{progress.pct}%"></div></div>
      </div>
      <div class="strip" title={$t('routines.page.tickHint')}>
        {#each strip as d (d.key)}
          <span
            class="cell"
            class:ticked={d.ticked}
            class:weekend={d.weekend}
            class:today={d.key === todayStr()}
            title={d.key}
          ></span>
        {/each}
        <span class="striplbl">{$t('routines.page.consistencyStrip')}</span>
      </div>
    </div>

    <div class="sessions">
      {#if board.routines.length === 0}
          <div class="emptyboard">
            <p class="muted">{$t('routines.page.noRoutinesDue')}</p>
            <p class="muted small">{$t('routines.page.emptyBoardHint')}</p>
            <div class="emptyactions">
              <button class="btn primary" onclick={addStarters}>{$t('routines.page.addStarterChecklists')}</button>
              <button class="btn" onclick={openNew}>{$t('routines.page.buildYourOwn')}</button>
            </div>
          </div>
        {/if}
        {#each routinesBySession as s (s.key)}
          <section class="session">
            <h2><span class="icon">{s.icon}</span>{s.label}</h2>
            {#each s.routines as r (r.id)}
              {@const done = r.items.filter((i) => i.checked).length}
              <div class="routine" class:complete={r.items.length > 0 && done === r.items.length}>
                <div class="rhead">
                  <h3>{r.name}</h3>
                  <span class="rcount">{done}/{r.items.length}</span>
                  <button class="edit" onclick={() => openEdit(r.id)} aria-label={$t('routines.page.editRoutine')}><Icon name="pencil" size={13} /></button>
                </div>
                <ul>
                  {#each r.items as item (item.id)}
                    <li>
                      <label class:done={item.checked}>
                        <input
                          type="checkbox"
                          checked={item.checked}
                          onchange={() => toggleItem(item)}
                        />
                        <span>{item.label}</span>
                      </label>
                    </li>
                  {/each}
                </ul>
              </div>
            {/each}
          </section>
        {/each}
    </div>
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {/if}
</div>

<RoutineModal bind:open={showRoutine} routineId={editRoutineId} onsaved={load} />

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  h1 {
    font-size: 1.4rem;
    font-weight: 700;
  }
  .datenav {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--accent);
  }

  .status {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    flex-wrap: wrap;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
  }
  .progress {
    flex: 1 1 280px;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .ptext {
    font-size: 0.85rem;
    color: var(--muted);
  }
  .ptext strong {
    color: var(--text);
  }
  .allset {
    color: var(--green);
  }
  .pbar {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .pfill {
    height: 100%;
    background: var(--green);
    border-radius: 3px;
    transition: width 0.25s ease;
  }
  .strip {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .cell {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
  .cell.weekend {
    opacity: 0.45;
  }
  .cell.ticked {
    background: var(--green);
    border-color: var(--green);
  }
  .cell.today {
    outline: 1px solid var(--accent);
    outline-offset: 1px;
  }
  .striplbl {
    margin-left: var(--space-2);
    font-size: 0.72rem;
    color: var(--muted);
  }

  .sessions {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .session h2 {
    font-size: 0.95rem;
    font-weight: 700;
    margin-bottom: var(--space-2);
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .session {
    display: flex;
    flex-direction: column;
  }
  .routine {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    margin-bottom: var(--space-2);
  }
  .routine.complete {
    border-color: var(--green);
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .rhead h3 {
    font-size: 0.9rem;
    font-weight: 600;
    flex: 1;
  }
  .rcount {
    font-size: 0.75rem;
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
  }
  .edit {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.85rem;
  }
  .edit:hover {
    color: var(--text);
  }
  .routine ul {
    list-style: none;
    margin-top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .routine label {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: 0.88rem;
    cursor: pointer;
    padding: 2px 0;
  }
  .routine label.done span {
    color: var(--muted);
    text-decoration: line-through;
  }
  .emptyboard {
    background: var(--surface);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
  }
  .emptyactions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.82rem;
  }
  .err {
    color: var(--red);
  }
</style>
