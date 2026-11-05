<script>
  // The board: named boards (tabs), each a set of saved views over hives (grid, honeycomb,
  // lines), in the spirit of an observability dashboard. Only the open board loads and goes
  // live. Values come from the evaluator's last closed bar (the items'
  // cache), so the board costs nothing beyond the scans. A view may go live (while this page
  // is open): its items ride the live hub, at most 100 per board; over that the board stays
  // on bar close and says so. Which views, their size, order and options persist in each
  // view's layout.
  import { untrack } from 'svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import GridView from './GridView.svelte';
  import HoneycombView from './HoneycombView.svelte';
  import LinesView from './LinesView.svelte';
  import { scannerApi } from './api.js';
  import { normalizeLayout, reorder, liveSet, LIVE_CAP, SIZES } from './board.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { liveStream } from '$lib/live/stream.js';
  import { t } from '$lib/i18n';

  let { items = [], hives = [], signals = [], ongo } = $props();

  /** Bar-close values refresh this often while the page is open. */
  const REFRESH_MS = 60_000;
  /** The open board, remembered per browser. */
  const ACTIVE_KEY = 'otw-scanner-board';

  let boardList = $state([]);
  let activeId = $state('');
  let naming = $state(null);
  let deletingBoard = $state(null);
  let views = $state([]);
  let boards = $state({});
  let hitsByHive = $state({});
  let live = $state({});
  let liveDown = $state(0);
  let error = $state('');
  let loaded = $state(false);
  let draft = $state(null);
  let deleting = $state(null);

  const palette = $derived(chartColors());
  const active = $derived(boardList.find((b) => b.id === activeId) ?? null);
  const sorted = $derived(views.filter((v) => v.board_id === activeId).sort((a, b) => a.sort - b.sort));

  function pickBoard(id) {
    activeId = id;
    try {
      localStorage.setItem(ACTIVE_KEY, id);
    } catch {
      // Storage off: the first board opens next time.
    }
    loadBoards();
  }

  async function loadViews() {
    try {
      const [bl, vl] = await Promise.all([scannerApi.boards(), scannerApi.views()]);
      boardList = bl;
      views = vl.map((v) => ({ ...v, layout: normalizeLayout(v.layout) }));
      if (!bl.some((b) => b.id === activeId)) {
        let saved = '';
        try {
          saved = localStorage.getItem(ACTIVE_KEY) ?? '';
        } catch {
          // Storage off: open the first board.
        }
        activeId = bl.some((b) => b.id === saved) ? saved : (bl[0]?.id ?? '');
      }
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      loaded = true;
    }
    await loadBoards();
  }

  async function loadBoards() {
    const ids = [...new Set(sorted.map((v) => v.hive_id).filter(Boolean))];
    const next = {};
    const hitMap = {};
    await Promise.all(
      ids.map(async (id) => {
        try {
          next[id] = await scannerApi.board(id);
          hitMap[id] = await scannerApi.hits({ hive_id: id, limit: 300 });
        } catch (e) {
          error = e.message;
        }
      })
    );
    boards = next;
    hitsByHive = hitMap;
  }

  $effect(() => {
    untrack(loadViews);
    const timer = setInterval(() => untrack(loadBoards), REFRESH_MS);
    return () => clearInterval(timer);
  });

  // ── Persistence ──
  const pending = new Map();
  function persist(v) {
    clearTimeout(pending.get(v.id));
    pending.set(
      v.id,
      setTimeout(async () => {
        pending.delete(v.id);
        try {
          await scannerApi.saveView(v.id, {
            board_id: v.board_id,
            name: v.name,
            kind: v.kind,
            hive_id: v.hive_id,
            color_by: $state.snapshot(v.color_by),
            layout: $state.snapshot(v.layout),
            sort: v.sort
          });
        } catch (e) {
          error = e.message;
        }
      }, 400)
    );
  }

  function setLayout(v, patch) {
    v.layout = { ...v.layout, ...patch };
    persist(v);
  }

  function setMetric(v, metric) {
    v.color_by = { metric };
    persist(v);
  }

  function move(v, dir) {
    const list = sorted;
    const from = list.findIndex((x) => x.id === v.id);
    const next = reorder(list, from, from + dir);
    for (const n of next) {
      const cur = views.find((x) => x.id === n.id);
      if (cur && cur.sort !== n.sort) {
        cur.sort = n.sort;
        persist(cur);
      }
    }
  }

  function newView() {
    draft = { name: '', kind: 'honeycomb', hive_id: hives[0]?.id ?? '', metric: 'change_pct' };
  }

  async function createView() {
    try {
      // The first view on an empty board page makes the board it sits on.
      if (!active) {
        const b = await scannerApi.createBoard({ name: $t('scanner.board.defaultName') });
        activeId = b.id;
      }
      await scannerApi.createView({
        board_id: activeId,
        name: draft.name,
        kind: draft.kind,
        hive_id: draft.hive_id,
        color_by: { metric: draft.metric },
        layout: normalizeLayout({ size: draft.kind === 'grid' ? 'l' : 'm' })
      });
      draft = null;
      await loadViews();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove() {
    try {
      await scannerApi.deleteView(deleting.id);
      deleting = null;
      await loadViews();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Boards ──
  async function saveBoardName({ name }) {
    try {
      if (naming.id) {
        await scannerApi.saveBoard(naming.id, { name, sort: naming.sort });
      } else {
        const b = await scannerApi.createBoard({ name });
        pickBoard(b.id);
      }
      naming = null;
      await loadViews();
    } catch (e) {
      error = e.message;
    }
  }

  async function removeBoard() {
    try {
      await scannerApi.deleteBoard(deletingBoard.id);
      deletingBoard = null;
      activeId = '';
      await loadViews();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Live ──
  const liveInfo = $derived(liveSet(sorted, boards, LIVE_CAP));
  const liveKey = $derived(liveInfo.items.map((i) => i.id).join(','));

  $effect(() => {
    const key = liveKey;
    if (!key) {
      live = {};
      liveDown = 0;
      return;
    }
    const list = untrack(() => liveInfo.items.map((i) => ({ ...i })));
    const down = new Set();
    const apply = (d) => {
      const it = list[d?.i ?? -1];
      if (!it || !Number.isFinite(d.c)) return;
      live = { ...live, [it.id]: { close: d.c, at: d.ts } };
    };
    const s = liveStream({
      url: () => scannerApi.streamsUrl(list),
      events: {
        snapshot: apply,
        bar: apply,
        status: (d) => {
          const it = list[d?.i ?? -1];
          if (!it) return;
          if (d.state === 'stopped') down.add(it.id);
          else if (d.state === 'live') down.delete(it.id);
          liveDown = down.size;
        }
      }
    });
    return () => s.close();
  });

  const metricOptions = $derived([
    { value: 'change_pct', label: $t('scanner.metric.change_pct') },
    { value: 'change_from_open', label: $t('scanner.metric.change_from_open') },
    { value: 'volume_ratio', label: $t('scanner.metric.volume_ratio') },
    { value: 'hit_recency', label: $t('scanner.metric.hit_recency') },
    ...signals.map((s) => ({ value: `signal:${s.id}`, label: $t('scanner.metric.signal', { name: s.name }) }))
  ]);
  const sortOptions = $derived([
    { value: 'ticker', label: $t('scanner.col.item') },
    { value: 'change_pct', label: $t('scanner.metric.change_pct') },
    { value: 'change_from_open', label: $t('scanner.metric.change_from_open') },
    { value: 'volume_ratio', label: $t('scanner.metric.volume_ratio') },
    { value: 'hit_recency', label: $t('scanner.metric.hit_recency') }
  ]);
  const KIND_ICON = { grid: 'list', honeycomb: 'grid', lines: 'trending-up' };
  const hiveName = (id) => hives.find((h) => h.id === id)?.name ?? $t('scanner.board.noHive');
</script>

<div class="wrap">
  <div class="filter-bar">
    <div class="boards" role="tablist" aria-label={$t('scanner.board.boards')}>
      {#each boardList as bd (bd.id)}
        <button class="chip" role="tab" class:active={bd.id === activeId} aria-selected={bd.id === activeId} onclick={() => pickBoard(bd.id)}>{bd.name}</button>
      {/each}
      <button class="icon" title={$t('scanner.board.newBoard')} aria-label={$t('scanner.board.newBoard')} onclick={() => (naming = { name: '' })}><Icon name="plus" size={13} /></button>
      {#if active}
        <button class="icon" title={$t('scanner.board.renameBoard')} aria-label={$t('scanner.board.renameBoard')} onclick={() => (naming = { ...active })}><Icon name="pencil" size={13} /></button>
        <button class="icon" title={$t('scanner.board.deleteBoardTitle')} aria-label={$t('scanner.board.deleteBoardTitle')} onclick={() => (deletingBoard = active)}><Icon name="trash" size={13} /></button>
      {/if}
    </div>
    <Button variant="primary" icon="plus" disabled={!hives.length} onclick={newView}>{$t('scanner.board.newView')}</Button>
    {#if liveInfo.over}
      <span class="note"><Icon name="info" size={12} /> {$t('scanner.board.liveOver', { cap: LIVE_CAP, n: liveInfo.total })}</span>
    {:else if liveInfo.items.length}
      <span class="note live"><span class="pip"></span> {$t('scanner.board.liveOn', { n: liveInfo.items.length })}</span>
      {#if liveDown}<span class="note">{$t('scanner.board.liveDown', { n: liveDown })}</span>{/if}
    {:else}
      <span class="note">{$t('scanner.board.barClose')}</span>
    {/if}
  </div>
  {#if error}<ErrorText {error} />{/if}

  {#if loaded && !sorted.length}
    <EmptyState icon="grid" title={$t('scanner.board.emptyTitle')} description={hives.length ? $t('scanner.board.emptyBody') : $t('scanner.board.emptyNoHive')}>
      {#snippet action()}
        {#if hives.length}
          <Button variant="primary" icon="plus" onclick={newView}>{$t('scanner.board.newView')}</Button>
        {:else}
          <Button icon="layers" onclick={() => ongo?.(items.length ? 'hives' : 'items')}>{items.length ? $t('scanner.board.toHives') : $t('scanner.board.toItems')}</Button>
        {/if}
      {/snippet}
    </EmptyState>
  {/if}

  <div class="board">
    {#each sorted as v, idx (v.id)}
      {@const b = boards[v.hive_id]}
      <section class="card view size-{v.layout.size}">
        <header>
          <Icon name={KIND_ICON[v.kind]} size={13} />
          <h3>{v.name}</h3>
          <span class="hv">{hiveName(v.hive_id)}</span>
          <div class="tools">
            {#if v.kind !== 'grid'}
              <div class="dd">
                <Dropdown float value={v.color_by?.metric ?? 'change_pct'} ariaLabel={$t('scanner.board.colorBy')} onpick={(m) => setMetric(v, m)} options={metricOptions} />
              </div>
            {/if}
            <div class="dd">
              <Dropdown float value={v.layout.sort} ariaLabel={$t('scanner.board.sortBy')} onpick={(s) => setLayout(v, { sort: s })} options={sortOptions} />
            </div>
            <button class="chip" class:active={v.layout.hitsOnly} aria-pressed={v.layout.hitsOnly} onclick={() => setLayout(v, { hitsOnly: !v.layout.hitsOnly })}>{$t('scanner.board.hitsOnly')}</button>
            <button class="chip" class:active={v.layout.live} aria-pressed={v.layout.live} title={$t('scanner.board.liveHint')} onclick={() => setLayout(v, { live: !v.layout.live })}>{$t('scanner.board.live')}</button>
            <span class="sizes" role="group" aria-label={$t('scanner.board.size')}>
              {#each SIZES as s (s)}
                <button class="sz" class:on={v.layout.size === s} aria-pressed={v.layout.size === s} onclick={() => setLayout(v, { size: s })}>{s.toUpperCase()}</button>
              {/each}
            </span>
            <button class="icon" disabled={idx === 0} title={$t('scanner.board.moveUp')} aria-label={$t('scanner.board.moveUp')} onclick={() => move(v, -1)}><Icon name="arrow-up" size={13} /></button>
            <button class="icon" disabled={idx === sorted.length - 1} title={$t('scanner.board.moveDown')} aria-label={$t('scanner.board.moveDown')} onclick={() => move(v, 1)}><Icon name="arrow-down" size={13} /></button>
            <button class="icon" title={$t('scanner.delete')} aria-label={$t('scanner.delete')} onclick={() => (deleting = v)}><Icon name="trash" size={13} /></button>
          </div>
        </header>
        {#if !b}
          <p class="hint pad">{v.hive_id ? $t('scanner.board.loading') : $t('scanner.board.noHive')}</p>
        {:else if v.kind === 'grid'}
          <GridView items={b.items} {hives} {signals} {live} sort={v.layout.sort} hitsOnly={v.layout.hitsOnly} onsort={(s) => setLayout(v, { sort: s })} />
        {:else if v.kind === 'honeycomb'}
          <HoneycombView items={b.items} {hives} {signals} {live} {palette} metric={v.color_by?.metric ?? 'change_pct'} sort={v.layout.sort} size={v.layout.size} hitsOnly={v.layout.hitsOnly} />
        {:else}
          <LinesView items={b.items} {hives} hits={hitsByHive[v.hive_id] ?? []} {live} {palette} sort={v.layout.sort} size={v.layout.size} hitsOnly={v.layout.hitsOnly} />
        {/if}
      </section>
    {/each}
  </div>
</div>

<Modal open={!!draft} title={$t('scanner.board.newView')} onclose={() => (draft = null)}>
  {#if draft}
    <div class="form">
      <Input label={$t('scanner.name')} bind:value={draft.name} required />
      <div class="form-grid">
        <label class="field">
          <span>{$t('scanner.board.kind')}</span>
          <Dropdown
            bind:value={draft.kind}
            float
            options={['honeycomb', 'grid', 'lines'].map((k) => ({ value: k, label: $t(`scanner.view.${k}`) }))}
          />
        </label>
        <label class="field">
          <span>{$t('scanner.alerts.hive')}</span>
          <Dropdown bind:value={draft.hive_id} float options={hives.map((h) => ({ value: h.id, label: h.name }))} />
        </label>
        {#if draft.kind === 'honeycomb'}
          <label class="field">
            <span>{$t('scanner.board.colorBy')}</span>
            <Dropdown bind:value={draft.metric} float options={metricOptions} />
          </label>
        {/if}
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (draft = null)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" disabled={!draft?.name?.trim() || !draft?.hive_id} onclick={createView}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<ConfirmModal
  open={!!deleting}
  title={$t('scanner.board.deleteTitle')}
  message={$t('scanner.board.deleteBody', { name: deleting?.name ?? '' })}
  confirmLabel={$t('scanner.delete')}
  cancelLabel={$t('scanner.cancel')}
  danger
  onconfirm={remove}
  oncancel={() => (deleting = null)}
/>

<PromptModal
  open={!!naming}
  title={naming?.id ? $t('scanner.board.renameBoard') : $t('scanner.board.newBoard')}
  fields={[{ key: 'name', label: $t('scanner.name'), value: naming?.name ?? '', required: true }]}
  confirmLabel={$t('scanner.save')}
  onconfirm={saveBoardName}
  oncancel={() => (naming = null)}
/>

<ConfirmModal
  open={!!deletingBoard}
  title={$t('scanner.board.deleteBoardTitle')}
  message={$t('scanner.board.deleteBoardBody', { name: deletingBoard?.name ?? '' })}
  confirmLabel={$t('scanner.delete')}
  cancelLabel={$t('scanner.cancel')}
  danger
  onconfirm={removeBoard}
  oncancel={() => (deletingBoard = null)}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .note {
    color: var(--dim);
    font-size: var(--text-xs);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .boards {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
    margin-right: var(--space-2);
  }
  .boards .chip,
  .boards .icon {
    height: var(--control-h);
  }
  .pip {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--green);
  }
  .board {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .view {
    padding: 0;
    min-width: 0;
    overflow: hidden;
  }
  .size-s {
    grid-column: span 2;
  }
  .size-m {
    grid-column: span 3;
  }
  .size-l {
    grid-column: span 6;
  }
  @media (max-width: 1100px) {
    .size-s,
    .size-m {
      grid-column: span 3;
    }
  }
  @media (max-width: 767px) {
    .size-s,
    .size-m,
    .size-l {
      grid-column: span 6;
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    flex-wrap: wrap;
    color: var(--dim);
  }
  h3 {
    margin: 0;
    color: var(--text);
  }
  .hv {
    font-size: var(--text-xs);
  }
  .tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
  }
  .tools .dd {
    min-width: 120px;
  }
  /* Every toolbar control shares the dropdown's height. */
  .tools .chip,
  .sizes,
  .icon {
    height: var(--control-h);
  }
  .tools .chip {
    padding: 0 var(--space-3);
  }
  .sizes {
    display: inline-flex;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .sz {
    background: none;
    border: none;
    padding: 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--dim);
    cursor: pointer;
  }
  .sz.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .icon {
    background: none;
    border: none;
    color: var(--dim);
    cursor: pointer;
    width: var(--control-h);
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
  }
  .icon:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .pad {
    padding: var(--space-4);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
</style>
