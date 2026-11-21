<script>
  // Quick order bar: Sell, quantity, Buy, stuck to an edge of one chart.
  //
  // Dragged by its grip, it follows the pointer freely. Dropped near an edge of the plot it
  // snaps to it: along the top or the bottom it lays out in a row, along a side in a column.
  // Dropped anywhere else it stays where it was let go (`edge` 'free', `fx`, `fy`) and keeps
  // the layout it had, which the turn button next to the cog then flips (`vertical`). Where it
  // sits is the pane's (`edge`, `at`, `fx`, `fy`, `vertical`), saved with the workspace like
  // the pane heights.
  //
  // A button press is a market order at whatever the market is; the prices on the buttons are
  // the side it would cross (ask to buy, bid to sell), or the last close without a quote.
  //
  // The cog at the far end (shown on hover) picks what the size is typed in: the instrument's
  // own unit, or an amount of money where the instrument allows it (see `tradeUnitsFor`).
  // It also picks an entry preset (presets.js): both buttons then send their market order with
  // the preset's take profit and stop loss (and its size, when it has one).
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { placeBar } from './orders.js';
  import { byKey } from './presets.js';

  let {
    edge = 'top',
    at = 0,
    fx = 0, // free: share of the room left of the bar
    fy = 0, // free: share of the room above it
    vertical = false, // free: laid out in a column
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
    onclose = null, // flatten this chart's position and cancel its exits; null (no position): no Close button
    canClose = true, // false while a close is on its way
    onmove,
    onunit, // (value): another unit picked
    presets = [], // entry presets: [{ id, name, ... }]
    preset = null, // the id of the one in use
    onpreset, // (id | null)
    onpresets // open the presets' editor
  } = $props();
  const active = $derived(presets.find((p) => p.id === preset) ?? null);

  const row = $derived(edge === 'free' ? !vertical : edge === 'top' || edge === 'bottom');
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
    if (h) onmove?.({ ...placeBar(drag.x, drag.y, drag.w, drag.hh, h.width, h.height, inset), vertical: !row });
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
    if (edge === 'free') {
      const x = Number(fx) || 0;
      const y = Number(fy) || 0;
      return `left:calc(${inset?.left ?? 0}px + (100% - ${(inset?.left ?? 0) + (inset?.right ?? 0)}px) * ${x});top:calc((100% - ${inset?.bottom ?? 0}px) * ${y});transform:translate(${(-x * 100).toFixed(2)}%, ${(-y * 100).toFixed(2)}%)`;
    }
    if (edge === 'top') return `top:6px;${along}`;
    if (edge === 'bottom') return `bottom:${b}px;${along}`;
    if (edge === 'left') return `left:${l}px;${down}`;
    return `right:${r}px;${down}`;
  });

  /** Where the menu opens: away from the edge the bar sits on, or the nearer one when free. */
  const menuSide = $derived(
    edge !== 'free' ? edge : row ? (Number(fy) < 0.5 ? 'top' : 'bottom') : Number(fx) < 0.5 ? 'left' : 'right'
  );

  const qtyOk = $derived(Number(qty) > 0);

  let menu = $state(false);
  const labelOf = (u) => $t(`histviz.trade.unit.${u.unit}`) + (u.code ? ` (${u.code})` : '');
  const current = $derived(units.find((u) => u.value === unit) ?? units[0] ?? null);
  const unitText = $derived(current ? labelOf(current) : $t('histviz.trade.qty'));
  function pick(v) {
    menu = false;
    if (v !== unit) onunit?.(v);
  }
  function pickPreset(id) {
    menu = false;
    if (id !== preset) onpreset?.(id);
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
  <!-- In a row the preset sits above the size; in a column the two fall back into the bar. -->
  <div class="mid">
    {#if active}
      <!-- The preset in use, its name cut short: a long name never widens the bar. -->
      <button type="button" class="pchip" title={$t('histviz.presets.inUse', { name: active.name })} aria-label={$t('histviz.presets.inUse', { name: active.name })} onclick={() => pickPreset(null)}>
        {#if active.key != null}<span class="pkey">{active.key}</span>{:else}<span class="pkey"><Icon name="layers" size={11} /></span>{/if}
        <span class="pname">{active.name}</span>
        <span class="px-x"><Icon name="x" size={10} /></span>
      </button>
    {/if}
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
  {#if onclose}
    <button
      type="button"
      class="side close"
      disabled={disabled || !canClose}
      title={$t('histviz.trade.closePosition')}
      onclick={() => onclose?.()}
    >
      <span class="lbl">{$t('histviz.trade.close')}</span>
    </button>
  {/if}
  <span class="unitbox">
    {#if edge === 'free'}
      <button type="button" class="cog" aria-label={$t('histviz.trade.turnBar')} title={$t('histviz.trade.turnBar')} onclick={() => onmove?.({ vertical: row })}>
        <Icon name="rotate-ccw" size={12} />
      </button>
    {/if}
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
      <Icon name="cog" size={12} />
    </button>
    {#if menu}
      <div class="menu {menuSide}" role="menu">
        {#each units as u (u.value)}
          <button type="button" role="menuitemradio" aria-checked={u.value === current?.value} class:on={u.value === current?.value} onclick={() => pick(u.value)}>
            <span class="tick">{#if u.value === current?.value}<Icon name="check" size={13} />{/if}</span>
            {labelOf(u)}
          </button>
        {/each}
        <span class="msep" role="separator"></span>
        <span class="mhead">{$t('histviz.presets.title')}</span>
        <button type="button" role="menuitemradio" aria-checked={!active} class:on={!active} onclick={() => pickPreset(null)}>
          <span class="tick">{#if !active}<Icon name="check" size={13} />{/if}</span>
          {$t('histviz.presets.noPreset')}
        </button>
        {#each byKey(presets) as p (p.id)}
          <button type="button" role="menuitemradio" aria-checked={p.id === active?.id} class:on={p.id === active?.id} onclick={() => pickPreset(p.id)}>
            <span class="tick">{#if p.id === active?.id}<Icon name="check" size={13} />{/if}</span>
            <span class="mname">{p.name}</span>{#if p.key != null}<span class="mkey">{p.key}</span>{/if}
          </button>
        {/each}
        <span class="msep" role="separator"></span>
        <button type="button" role="menuitem" onclick={() => ((menu = false), onpresets?.())}>
          <span class="tick"><Icon name="pencil" size={13} /></span>
          {$t('histviz.presets.manage')}
        </button>
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
    gap: 6px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.28);
    font-size: var(--text-sm);
    user-select: none;
  }
  .qo.col {
    flex-direction: column;
    width: 120px;
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
    height: 16px;
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
  /* In a row the turn button and the cog stack in one narrow strip. */
  .qo:not(.col) .unitbox {
    flex-direction: column;
    justify-content: center;
    gap: 2px;
  }
  .qo:not(.col) .cog {
    height: 22px;
  }
  .col .unitbox {
    order: 4;
    justify-content: center;
    height: 22px;
  }
  .cog {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 22px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    outline: none;
    cursor: pointer;
    opacity: 0.6;
  }
  .qo:hover .cog,
  .cog:focus-visible,
  .cog.open {
    opacity: 1;
  }
  .cog:hover,
  .cog:focus-visible,
  .cog.open {
    color: var(--text);
    background: var(--surface-2);
  }
  .menu {
    position: absolute;
    z-index: 1;
    display: flex;
    flex-direction: column;
    min-width: 220px;
    max-width: 280px;
    padding: var(--space-1);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.28);
    font-size: var(--text-sm);
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
    gap: var(--space-2);
    min-height: 32px;
    padding: var(--space-1) var(--space-3) var(--space-1) var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .msep {
    height: 1px;
    margin: var(--space-1) 0;
    background: var(--border);
  }
  .mhead {
    padding: var(--space-2) var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .pkey {
    display: inline-grid;
    place-items: center;
    flex: none;
    min-width: 17px;
    height: 17px;
    padding: 0 3px;
    border-radius: 4px;
    background: var(--accent);
    color: rgb(0 0 0 / 0.82);
    font-size: var(--text-xs);
  }
  .pkey,
  .mkey {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .mkey {
    display: inline-grid;
    place-items: center;
    flex: none;
    min-width: 20px;
    height: 20px;
    margin-left: auto;
    padding: 0 3px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 10px;
  }
  .mname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pchip {
    display: inline-flex;
    align-items: center;
    align-self: center;
    gap: 6px;
    max-width: 136px;
    height: 26px;
    padding: 0 6px 0 3px;
    border: 1px solid color-mix(in srgb, var(--accent) 55%, var(--border));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--text);
    font-size: var(--text-xs);
    white-space: nowrap;
    cursor: pointer;
  }
  .col .pchip {
    align-self: stretch;
    max-width: none;
  }
  /* In the row, the key alone: the name stays in the tooltip and in the column layout. */
  .pname {
    display: none;
  }
  .col .pname {
    display: block;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }
  .px-x {
    display: inline-flex;
    flex: none;
    color: var(--muted);
  }
  .pchip:hover .px-x {
    color: var(--text);
  }
  .menu button:hover {
    background: var(--surface-2);
  }
  .menu button.on {
    color: var(--accent);
  }
  .tick {
    display: inline-flex;
    flex: none;
    justify-content: center;
    width: 16px;
  }
  .side {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1px;
    min-width: 82px;
    padding: 6px 10px;
    border: none;
    border-radius: var(--radius-lg);
    color: rgb(0 0 0 / 0.82);
    line-height: 1.15;
    cursor: pointer;
  }
  /* Blue buy / red sell, the convention the quick-backtest markers already use. */
  .side.buy {
    background: var(--accent);
  }
  .side.sell {
    background: var(--red);
  }
  .side.close {
    min-width: 64px;
    background: var(--surface-2);
    color: var(--text);
  }
  .side:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .side:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .lbl {
    font-size: var(--text-sm);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .px {
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .mid {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 4px;
  }
  .col .mid {
    display: contents;
  }
  .qty {
    display: flex;
    align-items: stretch;
    width: 94px;
    height: 29px;
    align-self: center;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
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
    width: 26px;
    padding: 0;
    border: none;
    background: transparent;
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
  .col .close {
    order: 4;
  }
</style>
