<script>
  // Content-shaped placeholder. Never replace a table with a centered spinner:
  // the layout collapses, then snaps back, and the reader loses their place.
  // A skeleton holds the shape so only the content arrives.
  //
  //   <Skeleton height="1rem" width="60%" />
  //   <Skeleton rows={5} height={"var(--row-h)"} />   <!-- a table body -->
  //   <Skeleton circle size="32px" />                 <!-- an avatar -->
  //
  // Honors prefers-reduced-motion via the global rule in default.css (the shimmer
  // collapses to a static block).
  // props: rows, height, width, circle, size, gap
  let {
    rows = 1,
    height = '1rem',
    width = '100%',
    circle = false,
    size = '32px',
    gap = 'var(--space-2)'
  } = $props();

  const items = $derived(Array.from({ length: Math.max(1, rows) }, (_, i) => i));
  // Last row of a paragraph-ish block reads more natural when it's short.
  const widthFor = (i) => (rows > 1 && i === rows - 1 ? '65%' : width);
</script>

<div class="stack" style:gap aria-hidden="true">
  {#each items as i (i)}
    <span
      class="bone"
      class:circle
      style:height={circle ? size : height}
      style:width={circle ? size : widthFor(i)}
    ></span>
  {/each}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
  }

  .bone {
    display: block;
    flex: none;
    border-radius: var(--radius);
    /* Two stops of the same surface, swept across — reads as "loading" without
       the carnival effect of a bright gradient. */
    background: linear-gradient(
      90deg,
      var(--surface-2) 0%,
      color-mix(in srgb, var(--surface-2) 60%, var(--border)) 50%,
      var(--surface-2) 100%
    );
    background-size: 200% 100%;
    animation: sweep 1.4s ease-in-out infinite;
  }
  .bone.circle {
    border-radius: 50%;
  }

  @keyframes sweep {
    from {
      background-position: 200% 0;
    }
    to {
      background-position: -200% 0;
    }
  }
</style>
