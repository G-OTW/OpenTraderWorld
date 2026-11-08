<script>
  // Shown in an import modal reopened while its pull still runs in the background: the
  // answer lands here when it is done, and the pull can be stopped.
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtDuration } from './longrun.svelte.js';
  import { t, locale } from '$lib/i18n';

  /** guard: a LongRun. */
  let { guard } = $props();

  const since = $derived(
    guard.running
      ? new Date(guard.running.started_at).toLocaleTimeString($locale, {
          hour: '2-digit',
          minute: '2-digit'
        })
      : ''
  );
</script>

{#if guard.running}
  <div class="banner">
    <Icon name="clock" size={14} />
    <span class="txt">
      {$t('tasks.running', {
        since,
        time: fmtDuration(guard.running.estimate_secs, $t)
      })}
    </span>
    <button class="stop" onclick={() => guard.cancel()}>{$t('tasks.stop')}</button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-sm);
  }
  .txt {
    flex: 1;
    line-height: 1.4;
  }
  .stop {
    background: none;
    border: none;
    color: var(--red);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
  }
</style>
