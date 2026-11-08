<script>
  // Quick order bar: Sell, quantity, Buy, stuck to an edge of one chart.
  //
  // Dragged by its grip, it follows the pointer freely and snaps to the nearest edge on
  // release: along the top or the bottom it lays out in a row, along a side in a column, so it
  // never covers more of the chart than one strip. Where it sits is the pane's (`edge`, `at`),
  // saved with the workspace like the pane heights.
  //
  // A button press is a market order at whatever the market is; the prices on the buttons are
  // the side it would cross (ask to buy, bid to sell), or the last close without a quote.
  //
  // The cog at the far end (shown on hover) picks what the size is typed in: the instrument's
  // own unit, or an amount of money where the instrument allows it (see `tradeUnitsFor`).
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { snapEdge } from './orders.js';

  let {
    edge = 'top',
    at = 0,
    inset = null, // the chart's scales at each edge ({ left, right, bottom } px): docked past them
    qty = $bindable(1),
    unit = 'units', // which of `units` the size is typed in
    units = [], // tradeUnitsFor(): [{ value, unit, code }]
    bid = null,
    ask = null,
    fmtPrice = (v) => String(v ?? ''),
    disabled = false,
    reason = '', // why the buttons are off, when they are
    onbuy,
    onsell,
    onmove,
    onunit // (value): another unit picked
  } = $props();

  const row = $derived(edge === 'top' || edge === 'bottom');
  let box = $state(null);
  /** While dragging: where the bar is, in pane pixels. Null when it sits on its edge. */
  let drag = $state(null);

  function down(e) {
    if (e.button !== 0) return;
    const host = box?.offsetParent;
    if (!host) return;
    e.preventDefault();
    e.stopPropagation();
    const h = host.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    drag = { dx: e.clientX - b.left, dy: e.clientY - b.top, x: b.left - h.left, y: b.top - h.top, w: b.width, hh: b.height };
    e.currentTarget.setPointerCapture?.(e.pointerId);
  }
  function move(e) {
    if (!drag) return;
    const h = box?.offsetParent?.getBoundingClientRect();
    if (!h) return;
    drag = { ...drag, x: e.clientX - h.left - drag.dx, y: e.clientY - h.top - drag.dy };
  }
  function up() {
    if (!drag) return;
    const h = box?.offsetParent?.getBoundingClientRect();
    if (h) onmove?.(snapEdge(drag.x + drag.w / 2, drag.y + drag.hh / 2, h.width, h.height));
    drag = null;
  }

  const place = $derived.by(() => {
    if (drag) return `left:${drag.x}px;top:${drag.y}px`;
    const shift = `${(-at * 100).toFixed(2)}%`;
    const l = 6 + (inset?.left ?? 0);
    const r = 6 + (inset?.right ?? 0);
    const b = 6 + (inset?.bottom ?? 0);
    // Along an edge the bar slides between the scales, never over them.
    const along = `left:calc(${l}px + (100% - ${l + r}px) * ${at});transform:translateX(${shift})`;
    const down = `top:calc(6px + (100% - ${6 + b}px) * ${at});transform:translateY(${shift})`;
    if (edge === 'top') return `top:6px;${along}`;
    if (edge === 'bottom') return `bottom:${b}px;${along}`;
    if (edge === 'left') return `left:${l}px;${down}`;
    return `right:${r}px;${down}`;
  });

  const qtyOk = $derived(Number(qty) > 0);

  let menu = $state(false);
  const labelOf = (u) => $t(`histviz.trade.unit.${u.unit}`) + (u.code ? ` (${u.code})` : '');
  const current = $derived(units.find((u) => u.value === unit) ?? units[0] ?? null);
  const unitText = $derived(current ? labelOf(current) : $t('histviz.trade.qty'));
  function pick(v) {
    menu = false;
    if (v !== unit) onunit?.(v);
  }

  /** One unit up or down. Never to zero or below: the minus stops where the size would
   *  vanish, so the buttons never fall silent on a size of 0. */
  function bump(d) {
    const q = Number(qty) || 0;
    const n = Math.round((q + d) * 1e8) / 1e8;
    if (n > 0) qty = n;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="qo"
  class:col={!row}
  class:dragging={!!drag}
  style={place}
  bind:this={box}
  onpointerdown={(e) => e.stopPropagation()}
  onwheel={(e) => e.stopPropagation()}
  title={disabled ? reason : ''}
>
  <span
    class="grip"
    role="button"
    tabindex="-1"
    aria-label={$t('histviz.trade.moveBar')}
    title={$t('histviz.trade.moveBar')}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
  >
    <Icon name="grip-vertical" size={12} />
  </span>
  <button
    type="button"
    class="side sell"
    disabled={disabled || !qtyOk}
    title={$t('histviz.trade.sellMarket')}
    onclick={() => onsell?.()}
  >
    <span class="lbl">{$t('histviz.trade.sell')}</span>
    {#if bid != null}<span class="px">{fmtPrice(bid)}</span>{/if}
  </button>
  <!-- One frame for the stepper: the field inside is borderless. -->
  <div class="qty">
    <button type="button" class="step" aria-label={$t('histviz.trade.qtyLess')} title={$t('histviz.trade.qtyLess')} disabled={!(Number(qty) > 1)} onclick={() => bump(-1)}>
      <Icon name="minus" size={11} />
    </button>
    <input
      type="number"
      min="0"
      step="any"
      aria-label={unitText}
      title={unitText}
      bind:value={qty}
      onkeydown={(e) => {
        e.stopPropagation();
        if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
          e.preventDefault();
          bump(e.key === 'ArrowUp' ? 1 : -1);
        }
      }}
    />
    <button type="button" class="step" aria-label={$t('histviz.trade.qtyMore')} title={$t('histviz.trade.qtyMore')} onclick={() => bump(1)}>
      <Icon name="plus" size={11} />
    </button>
  </div>
  <button
    type="button"
    class="side buy"
    disabled={disabled || !qtyOk}
    title={$t('histviz.trade.buyMarket')}
    onclick={() => onbuy?.()}
  >
    <span class="lbl">{$t('histviz.trade.buy')}</span>
    {#if ask != null}<span class="px">{fmtPrice(ask)}</span>{/if}
  </button>
  <span class="unitbox">
    <button
      type="button"
      class="cog"
      class:open={menu}
      aria-haspopup="menu"
      aria-expanded={menu}
      aria-label={$t('histviz.trade.unitMenu')}
      title={`${$t('histviz.trade.unitMenu')}: ${unitText}`}
      onclick={() => (menu = !menu)}
    >
      <Icon name="cog" size={11} />
    </button>
    {#if menu}
      <div class="menu {edge}" role="menu">
        {#each units as u (u.value)}
          <button type="button" role="menuitemradio" aria-checked={u.value === current?.value} class:on={u.value === current?.value} onclick={() => pick(u.value)}>
            <span class="tick">{#if u.value === current?.value}<Icon name="check" size={11} />{/if}</span>
            {labelOf(u)}
          </button>
        {/each}
      </div>
    {/if}
  </span>
</div>

<svelte:window
  onpointerdown={() => (menu = false)}
  onkeydown={(e) => {
    if (menu && e.key === 'Escape') menu = false;
  }}
/>

<style>
  .qo {
    position: absolute;
    z-index: 12;
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 2px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.18);
    font-size: var(--text-xs);
    user-select: none;
  }
  .qo.col {
    flex-direction: column;
    min-width: 74px;
  }
  .qo.dragging {
    opacity: 0.85;
  }
  .grip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    cursor: grab;
    touch-action: none;
  }
  /* The icon turns, not the grip: a turned grip keeps its full-width box rotated into a tall
     strip over the middle of the Buy button, and a click there drags instead of buying. */
  .col .grip {
    height: 14px;
  }
  .col .grip :global(svg) {
    transform: rotate(90deg);
  }
  .dragging .grip {
    cursor: grabbing;
  }
  /* The far end mirrors the grip: a strip of its own, the cog showing on hover only. */
  .unitbox {
    position: relative;
    display: inline-flex;
    align-items: stretch;
  }
  .col .unitbox {
    order: 4;
    height: 14px;
  }
  .cog {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    padding: 0 1px;
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .qo:hover .cog,
  .cog:focus-visible,
  .cog.open {
    opacity: 1;
  }
  .cog:hover,
  .cog.open {
    color: var(--text);
  }
  .menu {
    position: absolute;
    z-index: 1;
    display: flex;
    flex-direction: column;
    min-width: 130px;
    padding: 2px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.18);
  }
  /* Opens toward the chart, away from the edge the bar is docked on. */
  .menu.top {
    top: calc(100% + 4px);
    right: 0;
  }
  .menu.bottom {
    bottom: calc(100% + 4px);
    right: 0;
  }
  .menu.left {
    left: calc(100% + 4px);
    bottom: 0;
  }
  .menu.right {
    right: calc(100% + 4px);
    bottom: 0;
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border: none;
    border-radius: calc(var(--radius) - 2px);
    background: transparent;
    color: var(--text);
    font-size: var(--text-xs);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--surface-2);
  }
  .menu button.on {
    color: var(--accent);
  }
  .tick {
    display: inline-flex;
    width: 11px;
  }
  .side {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-width: 62px;
    padding: 2px var(--space-2);
    border: none;
    border-radius: calc(var(--radius) - 2px);
    color: #fff;
    line-height: 1.2;
    cursor: pointer;
  }
  /* Blue buy / red sell, the convention the quick-backtest markers already use. */
  .side.buy {
    background: var(--accent);
  }
  .side.sell {
    background: var(--red);
  }
  .side:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .side:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .lbl {
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .px {
    font-family: var(--mono);
  }
  .qty {
    display: flex;
    align-items: stretch;
    width: 76px;
    height: 22px;
    align-self: center;
    border: 1px solid var(--border);
    border-radius: calc(var(--radius) - 2px);
    overflow: hidden;
  }
  .qty input {
    flex: 1;
    width: 0;
    min-width: 0;
    height: auto;
    padding: 0;
    border: none;
    background: transparent;
    text-align: center;
    font-family: var(--mono);
    font-size: var(--text-xs);
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .qty input::-webkit-inner-spin-button,
  .qty input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .step {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 18px;
    padding: 0;
    border: none;
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
  }
  .step:hover:not(:disabled) {
    color: var(--text);
  }
  .step:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .col .qty {
    width: auto;
    align-self: stretch;
  }
  .col .side {
    min-width: 0;
  }
  /* The column reads top-down: buy above the size, sell under it. */
  .col .buy {
    order: 1;
  }
  .col .qty {
    order: 2;
  }
  .col .sell {
    order: 3;
  }
</style>
