<script>
  // Raised over an import modal when the pull it is about to run is long: say how long,
  // and let the user wait here or send it to the background and be told when it is done.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtDuration } from './longrun.svelte.js';
  import { t } from '$lib/i18n';

  /** guard: a LongRun. */
  let { guard } = $props();

  let open = $state(false);
  $effect(() => {
    open = !!guard.prompt;
  });
</script>

<Modal bind:open title={$t('tasks.long.title')} onclose={() => guard.dismiss()}>
  <div class="body">
    <p class="eta">
      <Icon name="clock" size={16} />
      <span>{$t('tasks.long.eta', { time: fmtDuration(guard.prompt?.seconds, $t) })}</span>
    </p>
    <p class="hint">{$t('tasks.long.hint')}</p>
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => guard.wait()}>{$t('tasks.long.wait')}</button>
    <button class="primary" onclick={() => guard.sendToBackground()}>
      {$t('tasks.long.background')}
    </button>
  {/snippet}
</Modal>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .eta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
    font-size: var(--text-base);
    margin: 0;
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
    margin: 0;
  }
</style>
