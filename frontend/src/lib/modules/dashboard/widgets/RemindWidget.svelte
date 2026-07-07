<script>
  // Reminder widget: a quick add-reminder form (reusing the module's ReminderForm). Saves
  // straight through the reminders API; on success it resets for the next one.
  import ReminderForm from '$lib/modules/remindme/ReminderForm.svelte';
  import { remindApi } from '$lib/modules/remindme/api.js';
  import { t } from '$lib/i18n';

  let { editing } = $props();

  let err = $state('');
  let saved = $state(false);
  let key = $state(0); // bump to reset the form after a save

  async function submit(payload) {
    err = '';
    try {
      await remindApi.add(payload);
      saved = true;
      key += 1;
      setTimeout(() => (saved = false), 2500);
    } catch (e) {
      err = e.message;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.remind.preview')}</p>
{:else}
  {#if err}<p class="err">{err}</p>{/if}
  {#if saved}<p class="ok">{$t('dashboard.widgets.remind.added')}</p>{/if}
  {#key key}
    <ReminderForm onsubmit={submit} oncancel={() => {}} />
  {/key}
{/if}

<style>
  .hint {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .err {
    color: var(--red);
    font-size: 0.8rem;
  }
  .ok {
    color: var(--green);
    font-size: 0.8rem;
  }
</style>
