<script>
  // A QR code as inline SVG, one <rect> per dark module.
  //
  // The encoder is loaded on demand: it is the only screen that needs it, so it stays out
  // of the main bundle and costs nothing until an enrolment is actually started.
  let { value, size = 176, alt = '' } = $props();

  let grid = $state(null); // boolean[][] once the encoder has run, null while loading

  $effect(() => {
    const text = value;
    let alive = true;
    (async () => {
      if (!text) {
        grid = null;
        return;
      }
      const { default: qrcode } = await import('qrcode-generator');
      const qr = qrcode(0, 'M'); // 0 = smallest version that fits, M = 15% recovery
      qr.addData(text);
      qr.make();
      const n = qr.getModuleCount();
      const next = Array.from({ length: n }, (_, r) =>
        Array.from({ length: n }, (_, c) => qr.isDark(r, c))
      );
      if (alive) grid = next;
    })();
    return () => {
      alive = false;
    };
  });

  const QUIET = 4; // modules of margin the spec requires around the symbol
  let span = $derived(grid ? grid.length + QUIET * 2 : 0);
</script>

{#if grid}
  <svg
    class="qr"
    width={size}
    height={size}
    viewBox="0 0 {span} {span}"
    shape-rendering="crispEdges"
    role="img"
    aria-label={alt}
  >
    <rect width={span} height={span} fill="#fff" />
    {#each grid as row, r}
      {#each row as dark, c}
        {#if dark}
          <rect x={c + QUIET} y={r + QUIET} width="1" height="1" fill="#000" />
        {/if}
      {/each}
    {/each}
  </svg>
{:else}
  <div class="qr placeholder" style="width: {size}px; height: {size}px;"></div>
{/if}

<style>
  .qr {
    display: block;
    border-radius: var(--radius);
  }
  .placeholder {
    background: var(--surface-2);
  }
</style>
