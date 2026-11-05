<script>
  // Board — the day view of the Routines module.
  //
  // Pick a date (default today); the board shows the templates due that date, grouped by
  // category, with per-step checkboxes, a day progress bar and a 14-day consistency strip.
  // A step carrying a note or a link says so and unfolds it on click — the *how* sits one
  // click behind the *what*, so a ticked-through checklist stays fast but never loses the
  // detail behind a step.
  //
  // Authoring lives on the Templates page; this page only ticks. Empty board points there.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ConsistencyModal from '$lib/ui/ConsistencyModal.svelte';
  import {
    traderApi,
    describeSchedule,
    fmtLocal,
    todayStr,
    parseLocal,
    periodStats
  } from '$lib/modules/routines/api.js';
  import { t, locale } from '$lib/i18n';

  let date = $state(todayStr());
  let board = $state(null);
  let error = $state('');
  let loading = $state(false);
  let expanded = $state(new Set()); // item ids whose note/link is unfolded

  const NAV_LINKS = [
    { href: '/routines', icon: 'check-square', key: 'routines.nav.board' },
    { href: '/routines/templates', icon: 'clipboard-list', key: 'routines.nav.templates' }
  ];
  const PERIODS = ['week', 'month', 'year'];
  let period = $state('week');
  let showConsistency = $state(false);

  // Category ids the user has folded away. Persisted so the board opens the way it was left.
  const COLLAPSE_KEY = 'routines.board.collapsed';
  let collapsed = $state(new Set(readCollapsed()));

  function readCollapsed() {
    try {
      return JSON.parse(localStorage.getItem(COLLAPSE_KEY) ?? '[]');
    } catch {
      return [];
    }
  }

  function shiftDate(days) {
    const d = parseLocal(date);
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

  // Day progress across every due step.
  let progress = $derived.by(() => {
    const items = (board?.routines ?? []).flatMap((r) => r.items);
    const done = items.filter((i) => i.checked).length;
    return { done, total: items.length, pct: items.length ? (done / items.length) * 100 : 0 };
  });

  // Grouped by category, in category order, with uncategorised last — the same structure
  // as the Templates page, so a routine sits in the same place on both.
  let groups = $derived.by(() => {
    const cats = board?.categories ?? [];
    const byCat = new Map(cats.map((c) => [c.id, []]));
    const loose = [];
    for (const r of board?.routines ?? []) {
      const bucket = byCat.get(r.category_id);
      if (bucket) bucket.push(r);
      else loose.push(r);
    }
    const out = cats
      .map((c) => ({ category: c, routines: byCat.get(c.id) ?? [] }))
      .filter((g) => g.routines.length > 0);
    if (loose.length > 0) out.push({ category: null, routines: loose });
    return out;
  });

  // Consistency over the chosen period, counted from the day marks (not from ticks — a
  // day counts because the trader says it did, see ConsistencyModal).
  let stats = $derived(periodStats(board?.marks ?? [], period, parseLocal(date)));

  // The bar's one-click verdict for the selected day: claim it, or take the claim back.
  async function markRoutineDone() {
    const next = board.mark === 'full' ? null : 'full';
    const before = board.mark;
    board = { ...board, mark: next, marks: applyMark(board.marks, date, next) };
    try {
      await traderApi.setMark(date, next);
    } catch (e) {
      error = e.message;
      board = { ...board, mark: before, marks: applyMark(board.marks, date, before) };
    }
  }

  function applyMark(marks, day, mark) {
    const rest = (marks ?? []).filter((m) => m.day !== day);
    return mark ? [...rest, { day, mark }] : rest;
  }

  const dayLabel = $derived(
    parseLocal(date).toLocaleDateString($locale, {
      weekday: 'long',
      day: 'numeric',
      month: 'long'
    })
  );

  async function toggleItem(item) {
    item.checked = !item.checked; // optimistic
    try {
      await traderApi.checkItem(item.id, date, item.checked);
      // The strip is server-derived; a first tick on a fresh day has to come back from it.
      const dates = new Set(board.tick_dates ?? []);
      if (item.checked) dates.add(date);
      board = { ...board, tick_dates: [...dates] };
    } catch (e) {
      error = e.message;
      load();
    }
  }

  function toggleNote(id) {
    // A Set in a $state rune must be reassigned to trigger reactivity.
    const next = new Set(expanded);
    next.has(id) ? next.delete(id) : next.add(id);
    expanded = next;
  }

  // Keys (category id or routine id) with a bulk tick in flight, so a double-click can't
  // fire two passes over the same items.
  let busyGroups = $state(new Set());

  /// Tick (or untick) a whole set of steps at once — the "I ran this" gesture, used by
  /// both the category header and each routine. Only the items that actually change are
  /// sent; the rest cost nothing.
  async function bulkCheck(key, items, target) {
    const pending = items.filter((i) => i.checked !== target);
    if (pending.length === 0) return;

    busyGroups = new Set(busyGroups).add(key);
    for (const i of pending) i.checked = target; // optimistic
    try {
      await Promise.all(pending.map((i) => traderApi.checkItem(i.id, date, target)));
      const dates = new Set(board.tick_dates ?? []);
      if (target) dates.add(date);
      board = { ...board, tick_dates: [...dates] };
    } catch (e) {
      error = e.message;
      load(); // one failed write leaves the set half-ticked — refetch the truth
    } finally {
      const n = new Set(busyGroups);
      n.delete(key);
      busyGroups = n;
    }
  }

  const toggleGroupDone = (group, allDone) =>
    bulkCheck(
      group.category?.id ?? 'none',
      group.routines.flatMap((r) => r.items),
      !allDone
    );

  const toggleRoutineDone = (routine, allDone) =>
    bulkCheck(routine.id, routine.items, !allDone);

  // ── Quick tasks ──
  // One-off to-dos next to the recurring checklists. The board sends the open ones plus
  // those ticked on the viewed date, already sorted (open first, then priority, then due).
  const PRIORITIES = ['low', 'normal', 'high'];
  let newTitle = $state('');
  let newPriority = $state('normal');
  let newDue = $state('');
  let adding = $state(false);

  const openTasks = $derived((board?.tasks ?? []).filter((x) => !x.done).length);

  const nextPriority = (p) => PRIORITIES[(PRIORITIES.indexOf(p) + 1) % PRIORITIES.length];
  const priorityLabel = (p) =>
    $t(`routines.tasks.priority${p.charAt(0).toUpperCase()}${p.slice(1)}`);

  async function addTask(e) {
    e.preventDefault();
    const title = newTitle.trim();
    if (!title || adding) return;
    adding = true;
    try {
      const task = await traderApi.addTask({
        title,
        priority: newPriority,
        due_date: newDue || null
      });
      board = { ...board, tasks: [...(board.tasks ?? []), task] };
      newTitle = '';
      newPriority = 'normal';
      newDue = '';
    } catch (err) {
      error = err.message;
    } finally {
      adding = false;
    }
  }

  async function patchTask(task, patch) {
    Object.assign(task, patch); // optimistic
    try {
      await traderApi.updateTask(task.id, patch);
    } catch (err) {
      load(); // refetch the truth; load() clears the error, so set it after
      error = err.message;
    }
  }

  async function deleteTask(task) {
    board = { ...board, tasks: board.tasks.filter((x) => x.id !== task.id) };
    try {
      await traderApi.deleteTask(task.id);
    } catch (err) {
      load(); // refetch the truth; load() clears the error, so set it after
      error = err.message;
    }
  }

  const fmtDue = (iso) =>
    parseLocal(iso).toLocaleDateString($locale, { day: 'numeric', month: 'short' });

  function toggleGroup(key) {
    const next = new Set(collapsed);
    next.has(key) ? next.delete(key) : next.add(key);
    collapsed = next;
    // Folding is a per-user habit ("I never look at post-market before the close"), so it
    // outlives the page: without this a reload undoes every fold.
    try {
      localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...next]));
    } catch {
      /* private mode / quota — folding just stops persisting */
    }
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="routines.nav.label" />

  <PageHeader title={$t('routines.page.title')} subtitle={dayLabel}>
    {#snippet actions()}
      <div class="datenav">
        <button class="btn" onclick={() => shiftDate(-1)} aria-label={$t('routines.page.previousDay')}>
          <Icon name="chevron-left" size={14} />
        </button>
        <input type="date" bind:value={date} />
        <button class="btn" onclick={() => shiftDate(1)} aria-label={$t('routines.page.nextDay')}>
          <Icon name="chevron-right" size={14} />
        </button>
        {#if date !== todayStr()}
          <button class="btn" onclick={() => (date = todayStr())}>{$t('routines.page.today')}</button>
        {/if}
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

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
      <div class="consist">
        <div class="periods" role="group" aria-label={$t('routines.period.label')}>
          {#each PERIODS as p (p)}
            <button class="chip" class:active={period === p} onclick={() => (period = p)}>
              {$t(`routines.period.${p}`)}
            </button>
          {/each}
        </div>
        <!-- One stable readout: the green/amber split lives in the calendar, so the bar's
             width never depends on whether there is anything to split. -->
        <button class="tally" onclick={() => (showConsistency = true)}>
          <b>{stats.done}</b><span class="of">/{stats.total}</span>
          <span class="tlbl">{$t('routines.period.daysDone')}</span>
          <Icon name="calendar-days" size={13} />
        </button>
        <button
          class="markbtn"
          class:on={board.mark === 'full'}
          onclick={markRoutineDone}
          title={$t('routines.page.markRoutineDoneHint')}
        >
          <Icon name={board.mark === 'full' ? 'check-circle' : 'check'} size={13} />
          {board.mark === 'full'
            ? $t('routines.page.routineDone')
            : $t('routines.page.markRoutineDone')}
        </button>
      </div>
    </div>

    <section class="tasks">
      <h2 class="thead">
        {$t('routines.tasks.title')}
        {#if openTasks > 0}
          <span class="gcount">{$t('routines.tasks.openCount', { n: openTasks })}</span>
        {/if}
      </h2>
      <!-- One frame for the whole add row: the priority dot, the title and the date sit
           inside it borderless, so the row reads as a single control. -->
      <form class="tadd" onsubmit={addTask}>
        <button
          type="button"
          class="pri {newPriority}"
          onclick={() => (newPriority = nextPriority(newPriority))}
          title={$t('routines.tasks.priorityHint', { level: priorityLabel(newPriority) })}
          aria-label={$t('routines.tasks.priorityHint', { level: priorityLabel(newPriority) })}
        ></button>
        <input
          class="ttitle-in"
          type="text"
          bind:value={newTitle}
          placeholder={$t('routines.tasks.addPlaceholder')}
          aria-label={$t('routines.tasks.addPlaceholder')}
          disabled={adding}
        />
        <input
          class="tdue-in"
          type="date"
          bind:value={newDue}
          title={$t('routines.tasks.dueDate')}
          aria-label={$t('routines.tasks.dueDate')}
        />
        <!-- Also what makes Enter submit: with two fields, implicit submission needs one. -->
        <button
          type="submit"
          class="tsubmit"
          disabled={adding || !newTitle.trim()}
          title={$t('common.add')}
          aria-label={$t('common.add')}
        >
          <Icon name="plus" size={13} />
        </button>
      </form>
      {#if (board.tasks ?? []).length > 0}
        <ul class="tlist">
          {#each board.tasks as task (task.id)}
            <li class="task">
              <button
                type="button"
                class="pri {task.priority}"
                disabled={task.done}
                onclick={() => patchTask(task, { priority: nextPriority(task.priority) })}
                title={$t('routines.tasks.priorityHint', { level: priorityLabel(task.priority) })}
                aria-label={$t('routines.tasks.priorityHint', { level: priorityLabel(task.priority) })}
              ></button>
              <label class:done={task.done}>
                <input
                  type="checkbox"
                  checked={task.done}
                  onchange={() => patchTask(task, { done: !task.done })}
                />
                <span>{task.title}</span>
              </label>
              {#if task.due_date}
                <span
                  class="tdue"
                  class:overdue={!task.done && task.due_date < date}
                  title={$t('routines.tasks.dueOn', { date: task.due_date })}
                >
                  <Icon name="calendar-days" size={11} />
                  {fmtDue(task.due_date)}
                </span>
              {/if}
              <button
                type="button"
                class="tdel"
                onclick={() => deleteTask(task)}
                title={$t('common.delete')}
                aria-label={$t('common.delete')}
              >
                <Icon name="trash" size={12} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    {#if board.routines.length === 0}
      <EmptyState
        icon="check-square"
        title={$t('routines.page.noRoutinesDue')}
        description={$t('routines.page.emptyBoardHint')}
      >
        {#snippet action()}
          <a class="btn primary link" href="/routines/templates">
            {$t('routines.page.goToTemplates')}
          </a>
        {/snippet}
      </EmptyState>
    {/if}

    <div class="groups">
      {#each groups as g (g.category?.id ?? 'none')}
        {@const key = g.category?.id ?? 'none'}
        {@const open = !collapsed.has(key)}
        {@const gdone = g.routines.reduce((n, r) => n + r.items.filter((i) => i.checked).length, 0)}
        {@const gtotal = g.routines.reduce((n, r) => n + r.items.length, 0)}
        <section class="group">
          <h2 class="ghead">
            <button
              class="gtoggle"
              onclick={() => toggleGroup(key)}
              aria-expanded={open}
              aria-controls="group-{key}"
            >
              <Icon name={open ? 'chevron-down' : 'chevron-right'} size={13} />
              <span
                class="dot"
                style:background={g.category?.color || 'var(--muted)'}
                aria-hidden="true"
              ></span>
              {g.category?.name ?? $t('routines.templates.uncategorised')}
              <!-- Collapsed groups still report their progress, so folding one away never
                   hides whether it is finished. -->
              <span class="gcount" class:done={gtotal > 0 && gdone === gtotal}>{gdone}/{gtotal}</span>
            </button>
            <!-- Outside the fold control: ticking a whole phase off must not also collapse it. -->
            {#if gtotal > 0}
              <button
                class="gall"
                class:on={gdone === gtotal}
                disabled={busyGroups.has(key)}
                onclick={() => toggleGroupDone(g, gdone === gtotal)}
                title={gdone === gtotal
                  ? $t('routines.page.uncheckCategoryHint')
                  : $t('routines.page.checkCategoryHint')}
              >
                <Icon name={gdone === gtotal ? 'check-circle' : 'check'} size={13} />
                {gdone === gtotal
                  ? $t('routines.page.uncheckCategory')
                  : $t('routines.page.checkCategory')}
              </button>
            {/if}
          </h2>

          {#if open}
          <div class="glist" id="group-{key}">
          {#each g.routines as r (r.id)}
            {@const done = r.items.filter((i) => i.checked).length}
            <div
              class="routine"
              class:complete={r.items.length > 0 && done === r.items.length}
              style:--cat={g.category?.color || 'var(--border)'}
            >
              <div class="rhead">
                <h3>{r.name}</h3>
                <span class="rsched">{describeSchedule(r, $t)}</span>
                <span class="rcount">{done}/{r.items.length}</span>
                {#if r.items.length > 0}
                  <button
                    class="rall"
                    class:on={done === r.items.length}
                    disabled={busyGroups.has(r.id)}
                    onclick={() => toggleRoutineDone(r, done === r.items.length)}
                    title={done === r.items.length
                      ? $t('routines.page.uncheckCategoryHint')
                      : $t('routines.page.checkCategoryHint')}
                  >
                    <Icon name={done === r.items.length ? 'check-circle' : 'check'} size={12} />
                    {done === r.items.length
                      ? $t('routines.page.uncheckCategory')
                      : $t('routines.page.checkCategory')}
                  </button>
                {/if}
                <a
                  class="edit"
                  href="/routines/templates"
                  title={$t('routines.page.editInTemplates')}
                  aria-label={$t('routines.page.editInTemplates')}
                >
                  <Icon name="pencil" size={13} />
                </a>
              </div>

              {#if r.description}
                <p class="rdesc">{r.description}</p>
              {/if}

              <ul>
                {#each r.items as item (item.id)}
                  <li>
                    <div class="itemrow">
                      <label class:done={item.checked}>
                        <input
                          type="checkbox"
                          checked={item.checked}
                          onchange={() => toggleItem(item)}
                        />
                        <span>{item.label}</span>
                      </label>
                      {#if item.url}
                        <a
                          class="ilink"
                          href={item.url}
                          target="_blank"
                          rel="noopener noreferrer nofollow"
                          title={item.link_label || item.url}
                        >
                          <Icon name="external-link" size={12} />
                          {item.link_label || $t('routines.templates.openLink')}
                        </a>
                      {/if}
                      {#if item.note}
                        <button
                          class="inote"
                          onclick={() => toggleNote(item.id)}
                          aria-expanded={expanded.has(item.id)}
                          aria-controls="note-{item.id}"
                        >
                          <Icon
                            name={expanded.has(item.id) ? 'chevron-down' : 'chevron-right'}
                            size={12}
                          />
                          {$t('routines.page.how')}
                        </button>
                      {/if}
                    </div>
                    {#if item.note && expanded.has(item.id)}
                      <!-- Sanitised server-side at write time (ammonia allowlist). -->
                      <div class="notebody" id="note-{item.id}">{@html item.note}</div>
                    {/if}
                  </li>
                {/each}
              </ul>
            </div>
          {/each}
          </div>
          {/if}
        </section>
      {/each}
    </div>
  {:else if loading}
    <div class="sk-board" aria-busy="true">
      <Skeleton rows={4} height="2.4rem" gap="var(--space-3)" />
    </div>
  {/if}
</div>

<ConsistencyModal
  bind:open={showConsistency}
  marks={board?.marks ?? []}
  setMark={traderApi.setMark}
  title={$t('routines.consistency.title')}
  onchanged={(next) => (board = { ...board, marks: next, mark: next.find((m) => m.day === date)?.mark ?? null })}
/>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }

  .datenav {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  /* The empty-state CTA is a link that has to read as the page's primary button. */
  /* An <a> styled as a button: keep the global .btn layer's inline-flex centring, or the
     label sits at the top of the box with the padding pooled underneath. */
  .btn.link {
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  /* Two fixed columns rather than a flex row: the progress bar keeps the same share of
     the page whatever the counter reads, so the period picker never slides sideways when
     "0/4" becomes "12/31". */
  .status {
    display: grid;
    grid-template-columns: 58% minmax(0, 1fr);
    align-items: center;
    gap: var(--space-6);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
  }
  @media (max-width: 860px) {
    .status {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-3);
    }
  }
  .progress {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .ptext {
    font-size: var(--text-base);
    color: var(--muted);
  }
  /* The <strong> arrives via {@html} from the translation, so it needs :global to be
     reachable from this component's scoped styles. */
  .ptext :global(strong) {
    color: var(--text);
  }
  .allset {
    color: var(--green);
  }
  .pbar {
    height: 6px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .pfill {
    height: 100%;
    background: var(--green);
    transition: width 0.25s ease;
  }
  /* ── Consistency block ── */
  .consist {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
  }
  .periods {
    display: flex;
    gap: var(--space-1);
    flex: none;
  }

  /* The counter is the way into the calendar, so it reads as a control, not a label.
     Its width is reserved (widest case "365/365"), so growing digits push nothing. */
  .tally {
    display: inline-flex;
    align-items: baseline;
    justify-content: flex-end;
    gap: 3px;
    flex: none;
    min-width: 11rem;
    background: transparent;
    border: none;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-base);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .tally:hover {
    background: var(--surface-2);
  }
  .tally b {
    font-weight: var(--fw-medium);
  }
  .of,
  .tlbl {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .tlbl {
    margin-left: var(--space-1);
  }
  .markbtn {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .markbtn:hover {
    color: var(--text);
  }
  .markbtn.on {
    color: var(--green);
    border-color: var(--green);
  }

  /* ── Quick tasks ── */
  .tasks {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .thead {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .tadd {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 0 var(--space-1) 0 var(--space-3);
    transition: border-color var(--dur-fast) var(--ease);
  }
  .tadd:focus-within {
    border-color: var(--accent);
  }
  /* Borderless inside the row's frame: the form owns the hairline and the focus color. */
  .ttitle-in,
  .tdue-in {
    background: transparent;
    border: none;
    box-shadow: none;
    padding: 0;
    min-width: 0;
  }
  .ttitle-in {
    flex: 1;
  }
  .tdue-in {
    flex: none;
    width: auto;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .ttitle-in:focus,
  .tdue-in:focus {
    box-shadow: none;
  }
  .tsubmit,
  .tdel {
    flex: none;
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    padding: 4px;
    color: var(--muted);
    cursor: pointer;
  }
  .tsubmit:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .tsubmit:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .tdel:hover {
    color: var(--red);
  }

  /* Priority is a dot that cycles low, normal, high on click: one glance, one click,
     no menu for a three-way choice. */
  .pri {
    flex: none;
    width: 10px;
    height: 10px;
    padding: 0;
    border-radius: 50%;
    border: 1.5px solid var(--muted);
    background: transparent;
    cursor: pointer;
  }
  .pri.normal {
    border-color: var(--amber);
    background: color-mix(in srgb, var(--amber) 35%, transparent);
  }
  .pri.high {
    border-color: var(--red);
    background: var(--red);
  }
  .pri:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .tlist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .task {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-1) 0 var(--space-3);
  }
  .task label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-base);
    cursor: pointer;
    padding: 2px 0;
  }
  .task label.done span {
    color: var(--muted);
    text-decoration: line-through;
  }
  .tdue {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .tdue.overdue {
    color: var(--red);
  }

  .groups {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .ghead {
    display: flex;
    align-items: center;
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  /* The whole heading is the fold control — a category title is a big, obvious target,
     and there is nothing else to click on that row. */
  .gtoggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: var(--space-1) var(--space-2) var(--space-1) 0;
    margin-left: calc(var(--space-2) * -1);
    padding-left: var(--space-2);
    border-radius: var(--radius);
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .gtoggle:hover {
    background: var(--surface-2);
  }
  .gcount {
    font-size: var(--text-xs);
    font-weight: 400;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .gcount.done {
    color: var(--green);
  }
  /* Always visible, at both levels: a bulk tick you have to hover to discover is a bulk
     tick nobody uses. Kept as a hairline chip so a row of them still reads quieter than
     the checkboxes underneath. */
  .gall,
  .rall {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex: none;
    background: transparent;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: 400;
    white-space: nowrap;
    cursor: pointer;
  }
  .gall {
    margin-left: var(--space-2);
  }
  .gall:hover:not(:disabled),
  .rall:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-control);
  }
  .gall.on,
  .rall.on {
    color: var(--green);
    border-color: var(--green);
  }
  .gall:disabled,
  .rall:disabled {
    cursor: progress;
  }
  .glist {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }

  .routine {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-left: 2px solid var(--cat);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
  }
  .routine.complete {
    border-color: var(--green);
    border-left-color: var(--green);
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .rhead h3 {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    flex: 1;
    min-width: 0;
  }
  .rsched {
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .rcount {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
  }
  .edit {
    color: var(--muted);
    display: inline-flex;
    padding: 2px;
  }
  .edit:hover {
    color: var(--text);
  }
  .rdesc {
    margin: var(--space-1) 0 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }

  .routine ul {
    list-style: none;
    margin-top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .itemrow {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .routine label {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-base);
    cursor: pointer;
    padding: 2px 0;
  }
  .routine label.done span {
    color: var(--muted);
    text-decoration: line-through;
  }

  .ilink,
  .inote {
    font-size: var(--text-xs);
    color: var(--muted);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    text-decoration: none;
  }
  .ilink:hover,
  .inote:hover {
    color: var(--accent);
  }

  .notebody {
    margin: var(--space-1) 0 var(--space-2) var(--space-6);
    padding-left: var(--space-3);
    border-left: 2px solid var(--border);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .notebody :global(p) {
    margin: 0 0 var(--space-1);
  }
  .notebody :global(p:last-child) {
    margin-bottom: 0;
  }
  .notebody :global(ul),
  .notebody :global(ol) {
    margin: 0;
    padding-left: var(--space-4);
  }
  .notebody :global(a) {
    color: var(--accent);
  }
  @media (max-width: 767px) {
    .consist { justify-content: flex-start; flex-wrap: wrap; gap: var(--space-2); }
    .periods { flex: 1 1 100%; min-width: 0; }
    .periods .chip { flex: 1; min-width: 0; min-height: 44px; }
    .tally { min-width: 0; min-height: 44px; justify-content: flex-start; }
    .markbtn { min-width: 0; min-height: 44px; white-space: normal; }
    .ghead { flex-wrap: wrap; gap: var(--space-1); }
    .gtoggle { min-width: 0; text-align: left; overflow-wrap: anywhere; }
    .gall { margin-left: 0; }
    .gall,
    .rall { min-height: 44px; white-space: normal; }
    .rhead { flex-wrap: wrap; }
    .rhead h3 { flex: 1 1 100%; overflow-wrap: anywhere; }
    .rsched { white-space: normal; }
    .edit { min-width: 44px; min-height: 44px; align-items: center; justify-content: center; margin-left: auto; }
    .routine label { min-height: 44px; align-items: center; overflow-wrap: anywhere; }
    .ilink,
    .inote { min-height: 44px; }
    .ilink { max-width: 100%; overflow-wrap: anywhere; }
    .notebody { margin-left: var(--space-4); overflow-wrap: anywhere; }
    .tadd { flex-wrap: wrap; }
    .ttitle-in { flex: 1 1 60%; min-height: 44px; }
    .task label { min-height: 44px; align-items: center; overflow-wrap: anywhere; }
    .tsubmit,
    .tdel { min-width: 44px; min-height: 44px; justify-content: center; }
    .pri { width: 14px; height: 14px; }
  }

  /* Room under the last card for the floating search and assistant shortcuts. */
  @media (max-width: 767px) {
    .page {
      padding-bottom: calc(80px + env(safe-area-inset-bottom));
    }
  }
</style>
