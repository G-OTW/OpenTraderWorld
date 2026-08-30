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
    accept = '.csv,.tsv,.txt,.json,text/csv,text/plain,application/json',
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
    <input type="file" {accept} onchange={(e) => e.target.files?.[0] && onpick(e.target.files[0])} />
    <span class="btn">{busy ? busyLabel : browseLabel}</span>
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
  }
</style>
