<script>
  // A robot's state at a glance, the same everywhere it is shown: the status dot, where its
  // orders go (simulated, paper, testnet, live), what its guards hold, whether it is armed.
  import Badge from '$lib/ui/Badge.svelte';
  import { t } from '$lib/i18n';
  import { envTone, robotState, stateTone } from './fleet.js';

  let { robot } = $props();

  const state = $derived(robotState(robot));
  const guardTone = $derived(robot?.guard === 'halt' ? 'danger' : 'warn');
</script>

<span class="badges">
  <!-- Dot and word read as one unit: the state. -->
  <span class="state" class:on={stateTone(state) === 'ok'}>
    <span class="dot {stateTone(state)}" title={$t(`robots.state.${state}`)}></span>
    <span class="word">{$t(`robots.state.${state}`)}</span>
  </span>
  <!-- Simulated is the default and says nothing: the tag only marks where real orders go. -->
  {#if robot?.environment && robot.environment !== 'simulated'}
    <Badge tone={envTone(robot.environment)}>{$t(`robots.env.${robot.environment}`)}</Badge>
  {/if}
  {#if robot?.guard}
    <Badge tone={guardTone} icon="shield" title={robot.guard_holds ?? ''}>{robot.guard === 'halt' || robot.guard === 'pause' ? $t(`robots.guard.badge.${robot.guard}`) : robot.guard}</Badge>
  {/if}
  {#if robot?.environment && robot.environment !== 'simulated'}
    <Badge tone={robot.armed ? 'success' : 'neutral'}>{robot.armed ? $t('robots.armed') : $t('robots.disarmed')}</Badge>
  {/if}
</span>

<style>
  .badges {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-2);
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .dot.ok {
    background: var(--green);
  }
  .dot.warn {
    background: var(--amber);
  }
  .dot.bad {
    background: var(--red);
  }
  .word {
    font-size: var(--fs-body);
    font-weight: var(--fw-normal);
    color: var(--muted);
  }
  .state.on .word {
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  /* Square hairline tags in mono: environment, guard and arming are codes, not prose.
     The accent belongs to orientation, so the paper tag reads in neutral ink. */
  .badges :global(.badge) {
    border-radius: 0;
    background: transparent;
    padding: 0 var(--space-1);
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
  }
  .badges :global(.badge.accent) {
    color: var(--muted);
    border-color: var(--border-control);
  }
</style>
