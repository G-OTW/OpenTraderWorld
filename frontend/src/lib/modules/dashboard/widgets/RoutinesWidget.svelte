<script>
  // Trading routine widget: today's due routine checklists, tickable inline. Uses the
  // routines board endpoint (which already scopes to today) and per-item check calls.
  import { traderApi } from '$lib/modules/routines/api.js';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();

  // Read the day at call time, not at module init. A dashboard left open across midnight
  // would otherwise load yesterday's board — and, worse, `toggle()` would record the tick
  // against yesterday. dateKey() builds the key from local parts, so it does not lean on
  // 'en-CA' happening to order its parts as ISO does.
  let routines = $state(null);
  let err = $state('');
  let busy = $state(new Set());

  async function load() {
    err = '';
    try {
      const board = await traderApi.board(dateKey());
      routines = board.routines ?? [];
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  async function toggle(r, it) {
    if (busy.has(it.id)) return;
    busy = new Set(busy).add(it.id);
    try {
      await traderApi.checkItem(it.id, dateKey(), !it.checked);
      routines = routines.map((x) =>
        x.id !== r.id ? x : { ...x, items: x.items.map((i) => (i.id === it.id ? { ...i, checked: !i.checked } : i)) }
      );
    } catch (e) {
      err = e.message;
    } finally {
      const n = new Set(busy);
      n.delete(it.id);
      busy = n;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.routines.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if routines === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else if routines.length === 0}
  <p class="hint">{$t('dashboard.widgets.routines.empty')}</p>
{:else}
  <div class="routines">
    {#each routines as r (r.id)}
      {@const done = r.items.filter((i) => i.checked).length}
      <div class="routine">
        <div class="rhead">
          <span class="rname">{r.name}</span>
          <span class="rcount">{done}/{r.items.length}</span>
        </div>
        <ul>
          {#each r.items as it (it.id)}
            <li>
              <label class:done={it.checked}>
                <input type="checkbox" checked={it.checked} disabled={busy.has(it.id)} onchange={() => toggle(r, it)} />
                <span>{it.label}</span>
              </label>
            </li>
          {/each}
        </ul>
      </div>
    {/each}
  </div>
{/if}

<style>
  .sk {
    padding: var(--space-1) 0;
  }
  /* Preview, loading and empty text — not an error. This was grouped with a
     now-removed .err rule and inherited its red. */
  .hint {
    color: var(--dim);
  }
  .routines {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .rhead {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-2);
    margin-bottom: var(--space-1, 4px);
  }
  .rname {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
  }
  .rcount {
    font-size: var(--text-xs);
    color: var(--dim);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 3px;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  label.done span {
    color: var(--muted);
    text-decoration: line-through;
  }
</style>
