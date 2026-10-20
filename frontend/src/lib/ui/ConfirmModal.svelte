<script>
  import Modal from './Modal.svelte';

  // A small confirmation dialog. Replaces native confirm().
  // props:
  //   open (bindable), title, message, confirmLabel, cancelLabel,
  //   danger (red confirm button), onconfirm(), oncancel()
  let {
    open = $bindable(false),
    title = '',
    message = '',
    confirmLabel = 'OK',
    cancelLabel = 'Cancel',
    danger = false,
    onconfirm = () => {},
    oncancel = () => {}
  } = $props();

  function confirm() {
    open = false;
    onconfirm();
  }
  function cancel() {
    open = false;
    oncancel();
  }
</script>

<Modal bind:open {title} onclose={oncancel}>
  <p class="msg">{message}</p>

  {#snippet footer()}
    <button class="ghost" onclick={cancel}>{cancelLabel}</button>
    <button class="primary" class:danger onclick={confirm}>{confirmLabel}</button>
  {/snippet}
</Modal>

<style>
  .msg {
    color: var(--text);
    font-size: var(--text-base);
    line-height: 1.5;
    margin: 0;
    white-space: pre-line;
  }
</style>
