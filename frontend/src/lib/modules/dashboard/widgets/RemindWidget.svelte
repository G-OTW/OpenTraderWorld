<script>
  // Reminder widget: a quick add-reminder form (reusing the module's ReminderForm). Saves
  // straight through the reminders API; on success it resets for the next one.
  import ReminderForm from '$lib/modules/remindme/ReminderForm.svelte';
  import { remindApi } from '$lib/modules/remindme/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';

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
  <p class="w-state">{$t('dashboard.widgets.remind.preview')}</p>
{:else}
  <div class="w-body">
    <ErrorText error={err} />
    <!-- role="status": the save confirmation waits its turn rather than interrupting. -->
    {#if saved}<p class="ok" role="status"><Icon name="check" size={12} /> {$t('dashboard.widgets.remind.added')}</p>{/if}
    {#key key}
      <ReminderForm onsubmit={submit} oncancel={() => {}} />
    {/key}
  </div>
{/if}

<style>
  .ok {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    color: var(--green-ink);
    font-size: var(--text-sm);
  }
</style>
