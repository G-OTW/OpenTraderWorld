<script>
  import Modal from './Modal.svelte';

  // A question with more than one way forward (ConfirmModal has only yes/no).
  // props:
  //   open (bindable), title, message, cancelLabel,
  //   choices: [{ value, label, variant: 'primary' | 'danger' | 'ghost' | undefined }],
  //   onpick(value), oncancel()
  let {
    open = $bindable(false),
    title = '',
    message = '',
    cancelLabel = 'Cancel',
    choices = [],
    onpick = () => {},
    oncancel = () => {}
  } = $props();

  function pick(value) {
    open = false;
    onpick(value);
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
    {#each choices as c (c.value)}
      <button class={c.variant ?? 'btn'} onclick={() => pick(c.value)}>{c.label}</button>
    {/each}
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
