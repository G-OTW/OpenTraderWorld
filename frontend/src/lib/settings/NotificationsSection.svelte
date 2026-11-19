<script>
  // Settings → Notifications: the same broker admin the modules open in a modal. One
  // component, so a channel is created, credentialed and granted identically wherever
  // the user reaches it from.
  import ChannelAdmin from '$lib/notifications/ChannelAdmin.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import {
    ALERT_SOUNDS,
    getAlertSound,
    onAlertSound,
    playAlertSound,
    setAlertSound
  } from '$lib/notifications/alertSound.js';
  import { t } from '$lib/i18n';

  let sound = $state(getAlertSound());
  // The stored choice may arrive from the server after this section opened.
  $effect(() => onAlertSound((id) => (sound = id)));
  const soundOptions = $derived([
    { value: 'off', label: $t('notifch.sound.off') },
    ...ALERT_SOUNDS.map((s) => ({ value: s.id, label: $t(`notifch.sound.${s.id}`) }))
  ]);

  /** Picking a sound plays it: that is how the choice is made. */
  function pickSound(id) {
    setAlertSound(id);
    playAlertSound(id);
  }
</script>

<div class="section">
  <h2>{$t('notifch.title')}</h2>
  <p class="muted small">{$t('notifch.subtitle')}</p>

  <ChannelAdmin />

  <h2>{$t('notifch.sound.title')}</h2>
  <p class="muted small">{$t('notifch.sound.hint')}</p>
  <div class="sound">
    <div class="sound-pick">
      <Dropdown bind:value={sound} ariaLabel={$t('notifch.sound.title')} options={soundOptions} onpick={pickSound} />
    </div>
    <Button icon="volume-2" disabled={sound === 'off'} onclick={() => playAlertSound(sound)}>
      {$t('notifch.sound.listen')}
    </Button>
  </div>
</div>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--muted);
  }
  .sound {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .sound-pick {
    flex: 0 1 240px;
    min-width: 0;
  }
  .small {
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
  }
</style>
