<script>
  // The guards: the rules as sentences, what they hold now, and every trigger and restoration
  // newest first. A halt, a pause or a scale is cleared by hand with Resume.
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { guards = null, busy = false, onresume = () => {} } = $props();

  const when = (v) => (v && Number.isFinite(Date.parse(v)) ? new Date(v).toLocaleString() : '-');
  const tone = (kind) => (kind === 'halt' ? 'danger' : kind === 'pause' || kind === 'scale' ? 'warn' : 'success');
  const held = $derived(!!guards?.holds || (guards?.scale ?? 1) < 1);
</script>

<div class="guards">
  <section class="now">
    {#if guards?.holds}
      <p class="holds">{guards.holds}</p>
    {:else if (guards?.scale ?? 1) < 1}
      <p class="holds">{$t('robots.guard.scaledNow', { scale: guards.scale })}</p>
    {:else}
      <p class="calm">{$t('robots.guard.calm')}</p>
    {/if}
    {#if held}
      <Button size="sm" variant="primary" loading={busy} onclick={onresume}>{$t('robots.guard.resume')}</Button>
    {/if}
  </section>
  <section>
    <h4>{$t('robots.guard.rules')}</h4>
    {#if guards?.described?.length}
      <ol class="rules">{#each guards.described as d, i (i)}<li>{d}</li>{/each}</ol>
    {:else}
      <p class="calm">{$t('robots.guard.none')}</p>
    {/if}
  </section>
  <section>
    <h4>{$t('robots.guard.timeline')}</h4>
    {#if guards?.timeline?.length}
      <ul class="timeline">
        {#each guards.timeline as e (e.id)}
          <li>
            <Badge tone={tone(e.action?.kind)}>{$t(`robots.guard.action.${e.action?.kind ?? 'restore'}`)}{#if e.action?.scale} x{e.action.scale}{/if}</Badge>
            <span class="what">
              {#if e.action?.until}{$t('robots.guard.until', { at: when(e.action.until) })}{/if}
              {#if e.trigger?.streak}{$t('robots.guard.streak', { n: e.trigger.streak })}{/if}
              {#if e.trigger?.drawdown_pct !== undefined}{$t('robots.guard.drawdown', { pct: e.trigger.drawdown_pct })}{/if}
              {#if e.trigger?.pnl !== undefined}{$t('robots.guard.windowPnl', { pnl: e.trigger.pnl })}{/if}
            </span>
            <time>{when(e.at)}</time>
            {#if e.cleared_at}<small>{$t('robots.guard.cleared', { at: when(e.cleared_at) })}</small>{/if}
          </li>
        {/each}
      </ul>
    {:else}
      <EmptyState icon="shield" title={$t('robots.guard.noEvents')} compact />
    {/if}
  </section>
</div>

<style>
  .guards {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    padding: var(--space-4);
  }
  .now {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .holds {
    margin: 0;
    color: var(--amber);
    font-size: var(--text-sm);
  }
  .calm {
    margin: 0;
    color: var(--dim);
    font-size: var(--text-sm);
  }
  .rules {
    margin: 0;
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
  }
  .rules li::marker {
    font-family: var(--mono);
    color: var(--dim);
  }
  .timeline {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: var(--hairline) solid var(--border);
  }
  .timeline li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
    min-height: var(--row-h);
    padding: var(--space-1) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  /* Action tags: square hairline, mono caps, like the robot's other tags. */
  .timeline :global(.badge) {
    border-radius: 0;
    background: transparent;
    padding: 0 var(--space-1);
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
  }
  .what {
    flex: 1;
    min-width: 0;
    font-size: var(--text-sm);
  }
  time {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  small {
    font-size: var(--fs-desc);
    color: var(--dim);
  }
</style>
