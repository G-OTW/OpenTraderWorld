<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Goals module. Cards show name, progress bar (reached-points / total), and deadline.
  // Clicking a card opens a detail modal with the full KPI breakdown and a quick way to
  // tick KPIs done (which advances the progress bar).
  import { onMount } from 'svelte';
  import { goalsApi, progress, fmtDate, daysLeft } from '$lib/modules/goals/api.js';
  import GoalForm from '$lib/modules/goals/GoalForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { t } from '$lib/i18n';

  let goals = $state([]);
  let loading = $state(true);

  let showForm = $state(false);
  let editing = $state(null);
  let detail = $state(null); // the goal shown in the detail modal

  onMount(async () => {
    await reload();
    loading = false;
  });

  async function reload() {
    goals = await goalsApi.list();
    // Keep the open detail modal in sync with reloaded data.
    if (detail) detail = goals.find((g) => g.id === detail.id) ?? null;
  }

  function openAdd() {
    editing = null;
    showForm = true;
  }
  function openEdit(g) {
    editing = g;
    showForm = true;
    detail = null;
  }

  // Returns the saved goal (with id) so the form can pre-link a reminder after create.
  async function save(payload, { keepOpen = false } = {}) {
    let saved;
    if (editing) {
      await goalsApi.update(editing.id, payload);
      saved = { ...editing, ...payload };
    } else {
      saved = await goalsApi.add(payload);
    }
    if (!keepOpen) showForm = false;
    else editing = saved; // promote create → edit so further saves update in place
    await reload();
    return saved;
  }

  async function del(g) {
    if (!confirm($t('goals.confirmDelete', { name: g.name }))) return;
    await goalsApi.remove(g.id);
    detail = null;
    await reload();
  }

  // ── Drag & drop reorder (manual mode only) ──
  let dragId = $state(null);
  let dropId = $state(null);

  function onDragStart(e, g) {
    dragId = g.id;
    e.dataTransfer.effectAllowed = 'move';
  }
  function onDragOver(e, g) {
    if (!dragId || dragId === g.id) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    dropId = g.id;
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

    const list = [...goals];
    const fromIdx = list.findIndex((g) => g.id === from);
    const toIdx = list.findIndex((g) => g.id === target.id);
    if (fromIdx < 0 || toIdx < 0) return;
    const [moved] = list.splice(fromIdx, 1);
    list.splice(toIdx, 0, moved);
    goals = list; // optimistic

    const i = list.findIndex((g) => g.id === from);
    const prev = list[i - 1];
    const next = list[i + 1];
    let pos;
    if (!prev) pos = (next?.position ?? 1) - 1;
    else if (!next) pos = (prev.position ?? 0) + 1;
    else pos = (prev.position + next.position) / 2;

    try {
      await goalsApi.setPosition(from, pos);
    } finally {
      await reload();
    }
  }

  // Toggle a KPI's reached flag from the detail modal and persist.
  async function toggleKpi(g, i) {
    const kpis = g.kpis.map((k, idx) => (idx === i ? { ...k, reached: !k.reached } : k));
    await saveKpis(g, kpis);
  }

  // Increment/decrement a KPI's current value by `delta` and persist.
  // Auto-mark reached once current meets/exceeds target (and unmark if it drops below).
  async function bumpKpi(g, i, delta) {
    const kpis = g.kpis.map((k, idx) => {
      if (idx !== i) return k;
      const current = (Number(k.current) || 0) + delta;
      const target = Number(k.target);
      const reached = Number.isFinite(target) ? current >= target : k.reached;
      return { ...k, current, reached };
    });
    await saveKpis(g, kpis);
  }

  async function saveKpis(g, kpis) {
    await goalsApi.update(g.id, {
      name: g.name,
      deadline: g.deadline,
      details: g.details,
      category: g.category,
      kpis
    });
    await reload();
  }

  const pct = (g) => Math.round(progress(g.kpis) * 100);

  // Goal status: reached (100%), overdue (deadline passed and not done), else open.
  function status(g) {
    if (pct(g) >= 100) return 'reached';
    const d = daysLeft(g.deadline);
    if (d !== null && d < 0) return 'overdue';
    return 'open';
  }

  let filter = $state('all'); // all | open | reached | overdue
  const filters = ['all', 'open', 'reached', 'overdue'];
  let catFilter = $state('all'); // 'all' or a specific category
  // 'manual' (drag-to-reorder, server position) | 'asc'/'desc' (by deadline).
  let sortMode = $state('manual');

  // Sort by deadline; goals with no deadline sort last regardless of direction.
  function byDeadline(a, b) {
    const av = a.deadline ?? '';
    const bv = b.deadline ?? '';
    if (!av && !bv) return 0;
    if (!av) return 1;
    if (!bv) return -1;
    return sortMode === 'asc' ? av.localeCompare(bv) : bv.localeCompare(av);
  }

  // Drag-to-reorder is only meaningful in manual order and when the full,
  // unfiltered list is shown (so positions map 1:1 to the visible cards).
  const canReorder = $derived(sortMode === 'manual' && filter === 'all' && catFilter === 'all');

  // Distinct, sorted category labels present across goals (non-empty).
  const categories = $derived(
    [...new Set(goals.map((g) => (g.category || '').trim()).filter(Boolean))].sort()
  );

  // Status counts respect the active category filter so the numbers match what's shown.
  const counts = $derived.by(() => {
    const c = { all: 0, open: 0, reached: 0, overdue: 0 };
    for (const g of goals) {
      if (catFilter !== 'all' && (g.category || '').trim() !== catFilter) continue;
      c.all++;
      c[status(g)]++;
    }
    return c;
  });

  const shown = $derived.by(() => {
    const list = goals.filter(
      (g) =>
        (filter === 'all' || status(g) === filter) &&
        (catFilter === 'all' || (g.category || '').trim() === catFilter)
    );
    // Manual mode keeps the server-provided position order; otherwise sort by deadline.
    return sortMode === 'manual' ? list : [...list].sort(byDeadline);
  });

  function statusLabel(f) {
    if (f === 'all') return $t('goals.filter.all');
    if (f === 'open') return $t('goals.filter.open');
    if (f === 'reached') return $t('goals.filter.reached');
    return $t('goals.filter.overdue');
  }

  function deadlineTag(g) {
    const d = daysLeft(g.deadline);
    if (d === null) return null;
    if (d < 0) return { txt: $t('goals.deadline.overdue', { days: -d }), cls: 'overdue' };
    if (d === 0) return { txt: $t('goals.deadline.today'), cls: 'today' };
    if (d <= 7) return { txt: $t('goals.deadline.soon', { days: d }), cls: 'soon' };
    return { txt: fmtDate(g.deadline), cls: '' };
  }
</script>

<div class="goals">
  <header class="head">
    <div class="title">
      <h1>{$t('goals.title')}</h1>
      <span class="sub">{$t('goals.count', { count: goals.length, s: goals.length === 1 ? '' : 's' })}</span>
    </div>
    <div class="head-actions">
      <QuickReminderButton title={$t('goals.addReminder')} />
      <button class="primary" onclick={openAdd}><Icon name="plus" size={15} /> {$t('goals.addGoal')}</button>
    </div>
  </header>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if goals.length === 0}
    <div class="empty">{$t('goals.empty')}</div>
  {:else}
    <div class="toolbar">
      <div class="filters">
        {#each filters as f}
          <button class="chip" class:active={filter === f} onclick={() => (filter = f)}>
            {statusLabel(f)}
            <span class="chip-count">{counts[f]}</span>
          </button>
        {/each}
      </div>
      <button
        class="chip sort"
        onclick={() =>
          (sortMode = sortMode === 'manual' ? 'asc' : sortMode === 'asc' ? 'desc' : 'manual')}
        title={$t('goals.cycleOrder')}
      >
        {#if sortMode === 'manual'}{$t('goals.manual')} <Icon name="arrow-up-down" size={12} />{:else}{$t('goals.date')} <Icon name={sortMode === 'asc' ? 'arrow-up' : 'arrow-down'} size={12} />{/if}
      </button>
    </div>

    {#if categories.length > 0}
      <div class="filters cat">
        <button class="chip" class:active={catFilter === 'all'} onclick={() => (catFilter = 'all')}>
          {$t('goals.allCategories')}
        </button>
        {#each categories as c}
          <button class="chip" class:active={catFilter === c} onclick={() => (catFilter = c)}>
            {c}
          </button>
        {/each}
      </div>
    {/if}

    {#if shown.length === 0}
      <div class="empty">{$t('goals.noneForFilter', { filter: statusLabel(filter) })}</div>
    {:else}
    <div class="grid">
      {#each shown as g (g.id)}
        {@const tag = deadlineTag(g)}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
        <article
          class="card {status(g)}"
          class:dragging={dragId === g.id}
          class:drop-target={dropId === g.id}
          onclick={() => (detail = g)}
          tabindex="0"
          draggable={canReorder}
          ondragstart={(e) => onDragStart(e, g)}
          ondragover={(e) => canReorder && onDragOver(e, g)}
          ondrop={(e) => canReorder && onDrop(e, g)}
          ondragend={onDragEnd}
        >
          <header class="card-head">
            <h3>{g.name}</h3>
            <span class="pct">{pct(g)}%</span>
          </header>
          <div class="bar"><div class="fill" style="width:{pct(g)}%"></div></div>
          <footer class="card-foot">
            {#if tag}
              <span class="deadline {tag.cls}">{tag.txt}</span>
            {:else}
              <span class="deadline muted">{$t('goals.noDeadline')}</span>
            {/if}
            <span class="meta">
              {#if g.category}<span class="cat-tag">{g.category}</span>{/if}
              <span class="kpi-count">{$t('goals.metricCount', { count: g.kpis.length, s: g.kpis.length === 1 ? '' : 's' })}</span>
            </span>
          </footer>
        </article>
      {/each}
    </div>
    {/if}
  {/if}
</div>

<!-- Detail modal -->
<Modal open={!!detail} title={detail?.name ?? ''} size="md" onclose={() => (detail = null)}>
  {#if detail}
    {@const tag = deadlineTag(detail)}
    <div class="detail">
      <div class="detail-top">
        <div class="bar big"><div class="fill" style="width:{pct(detail)}%"></div></div>
        <span class="pct big">{pct(detail)}%</span>
      </div>
      {#if tag}<p class="detail-deadline {tag.cls}">⏱ {tag.txt}</p>{/if}

      {#if detail.kpis.length > 0}
        <table class="tbl kpi-table">
          <thead>
            <tr><th>{$t('goals.detail.metric')}</th><th class="num">{$t('goals.detail.now')}</th><th class="num">{$t('goals.detail.target')}</th><th class="num">{$t('goals.detail.pts')}</th><th></th></tr>
          </thead>
          <tbody>
            {#each detail.kpis as k, i}
              <tr class:reached={k.reached}>
                <td>{k.name}</td>
                <td class="num">
                  <div class="stepper">
                    <button class="step" onclick={() => bumpKpi(detail, i, -1)} aria-label={$t('goals.detail.decrement')}>−</button>
                    <span>{k.current}</span>
                    <button class="step" onclick={() => bumpKpi(detail, i, 1)} aria-label={$t('goals.detail.increment')}>+</button>
                  </div>
                </td>
                <td class="num">{k.target}</td>
                <td class="num">{k.points}</td>
                <td class="num">
                  <button class="chk" class:on={k.reached} onclick={() => toggleKpi(detail, i)} aria-label={$t('goals.detail.toggleReached')}>
                    {#if k.reached}<Icon name="check" size={12} />{/if}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="muted">{$t('goals.detail.noMetrics')}</p>
      {/if}

      {#if detail.details}
        <div class="notes">
          <span class="lbl">{$t('goals.detail.details')}</span>
          <p>{detail.details}</p>
        </div>
      {/if}

      <div class="detail-actions">
        <button class="ghost danger" onclick={() => del(detail)}>{$t('goals.delete')}</button>
        <button class="ghost" onclick={() => openEdit(detail)}>{$t('goals.edit')}</button>
      </div>
    </div>
  {/if}
</Modal>

<Modal bind:open={showForm} title={editing ? $t('goals.form.editTitle') : $t('goals.form.newTitle')} size="md">
  <GoalForm initial={editing} {categories} onsubmit={save} oncancel={() => (showForm = false)} />
</Modal>

<style>
  .goals {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }
  h1 {
    font-size: 1.4rem;
    font-weight: 700;
  }
  .sub {
    font-size: 0.8rem;
    color: var(--muted);
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .muted {
    color: var(--muted);
  }
  .empty {
    padding: var(--space-8) var(--space-4);
    text-align: center;
    color: var(--muted);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
  }
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .toolbar .filters {
    margin-bottom: 0;
  }
  .chip-count {
    font-size: 0.72rem;
    font-weight: 700;
    opacity: 0.8;
  }
  /* Category chips: one row, scrolls sideways with no visible scrollbar. */
  .filters.cat {
    margin-top: calc(-1 * var(--space-2));
    flex-wrap: nowrap;
    overflow-x: auto;
    scrollbar-width: none; /* Firefox */
    -ms-overflow-style: none; /* IE/Edge */
  }
  .filters.cat::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  .filters.cat .chip {
    flex: 0 0 auto;
  }
  .meta {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .cat-tag {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 8px;
    font-size: 0.7rem;
    color: var(--muted);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-4);
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    cursor: pointer;
    transition: border-color 0.12s, transform 0.12s;
  }
  .card:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
  }
  .card[draggable='true'] {
    cursor: grab;
  }
  .card.dragging {
    opacity: 0.5;
  }
  .card.drop-target {
    outline: 2px dashed var(--accent);
    outline-offset: -2px;
  }
  /* Status borders: completed goals get a green edge, overdue ones a red edge. */
  .card.reached {
    border-color: var(--green);
    box-shadow: inset 0 0 0 1px var(--green);
  }
  .card.overdue {
    border-color: var(--red);
    box-shadow: inset 0 0 0 1px var(--red);
  }
  .card.reached:hover,
  .card.overdue:hover {
    border-color: var(--accent);
  }
  .card-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .card-head h3 {
    font-size: 0.98rem;
    font-weight: 600;
  }
  .pct {
    font-size: 0.85rem;
    font-weight: 700;
    color: var(--accent);
  }
  .bar {
    height: 8px;
    background: var(--surface-2);
    border-radius: 999px;
    overflow: hidden;
  }
  .bar .fill {
    height: 100%;
    background: var(--green);
    border-radius: 999px;
    transition: width 0.25s;
  }
  .card-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: var(--space-3);
    font-size: 0.76rem;
  }
  .deadline {
    color: var(--muted);
  }
  .deadline.overdue {
    color: var(--red);
  }
  .deadline.today {
    color: var(--amber);
  }
  .deadline.soon {
    color: var(--accent);
  }
  .kpi-count {
    color: var(--muted);
  }

  /* Detail modal */
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .detail-top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .bar.big {
    flex: 1;
    height: 12px;
  }
  .pct.big {
    font-size: 1.1rem;
    font-weight: 700;
    color: var(--accent);
  }
  .detail-deadline {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .detail-deadline.overdue {
    color: var(--red);
  }
  .detail-deadline.today {
    color: var(--amber);
  }
  .kpi-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
  }
  .kpi-table th {
    text-align: left;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--muted);
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }
  .kpi-table td {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .kpi-table .num {
    text-align: right;
  }
  .kpi-table tr.reached td {
    color: var(--muted);
  }
  .stepper {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    justify-content: flex-end;
  }
  .stepper span {
    min-width: 2.5ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .step {
    width: 20px;
    height: 20px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    font-size: 0.9rem;
    line-height: 1;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .step:hover {
    border-color: var(--accent);
  }
  .chk {
    width: 22px;
    height: 22px;
    border-radius: 6px;
    border: 2px solid var(--border);
    background: transparent;
    color: #fff;
    cursor: pointer;
    font-size: 0.75rem;
  }
  .chk.on {
    background: var(--green);
    border-color: var(--green);
  }
  .notes .lbl {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .notes p {
    margin-top: 4px;
    font-size: 0.86rem;
    white-space: pre-wrap;
  }
  .detail-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
