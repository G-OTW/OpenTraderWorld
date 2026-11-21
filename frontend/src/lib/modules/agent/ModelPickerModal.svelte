<script>
  // The model picker as a dialog — the Agent page has room for one. The assistant widget
  // renders the same panel inline instead.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import ModelPicker from '$lib/modules/agent/ModelPicker.svelte';

  let {
    open = $bindable(false),
    providers = [],
    providerId = null,
    model = '',
    effective = '',
    scope = '',
    onapply = () => {}
  } = $props();
</script>

<Modal bind:open title={$t('agent.pick.modelTitle')} size="md">
  <ModelPicker
    {providers}
    {providerId}
    {model}
    {effective}
    {scope}
    onapply={(patch) => {
      open = false;
      onapply(patch);
    }}
    oncancel={() => (open = false)}
  />
</Modal>
