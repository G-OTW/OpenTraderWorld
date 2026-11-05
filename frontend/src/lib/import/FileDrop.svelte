<script>
  // The first step of every import: hand over a file. Drop zone + browse button, no
  // upload — the caller reads the bytes in the browser and ships them with each call.
  import Icon from '$lib/ui/Icon.svelte';

  let {
    title = '',
    hint = '',
    browseLabel = '',
    busyLabel = '',
    busy = false,
    accept = '.csv,.tsv,.txt,.json,.parquet,.pq,text/csv,text/plain,application/json',
    onpick = () => {}
  } = $props();

  let dragging = $state(false);

  function onDrop(e) {
    e.preventDefault();
    dragging = false;
    const f = e.dataTransfer?.files?.[0];
    if (f) onpick(f);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="drop"
  class:dragging
  class:busy
  ondragover={(e) => {
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={onDrop}
>
  <Icon name="upload" size={22} />
  <p class="drop-title">{title}</p>
  <p class="drop-hint">{hint}</p>
  <label class="browse">
    <input
      type="file"
      {accept}
      disabled={busy}
      onchange={(e) => e.target.files?.[0] && onpick(e.target.files[0])}
    />
    <!-- Reading a big file is a wait with nothing else on screen: the button spins for it. -->
    <span class="btn" class:busy aria-busy={busy}>
      {#if busy}<span class="spin" aria-hidden="true"></span>{/if}
      {busy ? busyLabel : browseLabel}
    </span>
  </label>
</div>

<style>
  .drop {
    border: 0.5px dashed var(--border-control);
    padding: var(--space-8) var(--space-4);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    text-align: center;
  }
  .drop.dragging {
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .drop.busy {
    cursor: progress;
  }
  .drop-title {
    font-size: var(--text-base);
    color: var(--text);
  }
  .drop-hint {
    font-size: var(--text-sm);
    max-width: 46ch;
  }
  .browse {
    margin-top: var(--space-2);
  }
  .browse input {
    display: none;
  }
  .browse .btn {
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .browse .btn.busy {
    cursor: progress;
  }
  .spin {
    width: 12px;
    height: 12px;
    flex: none;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    opacity: 0.7;
    animation: spin 600ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
