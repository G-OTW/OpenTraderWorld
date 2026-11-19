<script>
  // When the robot takes no new entry: whole weekdays, hour ranges, date ranges, any mix,
  // read in the robot's timezone. Exits are still managed inside one; "leave the market"
  // also closes what the robot holds when one starts. The server checks every range on save.
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { WEEKDAYS } from '$lib/ui/recurrence.js';
  import { t } from '$lib/i18n';

  let { exclusions = $bindable({}) } = $props();

  const ex = $derived(exclusions ?? {});

  function set(patch) {
    exclusions = { ...ex, ...patch };
  }
  function toggleDay(bit) {
    set({ weekdays: (ex.weekdays ?? 0) ^ bit });
  }
  function addRange(key, from, to) {
    set({ [key]: [...(ex[key] ?? []), { from, to }] });
  }
  function editRange(key, i, field, value) {
    set({ [key]: (ex[key] ?? []).map((r, j) => (j === i ? { ...r, [field]: value } : r)) });
  }
  function removeRange(key, i) {
    set({ [key]: (ex[key] ?? []).filter((_, j) => j !== i) });
  }
  const today = () => new Date().toISOString().slice(0, 10);
</script>

<div class="ex">
  <div class="part">
    <span class="lbl">{$t('robots.exclusions.days')}</span>
    <div class="daybtns" role="group" aria-label={$t('robots.exclusions.days')}>
      {#each WEEKDAYS as d (d.bit)}
        <button
          type="button"
          class:on={((ex.weekdays ?? 0) & d.bit) !== 0}
          aria-pressed={((ex.weekdays ?? 0) & d.bit) !== 0}
          onclick={() => toggleDay(d.bit)}
        >
          {$t(`routines.weekdayShort.${d.key}`)}
        </button>
      {/each}
    </div>
  </div>

  {#each [['hours', 'time', '15:30', '16:00'], ['dates', 'date', today(), today()]] as [key, type, from, to] (key)}
    <div class="part">
      <span class="lbl">{$t(`robots.exclusions.${key}`)}</span>
      {#each ex[key] ?? [] as r, i (i)}
        <div class="row">
          <Input {type} value={r.from} aria-label={$t('robots.exclusions.from')} oninput={(e) => editRange(key, i, 'from', e.currentTarget.value)} />
          <span class="to">{$t('robots.exclusions.to')}</span>
          <Input {type} value={r.to} aria-label={$t('robots.exclusions.to')} oninput={(e) => editRange(key, i, 'to', e.currentTarget.value)} />
          <button class="rm" type="button" title={$t('common.remove')} aria-label={$t('common.remove')} onclick={() => removeRange(key, i)}><Icon name="x" size={13} /></button>
        </div>
      {/each}
      <div><Button size="sm" icon="plus" onclick={() => addRange(key, from, to)}>{$t(`robots.exclusions.add.${key}`)}</Button></div>
    </div>
  {/each}

  <label class="check">
    <input type="checkbox" checked={!!ex.leave_market} onchange={(e) => set({ leave_market: e.currentTarget.checked })} />
    <span>
      {$t('robots.exclusions.leaveMarket')}
      <em>{$t('robots.exclusions.leaveMarketHint')}</em>
    </span>
  </label>
</div>

<style>
  .ex {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .part {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .lbl {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .daybtns {
    display: flex;
    gap: var(--space-1);
    max-width: 320px;
  }
  .daybtns button {
    flex: 1 1 0;
    min-width: 0;
    height: 32px;
    border-radius: var(--radius);
    border: 0.5px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .daybtns button:hover {
    color: var(--text);
  }
  .daybtns button.on {
    color: var(--text);
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .to {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .rm {
    border: 0;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    display: inline-flex;
    padding: var(--space-1);
  }
  .rm:hover {
    color: var(--red);
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    font-size: var(--text-sm);
  }
  .check em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  @media (max-width: 640px) {
    .daybtns button {
      min-height: 44px;
    }
  }
</style>
