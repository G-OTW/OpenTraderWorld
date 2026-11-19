<script>
  // `notify` block: where a workflow ends up most of the time.
  //
  // Channels come from the shared notification broker, filtered by what the Automator is
  // granted. There is no channel screen here on purpose: ChannelButton opens the one
  // everything else uses.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import ExprField from './ExprField.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { t } from '$lib/i18n';

  let { config = $bindable({}), sources = [] } = $props();

  let channels = $state([]);
  let loadError = $state('');

  onMount(async () => {
    try {
      channels = await channelsApi.list('automator');
    } catch (e) {
      loadError = e.message;
    }
  });

  const chosen = $derived(new Set(config.channels ?? []));

  function toggle(id) {
    const next = new Set(chosen);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    config.channels = [...next];
  }
</script>

<div class="stack">
  <ExprField label={$t('automator.notify.title')} bind:value={config.title} {sources} />
  <ExprField label={$t('automator.notify.body')} bind:value={config.body} rows={5} {sources} />

  <label class="check">
    <input type="checkbox" checked={config.in_app !== false} onchange={(e) => (config.in_app = e.currentTarget.checked)} />
    {$t('automator.notify.inApp')}
  </label>

  <div class="channels">
    <div class="chead">
      <span class="lbl">{$t('automator.notify.channels')}</span>
      <ChannelButton module="automator" />
    </div>
    {#if loadError}
      <p class="hint">{loadError}</p>
    {:else if !channels.length}
      <p class="hint">{$t('automator.notify.noChannels')}</p>
    {:else}
      <ul>
        {#each channels as c (c.id)}
          <li>
            <label class="check">
              <input type="checkbox" checked={chosen.has(c.id)} onchange={() => toggle(c.id)} />
              <span>
                {c.name}
                <em>{c.kind}</em>
              </span>
            </label>
          </li>
        {/each}
      </ul>
      <p class="hint">{$t('automator.notify.channelsHint')}</p>
    {/if}
  </div>
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .chead {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-2);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .check em {
    font-style: normal;
    color: var(--muted);
    font-size: var(--text-xs);
    margin-left: var(--space-1);
  }
  .hint {
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
