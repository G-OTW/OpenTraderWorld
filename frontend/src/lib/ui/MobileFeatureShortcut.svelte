<script>
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';
  import { goto } from '$app/navigation';
  import { onMount, tick } from 'svelte';

  const POSITION_KEY = 'otw.mobile.quick-search.position';
  const BUTTON_SIZE = 44;
  const EDGE_GAP = 12;

  let { entries = [], currentId = '', route = '' } = $props();
  let open = $state(false);
  let scope = $state('page');
  let query = $state('');
  let featureIndex = $state(0);
  let pageIndex = $state(0);
  let pageMatches = $state.raw([]);
  let input = $state(null);
  let trigger = $state(null);
  let root = $state(null);
  let panel = $state(null);
  let position = $state(null); // fractions of the available viewport, kept across sizes
  let viewport = $state({ left: 0, top: 0, width: 0, height: 0 });
  let panelPlacement = $state(null);
  let fallbackMarker = null;
  let drag = null;
  let suppressClick = false;

  const clamp = (value, min, max) => Math.min(Math.max(value, min), Math.max(min, max));
  const dockStyle = $derived.by(() => {
    if (!position || !viewport.width) return '';
    const x = viewport.left + EDGE_GAP + position.x * Math.max(0, viewport.width - BUTTON_SIZE - EDGE_GAP * 2);
    const y = viewport.top + EDGE_GAP + position.y * Math.max(0, viewport.height - BUTTON_SIZE - EDGE_GAP * 2);
    return `left:${x}px;top:${y}px;bottom:auto`;
  });
  const panelStyle = $derived.by(() => {
    const maxHeight = Math.max(0, Math.min(280, viewport.height * .42, viewport.height - EDGE_GAP * 2));
    return panelPlacement
      ? `left:${panelPlacement.left}px;top:${panelPlacement.top}px;bottom:auto;max-height:${maxHeight}px`
      : `max-height:${maxHeight}px`;
  });

  function measureViewport() {
    const visual = window.visualViewport;
    return {
      left: visual?.offsetLeft ?? 0,
      top: visual?.offsetTop ?? 0,
      width: visual?.width ?? window.innerWidth,
      height: visual?.height ?? window.innerHeight
    };
  }

  function placePanel() {
    if (!open || !panel || !trigger) return;
    const view = measureViewport();
    const anchor = trigger.getBoundingClientRect();
    const size = panel.getBoundingClientRect();
    const left = clamp(anchor.left, view.left + EDGE_GAP, view.left + view.width - EDGE_GAP - size.width);
    const above = anchor.top - size.height;
    const below = anchor.bottom;
    const roomAbove = anchor.top - view.top - EDGE_GAP;
    const roomBelow = view.top + view.height - EDGE_GAP - anchor.bottom;
    const preferredTop = roomAbove >= size.height || roomAbove >= roomBelow ? above : below;
    const top = clamp(preferredTop, view.top + EDGE_GAP, view.top + view.height - EDGE_GAP - size.height);
    if (panelPlacement?.left !== left || panelPlacement?.top !== top) panelPlacement = { left, top };
  }

  function onViewportChange() {
    viewport = measureViewport();
    requestAnimationFrame(placePanel);
  }

  onMount(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(POSITION_KEY) ?? 'null');
      if (Number.isFinite(saved?.x) && Number.isFinite(saved?.y)) {
        position = { x: clamp(saved.x, 0, 1), y: clamp(saved.y, 0, 1) };
      }
    } catch { /* Invalid old position: use the default corner. */ }
    onViewportChange();
    window.addEventListener('resize', onViewportChange);
    window.visualViewport?.addEventListener('resize', onViewportChange);
    window.visualViewport?.addEventListener('scroll', onViewportChange);
    return () => {
      window.removeEventListener('resize', onViewportChange);
      window.visualViewport?.removeEventListener('resize', onViewportChange);
      window.visualViewport?.removeEventListener('scroll', onViewportChange);
    };
  });

  $effect(() => {
    if (!open || !panel) return;
    const observer = new ResizeObserver(placePanel);
    observer.observe(panel);
    return () => observer.disconnect();
  });

  function onDragStart(event) {
    if (!event.isPrimary || (event.pointerType === 'mouse' && event.button !== 0)) return;
    suppressClick = false;
    const bounds = trigger.getBoundingClientRect();
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, left: bounds.left, top: bounds.top, moved: false };
    trigger.setPointerCapture(event.pointerId);
  }

  function onDragMove(event) {
    if (!drag || event.pointerId !== drag.id) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 6) return;
    drag.moved = true;
    event.preventDefault();
    const view = measureViewport();
    viewport = view;
    const left = clamp(drag.left + dx, view.left + EDGE_GAP, view.left + view.width - BUTTON_SIZE - EDGE_GAP);
    const top = clamp(drag.top + dy, view.top + EDGE_GAP, view.top + view.height - BUTTON_SIZE - EDGE_GAP);
    position = {
      x: (left - view.left - EDGE_GAP) / Math.max(1, view.width - BUTTON_SIZE - EDGE_GAP * 2),
      y: (top - view.top - EDGE_GAP) / Math.max(1, view.height - BUTTON_SIZE - EDGE_GAP * 2)
    };
    requestAnimationFrame(placePanel);
  }

  function onDragEnd(event) {
    if (!drag || event.pointerId !== drag.id) return;
    if (trigger.hasPointerCapture(event.pointerId)) trigger.releasePointerCapture(event.pointerId);
    if (drag.moved) {
      suppressClick = true;
      try { localStorage.setItem(POSITION_KEY, JSON.stringify(position)); } catch { /* Storage may be unavailable. */ }
    }
    drag = null;
  }

  function onTriggerClick(event) {
    if (suppressClick && event.detail !== 0) { suppressClick = false; return; }
    suppressClick = false;
    open ? collapse() : expand();
  }

  const featureMatches = $derived(entries.filter((entry) => entry.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())));
  const currentFeature = $derived(featureMatches[featureIndex] ?? null);
  const currentPageMatch = $derived(pageMatches[pageIndex] ?? null);
  const activeCount = $derived(scope === 'page' ? pageMatches.length : featureMatches.length);
  const activeIndex = $derived(scope === 'page' ? pageIndex : featureIndex);

  function clearMarker() {
    if (typeof CSS !== 'undefined' && CSS.highlights) CSS.highlights.delete('otw-page-search');
    fallbackMarker?.classList.remove('otw-page-search-fallback');
    fallbackMarker = null;
  }

  function mark(match) {
    clearMarker();
    if (!match) return;
    if (match.range && typeof CSS !== 'undefined' && CSS.highlights && typeof Highlight !== 'undefined') {
      CSS.highlights.set('otw-page-search', new Highlight(match.range));
    } else {
      match.element.classList.add('otw-page-search-fallback');
      fallbackMarker = match.element;
    }
  }

  function visible(element) {
    if (!element || element.closest('[hidden], [inert], [aria-hidden="true"], script, style, noscript, svg, canvas')) return false;
    const style = getComputedStyle(element);
    return style.visibility !== 'hidden' && style.display !== 'none' && element.getClientRects().length > 0;
  }

  function excerpt(value, position, length) {
    const start = Math.max(0, position - 28);
    const end = Math.min(value.length, position + length + 48);
    return `${start ? '…' : ''}${value.slice(start, end).replace(/\s+/g, ' ').trim()}${end < value.length ? '…' : ''}`;
  }

  // Search rendered page text and visible field values. Ranges leave Svelte's DOM
  // untouched, so even editable documents can be searched without changing them.
  function findPageMatches(container, term) {
    const needle = term.trim().toLocaleLowerCase();
    if (!needle) return [];
    const found = [];
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      const element = node.parentElement;
      if (!visible(element) || element.closest('textarea, select')) continue;
      const value = node.textContent ?? '';
      const lower = value.toLocaleLowerCase();
      let from = 0;
      while (from < lower.length) {
        const at = lower.indexOf(needle, from);
        if (at < 0) break;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + needle.length);
        if (range.getClientRects().length) found.push({ element, range, preview: excerpt(value, at, needle.length) });
        from = at + Math.max(needle.length, 1);
      }
    }
    for (const field of container.querySelectorAll('input, textarea, select')) {
      if (!visible(field) || field.type === 'password' || field.type === 'hidden' || field.type === 'file') continue;
      const value = field.tagName === 'SELECT' ? field.selectedOptions?.[0]?.textContent ?? '' : field.value ?? '';
      const lower = value.toLocaleLowerCase();
      let from = 0;
      while (from < lower.length) {
        const at = lower.indexOf(needle, from);
        if (at < 0) break;
        found.push({ element: field, range: null, preview: excerpt(value, at, needle.length) });
        from = at + Math.max(needle.length, 1);
      }
    }
    // Inline formatting can split a phrase across text nodes. In that case,
    // scroll to the smallest visible content block that contains the phrase.
    if (!found.length) {
      for (const element of container.querySelectorAll('h1, h2, h3, h4, p, li, td, th, label, button, [role="row"]')) {
        if (!visible(element)) continue;
        const value = element.textContent ?? '';
        const at = value.toLocaleLowerCase().indexOf(needle);
        if (at >= 0) found.push({ element, range: null, preview: excerpt(value, at, needle.length) });
      }
    }
    return found;
  }

  function showPageMatch(match) {
    if (!match) return;
    mark(match);
    match.element.scrollIntoView({ block: 'center', inline: 'nearest', behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
  }

  $effect(() => {
    route;
    open = false;
    clearMarker();
  });

  $effect(() => {
    if (!open || scope !== 'page' || !query.trim()) {
      pageMatches = [];
      clearMarker();
      return;
    }
    const container = document.querySelector('.module-context');
    if (!container) return;
    let timer;
    const scan = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const next = findPageMatches(container, query);
        pageMatches = next;
        pageIndex = Math.min(pageIndex, Math.max(0, next.length - 1));
        mark(next[pageIndex]);
      }, 100);
    };
    scan();
    const observer = new MutationObserver(scan);
    observer.observe(container, { subtree: true, childList: true, characterData: true });
    container.addEventListener('input', scan);
    container.addEventListener('change', scan);
    return () => {
      clearTimeout(timer);
      observer.disconnect();
      container.removeEventListener('input', scan);
      container.removeEventListener('change', scan);
      clearMarker();
    };
  });

  async function expand() {
    scope = 'page';
    query = '';
    pageIndex = 0;
    featureIndex = Math.max(0, entries.findIndex((entry) => entry.id === currentId));
    panelPlacement = null;
    open = true;
    await tick();
    placePanel();
    input?.focus();
  }

  async function collapse(returnFocus = true) {
    open = false;
    clearMarker();
    if (returnFocus) {
      await tick();
      trigger?.focus();
    }
  }

  function changeScope(next) {
    if (scope === next) return;
    scope = next;
    query = '';
    pageIndex = 0;
    featureIndex = Math.max(0, entries.findIndex((entry) => entry.id === currentId));
    clearMarker();
    input?.focus();
    tick().then(placePanel);
  }

  function step(delta) {
    if (!activeCount) return;
    if (scope === 'page') {
      pageIndex = (pageIndex + delta + pageMatches.length) % pageMatches.length;
      showPageMatch(pageMatches[pageIndex]);
    } else {
      featureIndex = (featureIndex + delta + featureMatches.length) % featureMatches.length;
    }
  }

  function activate() {
    if (scope === 'page') {
      showPageMatch(currentPageMatch);
    } else if (currentFeature) {
      collapse(false);
      goto(currentFeature.href);
    }
  }

  function onWindowPointer(e) {
    if (open && root && !root.contains(e.target)) collapse(false);
  }

  function onShortcutKey(e) {
    if (!matchMedia('(max-width: 767px)').matches || document.querySelector('[aria-modal="true"]')) return;
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(e.target?.tagName) || e.target?.isContentEditable;
    if (((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') || (e.key === '/' && !typing)) {
      e.preventDefault();
      expand();
    } else if (open && e.key === 'Escape') {
      collapse();
    }
  }
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onShortcutKey} />

<div class="mobile-shortcut" bind:this={root} style={dockStyle}>
  {#if open}
    <section bind:this={panel} class="shortcut-panel" role="search" aria-label={$t('mobile.quickBrowse')} style={panelStyle}>
      <div class="scope-row">
        <div class="scope-toggle" aria-label={$t('mobile.searchScope')}>
          <button class:active={scope === 'page'} aria-pressed={scope === 'page'} onclick={() => changeScope('page')}>{$t('mobile.thisPage')}</button>
          <button class:active={scope === 'features'} aria-pressed={scope === 'features'} onclick={() => changeScope('features')}>{$t('mobile.features')}</button>
        </div>
        <button class="close" aria-label={$t('common.close')} onclick={() => collapse()}><Icon name="x" size={16} /></button>
      </div>
      <div class="search-field">
        <Icon name="search" size={17} />
        <input bind:this={input} bind:value={query} oninput={() => { pageIndex = 0; featureIndex = 0; }}
          aria-label={scope === 'page' ? $t('mobile.searchPage') : $t('mobile.searchFeatures')}
          placeholder={scope === 'page' ? $t('mobile.searchPage') : $t('mobile.searchFeatures')}
          onkeydown={(e) => {
            if (e.key === 'Escape') { e.stopPropagation(); collapse(); }
            else if (e.key === 'Enter') { e.preventDefault(); e.shiftKey ? step(-1) : activate(); }
          }} />
      </div>
      <div class="browse-row">
        <div class="selection" aria-live="polite">
          {#if activeCount}
            <span class="count">{activeIndex + 1} / {activeCount}</span>
            <strong>{scope === 'page' ? currentPageMatch?.preview : currentFeature?.label}</strong>
          {:else}
            <span class="empty-hint">{scope === 'page' && !query.trim() ? $t('mobile.startPageSearch') : $t('search.none')}</span>
          {/if}
        </div>
        <button class="step" aria-label={scope === 'page' ? $t('mobile.previousMatch') : $t('mobile.previousFeature')}
          disabled={!activeCount} onclick={() => step(-1)}><Icon name="chevron-left" size={17} /></button>
        <button class="step" aria-label={scope === 'page' ? $t('mobile.nextMatch') : $t('mobile.nextFeature')}
          disabled={!activeCount} onclick={() => step(1)}><Icon name="chevron-right" size={17} /></button>
        <button class="activate" disabled={!activeCount} onclick={activate}>{scope === 'page' ? $t('mobile.showMatch') : $t('mobile.openFeature')}</button>
      </div>
    </section>
  {/if}
  <button bind:this={trigger} class="shortcut-trigger" aria-label={$t('mobile.quickBrowse')}
    aria-expanded={open} title={$t('mobile.dragSearch')}
    onpointerdown={onDragStart} onpointermove={onDragMove} onpointerup={onDragEnd} onpointercancel={onDragEnd}
    onclick={onTriggerClick}>
    <Icon name="search" size={18} />
  </button>
</div>

<style>
  .mobile-shortcut { display: none; }
  :global(::highlight(otw-page-search)) { background: color-mix(in srgb, var(--accent) 32%, transparent); color: var(--text); text-decoration: underline; }
  :global(.otw-page-search-fallback) { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (max-width: 767px) {
    .mobile-shortcut { display: block; position: fixed; left: max(12px, env(safe-area-inset-left)); bottom: max(12px, env(safe-area-inset-bottom)); z-index: calc(var(--z-sticky) + 2); }
    .shortcut-trigger { display: inline-flex; align-items: center; justify-content: center; width: 44px; height: 44px; border: var(--hairline) solid var(--border-control); border-radius: 12px; color: var(--text); background: var(--surface-raised); box-shadow: var(--shadow-2); cursor: grab; touch-action: none; user-select: none; }
    .shortcut-trigger:active { cursor: grabbing; }
    .shortcut-panel { position: fixed; left: 12px; bottom: 56px; width: min(318px, calc(100vw - 24px)); max-height: min(42dvh, 280px); overflow-y: auto; padding: 10px; border: var(--hairline) solid var(--border-control); border-radius: 14px; background: var(--surface-raised); box-shadow: 0 10px 30px rgb(0 0 0 / .18); }
    .scope-row { display: flex; align-items: center; gap: 6px; }
    .scope-toggle { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); flex: 1; min-width: 0; padding: 3px; border-radius: 9px; background: var(--surface-2); }
    .scope-toggle button { min-height: 38px; border: 0; border-radius: 7px; background: transparent; color: var(--muted); font-size: 12px; font-weight: var(--fw-medium); cursor: pointer; }
    .scope-toggle button.active { background: var(--surface-raised); color: var(--text); box-shadow: 0 1px 4px rgb(0 0 0 / .12); }
    .close, .step, .activate { display: inline-flex; align-items: center; justify-content: center; flex: none; min-width: 44px; min-height: 44px; border: 0; border-radius: 9px; background: transparent; color: var(--text); cursor: pointer; }
    .close { color: var(--muted); }
    .search-field { display: flex; align-items: center; gap: 8px; height: 46px; min-width: 0; margin-top: 8px; padding: 0 12px; border: var(--hairline) solid var(--border-control); border-radius: 9px; color: var(--muted); background: var(--bg); }
    .search-field:focus-within { border-color: var(--accent); outline: 1px solid var(--accent); }
    .search-field input { flex: 1; min-width: 0; width: 100%; height: 100%; padding: 0; border: 0; outline: 0; border-radius: 0; background: transparent; color: var(--text); box-shadow: none; font-size: 16px; }
    .search-field input:focus-visible { outline: 0; }
    .search-field input::placeholder { color: var(--muted); opacity: 1; }
    .browse-row { display: flex; align-items: center; gap: 2px; min-width: 0; margin-top: 8px; }
    .selection { display: flex; flex: 1; flex-direction: column; justify-content: center; min-width: 0; min-height: 44px; padding-left: 2px; }
    .selection .count { color: var(--muted); font-family: var(--mono); font-size: 10px; line-height: 1.3; }
    .selection strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); font-size: 13px; font-weight: var(--fw-medium); line-height: 1.4; }
    .selection .empty-hint { color: var(--muted); font-size: 12px; line-height: 1.3; }
    .step { border: var(--hairline) solid var(--border); }
    .activate { min-width: 52px; padding-inline: 8px; background: var(--accent); color: var(--accent-contrast); font-size: 12px; font-weight: var(--fw-medium); }
    button:disabled { opacity: .4; cursor: default; }
    button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  }
</style>
