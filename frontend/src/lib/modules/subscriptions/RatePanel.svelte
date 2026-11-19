<script>
  // Quarterly usage rating: every running subscription whose latest rating is missing or
  // older than 90 days, each with a 1 to 5 choice. A rating is saved on click.
  import { untrack } from 'svelte';
  import { subsApi } from './api.js';
  import { needsRating } from './calc.js';
  import { t } from '$lib/i18n';

  let { subs = [], onrated = () => {}, onclose = () => {} } = $props();

  // Frozen when the panel opens, so a rated row stays visible with its score.
  const queue = untrack(() => subs.filter((s) => needsRating(s)));
  let given = $state({});
  let error = $state('');

  async function rate(s, score) {
    error = '';
    try {
      await subsApi.rate(s.id, score);
      given = { ...given, [s.id]: score };
      onrated();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<div class="rate">
  <p class="hint">{$t('subscriptions.rate.hint')}</p>
  {#if queue.length === 0}
    <p class="hint">{$t('subscriptions.rate.allDone')}</p>
  {/if}
  <ul>
    {#each queue as s (s.id)}
      <li>
        <span class="name">{s.name}</span>
        <div class="scores" role="group" aria-label={$t('subscriptions.rate.scoreFor', { name: s.name })}>
          {#each [1, 2, 3, 4, 5] as n}
            <button class:on={given[s.id] === n} aria-pressed={given[s.id] === n} onclick={() => rate(s, n)}>{n}</button>
          {/each}
        </div>
      </li>
    {/each}
  </ul>
  <p class="legend">{$t('subscriptions.rate.legend')}</p>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="actions">
    <button class="primary" onclick={onclose}>{$t('common.close')}</button>
  </div>
</div>

<style>
  .rate {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 44px;
    padding: var(--space-1) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  li:last-child {
    border-bottom: none;
  }
  .name {
    font-size: 13px;
    font-weight: 600;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  /* Segmented 1 to 5: the tinted track is the frame, the picked score fills with accent. */
  .scores {
    display: inline-flex;
    flex: none;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
  }
  .scores button {
    min-width: 30px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 12px;
    cursor: pointer;
    transition:
      background-color 160ms ease,
      color 160ms ease;
  }
  .scores button:hover {
    color: var(--text);
  }
  .scores button.on {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .hint,
  .legend {
    color: var(--dim);
    font-size: 12px;
  }
  .error {
    color: var(--red);
    font-size: 12px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  @media (max-width: 767px) {
    .scores button {
      min-width: 44px;
      min-height: 44px;
    }
    li {
      flex-wrap: wrap;
    }
  }
</style>
