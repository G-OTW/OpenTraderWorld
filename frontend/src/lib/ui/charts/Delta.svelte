<script>
  // A signed change: glyph (+ or U+2212 minus) + arrow + tone. The sign is always in
  // the text and the arrow, never in the colour alone. Zero reads muted with no arrow;
  // a value that cannot be measured reads as `empty` (a word, never 0).
  //
  //   <Delta value={3.24} />                          +3.24% with an up arrow, green
  //   <Delta value={-120} format={(v) => fmtMoney(v, 'EUR')} sensitive />
  //   <Delta value={costChange} invert size="md" />   a cost: down is good
  //
  // props: value (number | null), suffix (default '%'), digits (default 2), size
  //        ('sm' 12px | 'md' 15px), format(abs) (formats the magnitude; overrides
  //        digits/suffix), invert (down is good), arrow (default true), empty (text for
  //        null, default 'n/a'; pass a translated word), sensitive (mask under the
  //        privacy screen, default false: a percent reveals no amount)
  import Icon from '$lib/ui/Icon.svelte';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import { MASK, toneOf } from './palette.js';

  let {
    value = null,
    suffix = '%',
    digits = 2,
    size = 'sm',
    format = null,
    invert = false,
    arrow = true,
    empty = 'n/a',
    sensitive = false
  } = $props();

  const known = $derived(typeof value === 'number' && Number.isFinite(value));
  const tone = $derived(known ? toneOf(value, invert) : 'flat');
  const dir = $derived(!known || value === 0 ? 'flat' : value > 0 ? 'up' : 'down');
  const sign = $derived(dir === 'up' ? '+' : dir === 'down' ? '−' : '');
  const magnitude = $derived(
    known ? (format ? format(Math.abs(value)) : `${Math.abs(value).toFixed(digits)}${suffix}`) : ''
  );
  const text = $derived(!known ? empty : sensitive && privacy.hidden ? MASK : `${sign}${magnitude}`);
</script>

<span class="delta {size} {tone}" class:unknown={!known}>
  {#if arrow && dir !== 'flat'}
    <Icon name={dir === 'up' ? 'arrow-up' : 'arrow-down'} size={size === 'md' ? 14 : 12} />
  {/if}
  <span class="num">{text}</span>
</span>

<style>
  .delta {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    line-height: var(--lh-tight);
    white-space: nowrap;
  }
  .delta.sm {
    font-size: 12px;
  }
  .delta.md {
    font-size: 15px;
  }
  .delta.up {
    color: var(--green);
  }
  .delta.down {
    color: var(--red);
  }
  .delta.flat {
    color: var(--muted);
  }
  .delta.unknown {
    font-family: var(--font);
  }
</style>
