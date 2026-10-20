<script>
  // Put existing routines on this calendar. Nothing is created here: routines are
  // written in the Trading routines module, and this dialog only says which ones this
  // book runs, and over which period. Their own recurrence still decides which days
  // inside that period are actually due.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { journalApi } from './api.js';
  import { traderApi, describeSchedule } from '$lib/modules/routines/api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    categoryId = '',
    categoryName = '',
    onchanged = () => {}
  } = $props();

  let library = $state([]); // every routine of the Trading routines module
  let links = $state([]); // the ones on this calendar
  let loading = $state(false);
  let error = $state('');

  // The picker is a list inside the dialog, not a popup: the modal is its own scroller,
  // so a menu drawn under the field is clipped by it. Inline it also takes several
  // routines in one gesture, and the period below applies to the whole batch.
  let staged = $state([]); // routine ids
  let q = $state('');
  let startDate = $state('');
  let endDate = $state('');
  let busy = $state(false);

  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});

  const todayIso = () => {
    const d = new Date();
    const p = (n) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  };

  $effect(() => {
    if (!open || !categoryId) return;
    staged = [];
    q = '';
    startDate = todayIso();
    endDate = '';
    load();
  });

  async function load() {
    loading = true;
    try {
      const [lib, ls] = await Promise.all([
        traderApi.listRoutines(),
        journalApi.routineLinks(categoryId)
      ]);
      // The module answers [{ routine, items }]; only the routine matters here.
      library = (lib?.routines ?? []).map((r) => r.routine);
      links = ls ?? [];
      error = '';
    } catch (e) {
      error = e?.message ?? 'failed to load routines';
    } finally {
      loading = false;
    }
  }

  // Accent- and case-insensitive contains, so "prefere" finds "Préféré".
  const norm = (v) =>
    (v ?? '')
      .toString()
      .normalize('NFD')
      .replace(/\p{Diacritic}/gu, '')
      .toLowerCase();

  // Already on the calendar: offering it again would only let the user overwrite a
  // period they cannot see from here.
  const available = $derived(library.filter((r) => !links.some((l) => l.routine_id === r.id)));
  const shown = $derived.by(() => {
    const needle = norm(q).trim();
    if (!needle) return available;
    return available.filter(
      (r) => norm(r.name).includes(needle) || norm(describeSchedule(r, $t)).includes(needle)
    );
  });

  const toggle = (id) =>
    (staged = staged.includes(id) ? staged.filter((x) => x !== id) : [...staged, id]);

  async function attach() {
    if (staged.length === 0 || !startDate) return;
    busy = true;
    try {
      links = await journalApi.attachRoutines({
        category_id: categoryId,
        routine_ids: staged,
        start_date: startDate,
        end_date: endDate || null
      });
      staged = [];
      q = '';
      error = '';
      onchanged();
    } catch (e) {
      error = e?.message ?? 'failed to add the routines';
    } finally {
      busy = false;
    }
  }

  /** A period edited in place on an attached row. */
  async function setPeriod(link, field, value) {
    const patch = field === 'start' ? { start_date: value } : { end_date: value || null };
    try {
      await journalApi.updateRoutineLink(link.id, patch);
      links = links.map((l) =>
        l.id === link.id
          ? { ...l, ...(field === 'start' ? { start_date: value } : { end_date: value || null }) }
          : l
      );
      error = '';
      onchanged();
    } catch (e) {
      error = e?.message ?? 'failed to move the period';
      await load();
    }
  }

  function detach(link) {
    confirmMessage = $t('journal.routines.confirmDetach', { name: link.name });
    onConfirmYes = async () => {
      await journalApi.detachRoutine(link.id);
      await load();
      onchanged();
    };
    confirmOpen = true;
  }
</script>

<Modal bind:open title={$t('journal.routines.title')} size="md">
  <div class="routines">
    <p class="scope">{categoryName}</p>
    {#if error}<p class="err">{error}</p>{/if}

    <!-- Add: tick the routines, then one period for the batch. -->
    <section class="add">
      <h3>{$t('journal.routines.addToCalendar')}</h3>
      {#if library.length === 0}
        <p class="muted">{$t('journal.routines.libraryEmptyHint')}</p>
      {:else if available.length === 0}
        <p class="muted">{$t('journal.routines.allAttached')}</p>
      {:else}
        <div class="picker">
          <div class="search">
            <Icon name="search" size={14} />
            <input
              type="search"
              bind:value={q}
              placeholder={$t('journal.routines.search')}
              aria-label={$t('journal.routines.search')}
            />
          </div>
          <div class="picklist">
            {#each shown as r (r.id)}
              <label class="prow">
                <input
                  type="checkbox"
                  checked={staged.includes(r.id)}
                  onchange={() => toggle(r.id)}
                />
                <span class="pname">{r.name}</span>
                <span class="prec">{describeSchedule(r, $t)}</span>
              </label>
            {:else}
              <p class="muted pempty">{$t('journal.routines.noMatch')}</p>
            {/each}
          </div>
        </div>
        <div class="dates">
          <label class="fld">
            <span>{$t('journal.routines.from')}</span>
            <input type="date" bind:value={startDate} />
          </label>
          <label class="fld">
            <span>{$t('journal.routines.to')}</span>
            <input type="date" bind:value={endDate} />
          </label>
        </div>
        <p class="hint">{$t('journal.routines.toHint')}</p>
        <div class="add-actions">
          <Button
            variant="primary"
            disabled={busy || staged.length === 0 || !startDate}
            onclick={attach}
          >
            {$t('journal.routines.addBtn', { count: staged.length })}
          </Button>
        </div>
      {/if}
    </section>

    <!-- On this calendar: the period is edited where it is read. -->
    <section class="on">
      <h3>{$t('journal.routines.attached')}</h3>
      {#if loading}
        <p class="muted">{$t('common.loading')}</p>
      {:else if links.length === 0}
        <p class="muted">{$t('journal.routines.noneAttached')}</p>
      {:else}
        <ul class="list">
          {#each links as l (l.id)}
            <li title={l.description ?? ''}>
              <span class="lname">
                {l.name}
                <span class="lrec">{describeSchedule(l, $t)}</span>
              </span>
              <input
                class="d"
                type="date"
                value={l.start_date}
                onchange={(e) => setPeriod(l, 'start', e.currentTarget.value)}
              />
              <input
                class="d"
                type="date"
                value={l.end_date ?? ''}
                onchange={(e) => setPeriod(l, 'end', e.currentTarget.value)}
              />
              <button class="icon" onclick={() => detach(l)} title={$t('journal.routines.detach')}>
                <Icon name="trash" size={13} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  </div>
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  message={confirmMessage}
  danger
  onconfirm={() => onConfirmYes()}
/>

<style>
  .routines {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .scope {
    font-size: var(--text-sm);
    color: var(--muted);
    margin: 0;
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
    margin: 0;
  }
  .muted,
  .hint {
    font-size: var(--text-sm);
    color: var(--muted);
    margin: 0;
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  h3 {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    font-weight: var(--fw-normal);
    margin: 0 0 var(--space-2);
  }
  .add,
  .on {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  /* One frame around the search + the list: the field inside is borderless, the box
     owns the hairline. */
  .picker {
    border: 0.5px solid var(--border-control);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-2);
    height: var(--control-h);
    border-bottom: 0.5px solid var(--border);
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    height: 100%;
  }
  .picklist {
    max-height: 220px;
    overflow-y: auto;
  }
  .prow {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-2);
    cursor: pointer;
  }
  .prow:hover {
    background: var(--surface-2);
  }
  .pname {
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prec {
    font-size: var(--text-xs);
    color: var(--dim);
    white-space: nowrap;
  }
  .pempty {
    padding: var(--space-2);
  }
  .dates {
    display: flex;
    gap: var(--space-2);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }
  .fld span {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .add-actions {
    display: flex;
    justify-content: flex-end;
  }
  /* The attached list scrolls rather than pushing the dialog past its limits. */
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
    max-height: 260px;
    overflow-y: auto;
  }
  /* Same metrics as the picker rows above: one list of routines, read twice. */
  .list li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto auto;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-2);
    background: var(--surface);
  }
  .lname {
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow: hidden;
    font-size: var(--text-sm);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .lrec {
    font-size: var(--text-xs);
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .d {
    width: 9.5rem;
    font-size: var(--text-sm);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    color: var(--red);
  }
</style>
