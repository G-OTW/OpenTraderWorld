<script>
  // First-run invite on the dashboard: the three things that turn an empty install into a
  // working one. Each step reads as done from real state (a ready connector, a broker
  // account, a workspace the user changed), never from a click. Shown while a step is open
  // and the user has not dismissed it; the dismissal is a user pref, so it follows them.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ConnectorsModal from '$lib/connectors/ConnectorsModal.svelte';
  import BrokersModal from '$lib/brokers/BrokersModal.svelte';
  import { connectorsApi, isReady } from '$lib/connectors/api.js';
  import { brokersApi } from '$lib/brokers/api.js';
  import { pref } from '$lib/prefs.svelte.js';
  import { t } from '$lib/i18n';
  import { untrack } from 'svelte';

  // `customized`: the workspace differs from what shipped. `oncustomize` enters edit mode.
  let { customized = false, oncustomize } = $props();

  const dismissed = pref('otw.dashboard.welcome.dismissed.v1', false);
  // The shipped layout can be the one the user wants: Done closes the step without editing.
  const keptLayout = pref('otw.dashboard.welcome.keptLayout.v1', false);

  // null until read: a step is not drawn as open before its state is known.
  let hasConnector = $state(null);
  let hasBroker = $state(null);
  let connectorsOpen = $state(false);
  let brokersOpen = $state(false);

  // A list that cannot be read leaves its step open: the invite is a nudge, not a gate.
  async function refreshConnectors() {
    hasConnector = await connectorsApi
      .list()
      .then((list) => list.some(isReady))
      .catch(() => false);
  }
  async function refreshBrokers() {
    hasBroker = await brokersApi
      .list()
      .then((list) => list.length > 0)
      .catch(() => false);
  }

  // Read on mount, and again each time a modal closes, whatever happened inside it.
  $effect(() => {
    if (!connectorsOpen) untrack(refreshConnectors);
  });
  $effect(() => {
    if (!brokersOpen) untrack(refreshBrokers);
  });

  const steps = $derived([
    { id: 'connector', icon: 'plug', done: hasConnector, run: () => (connectorsOpen = true) },
    { id: 'broker', icon: 'briefcase', done: hasBroker, run: () => (brokersOpen = true) },
    {
      id: 'customize',
      icon: 'settings',
      done: customized || keptLayout.value,
      run: () => oncustomize?.(),
      skip: () => (keptLayout.value = true)
    }
  ]);
  const known = $derived(hasConnector !== null && hasBroker !== null);
  const show = $derived(known && !dismissed.value && steps.some((s) => !s.done));

  // Once every step is done the guide retires for good: a broker removed later does not
  // bring it back.
  $effect(() => {
    if (known && steps.every((s) => s.done) && !dismissed.value) dismissed.value = true;
  });
</script>

{#if show}
  <section class="card welcome" aria-labelledby="welcome-title">
    <div class="head">
      <h3 id="welcome-title">{$t('dashboard.welcome.title')}</h3>
      <Button size="sm" variant="ghost" icon="x" onclick={() => (dismissed.value = true)}>
        {$t('dashboard.welcome.dismiss')}
      </Button>
    </div>
    <p class="lead">{$t('dashboard.welcome.lead')}</p>
    <ol class="steps">
      {#each steps as s (s.id)}
        <li class:done={s.done}>
          <span class="mark"><Icon name={s.done ? 'check' : s.icon} size={15} /></span>
          <div class="text">
            <span class="name">{$t(`dashboard.welcome.${s.id}`)}</span>
            <span class="hint">{$t(`dashboard.welcome.${s.id}.hint`)}</span>
          </div>
          {#if s.done}
            <span class="state">{$t('dashboard.welcome.done')}</span>
          {:else}
            <div class="actions">
              <Button size="sm" onclick={s.run}>{$t(`dashboard.welcome.${s.id}.action`)}</Button>
              {#if s.skip}
                <Button size="sm" variant="ghost" icon="check" onclick={s.skip}>
                  {$t('dashboard.welcome.done')}
                </Button>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
  </section>
{/if}

<ConnectorsModal bind:open={connectorsOpen} />
<BrokersModal bind:open={brokersOpen} />

<style>
  .welcome {
    flex: none;
    margin-bottom: var(--space-4);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .head h3 {
    margin: 0;
  }
  .lead {
    margin: var(--space-1) 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .steps li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .mark {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }
  .done .mark {
    color: var(--green);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .done .name {
    color: var(--muted);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .state {
    flex: none;
    color: var(--green);
    font-size: var(--text-xs);
  }
  @media (max-width: 720px) {
    .steps {
      grid-template-columns: 1fr;
    }
  }
</style>
