<script>
  // Test harness: renders `component` with `inner` props inside a box of a given size, so a
  // browser test can resize the box with `rerender({ width })` and watch the chart follow.
  let { component: Inner, inner = {}, width = 600, height = 'auto' } = $props();
</script>

<div
  data-testid="box"
  style:width="{width}px"
  style:height={typeof height === 'number' ? `${height}px` : height}
  style:position="relative"
>
  <Inner {...inner} />
</div>
