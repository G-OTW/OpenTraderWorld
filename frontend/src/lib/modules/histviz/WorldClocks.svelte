<script>
  // The bottom bar's clocks: the chart's own zone (always shown, with its offset) and the
  // extra zones the user adds from the clock button. Extras belong to the workspace, so they
  // are saved with it. When they do not fit the bar, the row steps through them on its own
  // and pauses while hovered.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { t } from '$lib/i18n';
  import { cityOf, validZone } from './clocks.js';

  let {
    /** The main clock's text and zone (the chart's time axis zone). */
    main = '',
    mainTz = '',
    now = Date.now(),
    /** The workspace's extra clocks: [{ tz }]. */
    clocks = $bindable([])
  } = $props();

  const STEP_MS = 4000;

  let open = $state(false);
  let btn = $state(null);
  let at = $state(null);
  let viewport = $state(null);
  let overflow = $state(false);
  let hover = $state(false);

  const zones = (() => {
    try {
      return typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : ['UTC'];
    } catch {
      return ['UTC'];
    }
  })();

  const list = $derived((clocks ?? []).filter((c) => validZone(c?.tz)));

  const fmts = $derived(
    new Map(
      list.map((c) => [
        c.tz,
        new Intl.DateTimeFormat('en-GB', { timeZone: c.tz, hourCycle: 'h23', hour: '2-digit', minute: '2-digit' })
      ])
    )
  );

  const options = $derived(
    zones.filter((z) => !list.some((c) => c.tz === z)).map((z) => ({ value: z, label: z.replaceAll('_', ' ') }))
  );

  function add(tz) {
    if (!validZone(tz) || list.some((c) => c.tz === tz)) return;
    clocks = [...list, { tz }];
  }
  function remove(tz) {
    clocks = list.filter((c) => c.tz !== tz);
  }

  function toggle() {
    if (open) return (open = false);
    const r = btn.getBoundingClientRect();
    at = { right: Math.max(4, window.innerWidth - r.right), bottom: window.innerHeight - r.top + 4 };
    open = true;
  }

  // Whether the extras overflow the room the bar leaves them.
  $effect(() => {
    if (!viewport) return;
    list.length;
    const check = () => (overflow = viewport.scrollWidth > viewport.clientWidth + 1);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(viewport);
    return () => ro.disconnect();
  });

  // Step one clock at a time while they overflow, back to the first after the last.
  $effect(() => {
    if (!viewport || !overflow || hover) return;
    const id = setInterval(() => {
      const items = [...viewport.children];
      const max = viewport.scrollWidth - viewport.clientWidth;
      if (viewport.scrollLeft >= max - 1) return viewport.scrollTo({ left: 0, behavior: 'smooth' });
      const next = items.find((el) => el.offsetLeft > viewport.scrollLeft + 1);
      viewport.scrollTo({ left: Math.min(next ? next.offsetLeft : max, max), behavior: 'smooth' });
    }, STEP_MS);
    return () => clearInterval(id);
  });
</script>

<span class="clocks" use:clickOutside={() => (open = false)}>
  {#if list.length}
    <span
      class="extras"
      class:overflow
      bind:this={viewport}
      role="group"
      aria-label={$t('histviz.clocks.title')}
      onmouseenter={() => (hover = true)}
      onmouseleave={() => (hover = false)}
    >
      {#each list as c (c.tz)}
        <span class="extra" title={c.tz}>{cityOf(c.tz)}: {fmts.get(c.tz)?.format(now)}</span>
      {/each}
    </span>
  {/if}
  <button
    bind:this={btn}
    type="button"
    class="clock-btn"
    class:on={open}
    aria-expanded={open}
    aria-haspopup="dialog"
    title={$t('histviz.clocks.title')}
    aria-label={$t('histviz.clocks.title')}
    onclick={toggle}
  >
    <Icon name="clock" size={12} />
  </button>
  {#if main}
    <span class="main" title={mainTz}>{main}</span>
  {/if}

  {#if open && at}
    <div class="pop" style="right:{at.right}px;bottom:{at.bottom}px" role="dialog" aria-label={$t('histviz.clocks.title')}>
      <div class="head">{$t('histviz.clocks.title')}</div>
      {#if list.length}
        <ul>
          {#each list as c (c.tz)}
            <li>
              <span class="zone" title={c.tz}>{cityOf(c.tz)}</span>
              <span class="time">{fmts.get(c.tz)?.format(now)}</span>
              <button
                type="button"
                class="rm"
                title={$t('histviz.clocks.remove')}
                aria-label={$t('histviz.clocks.remove')}
                onclick={() => remove(c.tz)}
              >
                <Icon name="x" size={11} />
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="none">{$t('histviz.clocks.none')}</p>
      {/if}
      <Dropdown
        value=""
        {options}
        float
        searchable
        placeholder={$t('histviz.clocks.add')}
        ariaLabel={$t('histviz.clocks.add')}
        searchPlaceholder={$t('histviz.clocks.add')}
        onpick={add}
      />
    </div>
  {/if}
</span>

<style>
  .clocks {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
    margin-left: auto;
    gap: var(--space-1);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .extras {
    display: flex;
    gap: var(--space-3);
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    color: var(--muted);
    white-space: nowrap;
  }
  .extras::-webkit-scrollbar {
    display: none;
  }
  .extras.overflow {
    mask-image: linear-gradient(to right, transparent, #000 12px, #000 calc(100% - 12px), transparent);
  }
  .extra {
    flex: none;
    padding: 0 var(--space-1);
  }
  .clock-btn {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .clock-btn:hover,
  .clock-btn.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .main {
    flex: none;
    padding: 0 var(--space-2) 0 0;
    color: var(--muted);
    white-space: nowrap;
  }
  .pop {
    position: fixed;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 260px;
    padding: var(--space-3);
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
    font-family: var(--font-ui);
  }
  .head {
    color: var(--text);
    font-weight: 600;
  }
  ul {
    display: flex;
    flex-direction: column;
    max-height: 220px;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
  }
  .zone {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    color: var(--muted);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .rm {
    display: inline-flex;
    padding: 2px;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .rm:hover {
    background: var(--surface-2);
    color: var(--red);
  }
  .none {
    margin: 0;
    color: var(--muted);
  }
  /* Phone: the panel fits the screen and its rows take a finger. The clock button keeps its
     size in the chart's strip, a wider invisible hit area around it takes the tap. */
  @media (max-width: 767px) {
    .pop { width: min(260px, calc(100vw - 2 * var(--space-4))); }
    .zone { white-space: normal; overflow-wrap: anywhere; }
    li { min-height: 44px; }
    .rm { min-width: 44px; min-height: 44px; align-items: center; justify-content: center; }
  }
  @media (max-width: 767px), (hover: none) {
    .clock-btn { position: relative; }
    .clock-btn::after { content: ''; position: absolute; inset: -12px; }
  }
</style>
