<script>
  // Settings → Market sessions: named trading windows in a venue's timezone. Three come with
  // the install (Asia/Pacific, Europe, US); each can be edited or deleted and more added.
  // Times are the venue's local wall clock, so a session follows daylight saving on its own.
  // Chart alerts that fire "once per session" pick one of these.
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { settingsApi } from './api.js';
  import { t } from '$lib/i18n';

  const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']; // Monday = bit 0

  let sessions = $state([]);
  let timezones = $state([]);
  let error = $state('');
  let busy = $state(false);
  /** Row being edited: an id, 'new', or null. */
  let editing = $state(null);
  let draft = $state(blank());

  function blank() {
    return { name: '', timezone: 'UTC', start: '09:00', end: '17:00', weekdays: 31 };
  }

  const toHm = (m) =>
    `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
  const toMin = (hm) => {
    const [h, m] = String(hm || '0:0').split(':').map(Number);
    return (h || 0) * 60 + (m || 0);
  };

  async function load() {
    try {
      const r = await settingsApi.marketSessions();
      sessions = r.sessions;
      timezones = r.timezones;
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  load();

  function edit(s) {
    editing = s.id;
    draft = {
      name: s.name,
      timezone: s.timezone,
      start: toHm(s.start_minute),
      end: toHm(s.end_minute),
      weekdays: s.weekdays
    };
  }

  function add() {
    editing = 'new';
    draft = blank();
  }

  async function save() {
    busy = true;
    const body = {
      name: draft.name,
      timezone: draft.timezone,
      start_minute: toMin(draft.start),
      end_minute: toMin(draft.end),
      weekdays: draft.weekdays
    };
    try {
      if (editing === 'new') await settingsApi.createMarketSession(body);
      else await settingsApi.saveMarketSession(editing, body);
      editing = null;
      await load();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function remove(s) {
    try {
      await settingsApi.deleteMarketSession(s.id);
      if (editing === s.id) editing = null;
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  const toggleDay = (i) => (draft.weekdays ^= 1 << i);
  const days = (mask) =>
    DAYS.filter((_, i) => mask & (1 << i))
      .map((d) => $t(`common.weekday.${d}`))
      .join(' ');
  const overnight = $derived(toMin(draft.end) <= toMin(draft.start));
  const tzOptions = $derived(timezones.map((z) => ({ value: z, label: z })));
</script>

<div class="section">
  <div class="head">
    <h2>{$t('sessions.title')}</h2>
    <Button size="sm" icon="plus" onclick={add} disabled={editing === 'new'}>
      {$t('sessions.add')}
    </Button>
  </div>
  <p class="muted small">{$t('sessions.subtitle')}</p>

  <ErrorText {error} />

  {#snippet form()}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <label class="f wide">
        {$t('sessions.name')}
        <input bind:value={draft.name} required />
      </label>
      <div class="f wide">
        <span>{$t('sessions.timezone')}</span>
        <Dropdown bind:value={draft.timezone} options={tzOptions} ariaLabel={$t('sessions.timezone')} />
      </div>
      <label class="f">
        {$t('sessions.start')}
        <input type="time" bind:value={draft.start} required />
      </label>
      <label class="f">
        {$t('sessions.end')}
        <input type="time" bind:value={draft.end} required />
      </label>
      <div class="f">
        <span>{$t('sessions.days')}</span>
        <div class="days">
          {#each DAYS as d, i (d)}
            <button
              type="button"
              class="day"
              class:on={draft.weekdays & (1 << i)}
              onclick={() => toggleDay(i)}
            >
              {$t(`common.weekday.${d}`)}
            </button>
          {/each}
        </div>
      </div>
      {#if overnight}
        <p class="muted small full">{$t('sessions.overnight')}</p>
      {/if}
      <div class="actions full">
        <Button variant="ghost" size="sm" onclick={() => (editing = null)}>
          {$t('common.cancel')}
        </Button>
        <Button
          variant="primary"
          size="sm"
          type="submit"
          loading={busy}
          disabled={!draft.name.trim() || !draft.weekdays}
        >
          {$t('common.save')}
        </Button>
      </div>
    </form>
  {/snippet}

  {#if editing === 'new'}
    {@render form()}
  {/if}

  <ul class="list">
    {#each sessions as s (s.id)}
      <li>
        {#if editing === s.id}
          {@render form()}
        {:else}
          <span class="name">{s.name}</span>
          <span class="muted">
            {$t('sessions.range', { start: toHm(s.start_minute), end: toHm(s.end_minute) })}
          </span>
          <span class="muted">{s.timezone}</span>
          <span class="muted">{days(s.weekdays)}</span>
          <span class="grow"></span>
          <Button variant="ghost" size="sm" icon="pencil" onclick={() => edit(s)}>
            {$t('common.edit')}
          </Button>
          <Button variant="danger" size="sm" icon="trash-2" onclick={() => remove(s)}>
            {$t('common.delete')}
          </Button>
        {/if}
      </li>
    {:else}
      <li class="muted">{$t('sessions.empty')}</li>
    {/each}
  </ul>
</div>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
  }
  .name {
    font-weight: var(--fw-medium);
    min-width: 120px;
  }
  .grow {
    flex: 1;
  }
  .form {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    align-items: flex-end;
    width: 100%;
    padding: var(--space-3);
    background: var(--surface-2);
    border-radius: var(--radius);
  }
  .f {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .f.wide {
    flex: 1;
    min-width: 180px;
  }
  .full {
    flex-basis: 100%;
    margin: 0;
  }
  .days {
    display: flex;
    align-items: center;
    gap: 2px;
    height: var(--control-h);
  }
  .day {
    padding: 4px 6px;
    font-size: 0.72rem;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .day.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
