<script>
  // Calendar widget: a compact scrollable list of events from today through the next 7 days,
  // grouped by day. Config: { limit } caps the number of events shown.
  import { calendarApi } from '$lib/modules/calendar/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();
  const limit = $derived(item.config?.limit ?? 12);

  let events = $state(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      const start = new Date();
      start.setHours(0, 0, 0, 0);
      const end = new Date(start);
      end.setDate(end.getDate() + 7);
      const list = await calendarApi.list(start.toISOString(), end.toISOString());
      events = list
        .slice()
        .sort((a, b) => new Date(a.start_at) - new Date(b.start_at))
        .slice(0, limit);
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  function dayLabel(iso) {
    const d = new Date(iso);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const dd = new Date(d);
    dd.setHours(0, 0, 0, 0);
    const diff = Math.round((dd - today) / 86400000);
    if (diff === 0) return $t('dashboard.widgets.calendar.today');
    if (diff === 1) return $t('dashboard.widgets.calendar.tomorrow');
    return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
  }
  function timeLabel(e) {
    if (e.all_day) return $t('dashboard.widgets.calendar.allDay');
    return new Date(e.start_at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }

  // Group consecutive events by day label.
  const groups = $derived.by(() => {
    const out = [];
    for (const e of events ?? []) {
      const lbl = dayLabel(e.start_at);
      let g = out[out.length - 1];
      if (!g || g.label !== lbl) out.push((g = { label: lbl, items: [] }));
      g.items.push(e);
    }
    return out;
  });
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.calendar.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if events === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else if events.length === 0}
  <p class="hint">{$t('dashboard.widgets.calendar.empty')}</p>
{:else}
  <div class="groups">
    {#each groups as g (g.label)}
      <div class="group">
        <div class="glabel">{g.label}</div>
        {#each g.items as e (e.id)}
          <div class="ev">
            <span class="dot" style:background={e.color || 'var(--accent)'}></span>
            <span class="time">{timeLabel(e)}</span>
            <span class="title">{e.title}</span>
          </div>
        {/each}
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
    color: var(--muted);
  }
  .groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .glabel {
    font-size: var(--text-xs);
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    margin-bottom: 3px;
  }
  .ev {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    padding: 2px 0;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .time {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
    min-width: 52px;
  }
  .title {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
