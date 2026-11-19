<script>
  // Top bar: the timers pinned from the Time module. One chip: start/pause, then the name
  // or the elapsed time (a click on it swaps the two). With several pins the chip shows
  // the one picked in the menu, else the running one, else the first pinned; a chevron
  // opens the others.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { fmtDuration } from '$lib/format.js';
  import {
    timerPins,
    refreshPins,
    toggleTimer,
    togglePin,
    toggleLabel,
    liveSeconds
  } from './pins.svelte.js';

  const POLL_MS = 60_000;

  let now = $state(Date.now());
  let open = $state(false);
  let busy = $state(null); // id of the timer being started or stopped
  let box = $state(null);
  let focusId = $state(null); // timer picked in the menu

  // Pinned timers in pin order, deleted ones left out.
  const pinned = $derived.by(() => {
    const byId = new Map((timerPins.projects ?? []).map((p) => [p.id, p]));
    return timerPins.ids.map((id) => byId.get(id)).filter(Boolean);
  });
  const primary = $derived(
    pinned.find((p) => p.id === focusId) ?? pinned.find((p) => p.running) ?? pinned[0] ?? null
  );
  const anyRunning = $derived(pinned.some((p) => p.running));
  const runningCount = $derived(pinned.filter((p) => p.running).length);

  // First load once something is pinned (also when the first pin is added).
  $effect(() => {
    if (timerPins.ids.length && timerPins.projects === null) refreshPins().catch(() => {});
  });

  // The clock ticks only while a pinned timer runs.
  $effect(() => {
    if (!anyRunning) return;
    now = Date.now();
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  $effect(() => {
    if (!pinned.length) open = false;
  });

  onMount(() => {
    // A timer started on another device shows within a minute, or as the tab comes back.
    const poll = setInterval(() => refreshPins().catch(() => {}), POLL_MS);
    const onVisible = () => document.visibilityState === 'visible' && refreshPins().catch(() => {});
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      clearInterval(poll);
      document.removeEventListener('visibilitychange', onVisible);
    };
  });

  async function toggle(p) {
    if (busy) return;
    busy = p.id;
    try {
      await toggleTimer(p);
    } catch (e) {
      toast.err(e.message);
    } finally {
      busy = null;
    }
  }

  const elapsed = (p) => fmtDuration(liveSeconds(p, now));
  const actionLabel = (p) => `${p.running ? $t('time.stop') : $t('time.start')}: ${p.name}`;

  function pick(p) {
    focusId = p.id;
    open = false;
  }

  function onWindowClick(e) {
    if (open && box && !box.contains(e.target)) open = false;
  }
  function onKey(e) {
    if (open && e.key === 'Escape') open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

{#if primary}
  <div class="tpin" class:running={primary.running} bind:this={box}>
    <button
      class="play"
      disabled={busy === primary.id}
      title={actionLabel(primary)}
      aria-label={actionLabel(primary)}
      onclick={() => toggle(primary)}
    >
      <Icon name={primary.running ? 'pause' : 'play'} size={12} strokeWidth={2.4} />
    </button>
    <button
      class="label"
      title={timerPins.showValue ? primary.name : elapsed(primary)}
      aria-label={`${primary.name}: ${elapsed(primary)}`}
      onclick={toggleLabel}
    >
      <span class="dot" style:background={primary.color || 'var(--accent)'}></span>
      {#if timerPins.showValue}
        <span class="value">{elapsed(primary)}</span>
      {:else}
        <span class="name">{primary.name}</span>
      {/if}
    </button>
    {#if pinned.length > 1}
      <button
        class="more"
        class:open
        title={$t('time.pin.all')}
        aria-label={$t('time.pin.all')}
        aria-haspopup="true"
        aria-expanded={open}
        onclick={() => (open = !open)}
      >
        {#if runningCount > 1}<span class="count">{runningCount}</span>{/if}
        <Icon name="chevron-down" size={13} />
      </button>
    {/if}

    {#if open}
      <div class="menu" role="group" aria-label={$t('time.pin.all')}>
        <ul>
          {#each pinned as p (p.id)}
            <li class="row" class:running={p.running} class:current={p.id === primary.id}>
              <button
                class="play"
                disabled={busy === p.id}
                title={actionLabel(p)}
                aria-label={actionLabel(p)}
                onclick={() => toggle(p)}
              >
                <Icon name={p.running ? 'pause' : 'play'} size={12} strokeWidth={2.4} />
              </button>
              <button class="pick" title={p.name} onclick={() => pick(p)}>
                <span class="dot" style:background={p.color || 'var(--accent)'}></span>
                <span class="name">{p.name}</span>
                <span class="value">{elapsed(p)}</span>
              </button>
              <button
                class="unpin"
                title={$t('time.pin.unpin')}
                aria-label={`${$t('time.pin.unpin')}: ${p.name}`}
                onclick={() => togglePin(p.id)}
              >
                <Icon name="x" size={12} />
              </button>
            </li>
          {/each}
        </ul>
        <a class="foot" href="/time" onclick={() => (open = false)}>
          <Icon name="timer" size={13} />{$t('time.pin.open')}
        </a>
      </div>
    {/if}
  </div>
{/if}

<style>
  /* One frame for the whole control: the buttons inside carry no border of their own. */
  .tpin {
    position: relative;
    display: inline-flex;
    align-items: center;
    height: 28px;
    padding: 0 2px;
    border: var(--hairline) solid var(--border-control);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
  }
  .tpin.running {
    border-color: color-mix(in srgb, var(--green) 45%, var(--border-control));
    background: color-mix(in srgb, var(--green) 7%, var(--surface));
  }
  button {
    border: none;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    padding: 0;
  }
  .play {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    color: var(--green);
    background: color-mix(in srgb, var(--green) 14%, transparent);
    transition: background var(--dur-fast) var(--ease);
  }
  .play:hover {
    background: color-mix(in srgb, var(--green) 26%, transparent);
  }
  .running > .play,
  .row.running .play {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 14%, transparent);
  }
  .running > .play:hover,
  .row.running .play:hover {
    background: color-mix(in srgb, var(--red) 26%, transparent);
  }
  .play:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 8px 0 7px;
    font-size: var(--fs-body);
    color: var(--text);
    border-radius: 999px;
  }
  .label:hover .name,
  .label:hover .value {
    color: var(--accent);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
  .running > .label .dot {
    animation: beat 2s ease-in-out infinite;
  }
  .name {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .value {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.01em;
  }
  .running > .label .value {
    color: var(--green);
  }
  .more {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 22px;
    padding: 0 6px 0 4px;
    margin-left: -2px;
    border-left: var(--hairline) solid var(--border);
    border-radius: 0 999px 999px 0;
    color: var(--muted);
  }
  .more:hover {
    color: var(--text);
  }
  .more :global(svg) {
    transition: transform var(--dur-fast) var(--ease);
  }
  .more.open :global(svg) {
    transform: rotate(180deg);
  }
  .count {
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--green);
    color: var(--surface);
    font-size: 10px;
    font-family: var(--mono);
    line-height: 14px;
    text-align: center;
  }

  .menu {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: var(--z-dropdown);
    min-width: 280px;
    max-width: 360px;
    padding: var(--space-1);
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
  }
  .menu ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: min(60vh, 420px);
    overflow-y: auto;
  }
  .row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-2);
    padding: 6px 6px 6px 8px;
    border-radius: var(--radius-sm);
    font-size: var(--fs-body);
    color: var(--text);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.current {
    background: var(--surface-2);
  }
  .pick {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    text-align: left;
    color: inherit;
  }
  .row .name {
    max-width: none;
  }
  .pick:hover .name {
    color: var(--accent);
  }
  .row .value {
    color: var(--muted);
  }
  .row.running .value {
    color: var(--green);
  }
  .unpin {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-sm);
    color: var(--dim);
    opacity: 0;
  }
  .row:hover .unpin,
  .unpin:focus-visible {
    opacity: 1;
  }
  .unpin:hover {
    color: var(--text);
    background: var(--surface);
  }
  @media (hover: none) {
    .unpin {
      opacity: 1;
    }
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-1);
    padding: 8px 8px 6px;
    border-top: var(--hairline) solid var(--border);
    font-size: var(--text-xs);
    color: var(--muted);
    text-decoration: none;
  }
  .foot:hover {
    color: var(--text);
  }

  @keyframes beat {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
  @media (prefers-reduced-motion: reduce) {
    .running > .label .dot { animation: none; }
  }
</style>
