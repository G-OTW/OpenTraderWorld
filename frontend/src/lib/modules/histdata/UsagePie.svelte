<script>
  // "Camembert" usage gauge: a small filled pie of used/max requests for a connector's
  // current quota window. Green → amber (≥70%) → red (≥90%/full). Callers only render
  // it when a capped quota exists; unlimited/untracked connectors keep their status dot.
  let { used = 0, max = 1, size = 14, title = '' } = $props();

  const frac = $derived(Math.min(Math.max(used, 0) / Math.max(max, 1), 1));
  const color = $derived(
    frac >= 0.9 ? 'var(--red)' : frac >= 0.7 ? 'var(--amber)' : 'var(--green)'
  );
  // Wedge from 12 o'clock, clockwise. Degenerate fractions are handled outside the path:
  // 0 draws nothing, 1 draws a full disc (an arc whose start == end renders nothing).
  const path = $derived.by(() => {
    if (frac <= 0 || frac >= 1) return '';
    const a = frac * 2 * Math.PI - Math.PI / 2;
    const x = 8 + 8 * Math.cos(a);
    const y = 8 + 8 * Math.sin(a);
    return `M8 8 L8 0 A8 8 0 ${frac > 0.5 ? 1 : 0} 1 ${x.toFixed(3)} ${y.toFixed(3)} Z`;
  });
</script>

<svg viewBox="0 0 16 16" width={size} height={size} role="img" aria-label={title}>
  {#if title}<title>{title}</title>{/if}
  <circle cx="8" cy="8" r="7.5" fill="var(--surface-2)" stroke="var(--border)" stroke-width="1" />
  {#if frac >= 1}
    <circle cx="8" cy="8" r="8" fill={color} />
  {:else if path}
    <path d={path} fill={color} />
  {/if}
</svg>
