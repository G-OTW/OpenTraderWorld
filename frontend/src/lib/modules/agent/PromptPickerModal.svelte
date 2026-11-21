<script>
  // The prompt browser as a dialog — the Agent page has room for one. The assistant widget
  // renders the same panel inline instead.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import PromptPicker from '$lib/modules/agent/PromptPicker.svelte';

  let { open = $bindable(false), oninsert = () => {} } = $props();
</script>

<Modal bind:open title={$t('agent.promptPick.title')} size="lg">
  <PromptPicker
    oninsert={(body) => {
      open = false;
      oninsert(body);
    }}
    oncancel={() => (open = false)}
  />
</Modal>
