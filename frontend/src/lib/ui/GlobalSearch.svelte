<script>
  // Global top-bar search. Default scope: module names, Resources items, Settings
  // sections. The layers toggle widens it to content titles (Editor pages, Goals,
  // Calendar events, ToDos, Routines, Reminders, Prompts, Community docs) — titles
  // only, never bodies. Modules + settings match client-side; everything DB-backed
  // goes through GET /api/search. Results render as one flat list ordered by type,
  // each row showing the name with the match emphasized and the type below.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { goto } from '$app/navigation';
  import { featureModules } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';
  import { redirectIfUnauthorized } from '$lib/auth.js';

  // Settings rail sections (mirrors /settings/+page.svelte); hash deep-links there.
  const SETTING_SECTIONS = [
    'account', 'defaults', 'appearance', 'network', 'modules', 'data', 'backup',
    'update', 'logs', 'rate', 'mcp', 'credits', 'about'
  ];

  // Backend scope key → owning module (gates the query) and click-through route.
  // Order here is the display order of the deep-scope groups.
  const DEEP_SCOPES = [
    { scope: 'documents', module: 'editor', base: '/editor' },
    { scope: 'goals', module: 'goals', base: '/goals' },
    { scope: 'events', module: 'calendar', base: '/calendar' },
    { scope: 'todos', module: 'todos', base: '/todos' },
    { scope: 'routines', module: 'routines', base: '/routines' },
    { scope: 'reminders', module: 'remindme', base: '/remindme' },
    { scope: 'prompts', module: 'prompt-store', base: '/prompt-store' },
    { scope: 'community-docs', module: 'community-docs', base: '/community-docs' }
  ];
  const DEEP_KEY = 'otw.search.deep';

  let q = $state('');
  let open = $state(false);
  let deep = $state(
    typeof localStorage !== 'undefined' && localStorage.getItem(DEEP_KEY) === '1'
  );
  let serverHits = $state([]);
  let loading = $state(false);
  let cursor = $state(0);
  let inputEl = $state();
  let rootEl = $state();

  const isMac =
    typeof navigator !== 'undefined' && /Mac|iP(hone|ad|od)/.test(navigator.platform);

  function toggleDeep() {
    deep = !deep;
    localStorage.setItem(DEEP_KEY, deep ? '1' : '0');
    inputEl?.focus();
  }

  const nq = $derived(q.trim().toLowerCase());

  // Client-side scopes: installed feature modules by name, settings sections by label.
  const moduleHits = $derived.by(() => {
    if (!nq) return [];
    return featureModules
      .filter((m) => ($installedIds ? $installedIds.has(m.id) : true))
      .filter((m) => m.name.toLowerCase().includes(nq))
      .slice(0, 8)
      .map((m) => ({ type: 'module', name: m.name, target: m.base, icon: m.icon }));
  });
  const settingHits = $derived.by(() => {
    if (!nq) return [];
    return SETTING_SECTIONS.map((id) => ({ id, label: $t(`settings.nav.${id}`) }))
      .filter((s) => s.label.toLowerCase().includes(nq))
      .slice(0, 8)
      .map((s) => ({ type: 'setting', name: s.label, target: `/settings#${s.id}` }));
  });

  // Server-side scopes, gated to installed modules; deep ones only when toggled on.
  const serverScopes = $derived.by(() => {
    const has = (id) => ($installedIds ? $installedIds.has(id) : true);
    const scopes = has('resources') ? ['resources'] : [];
    if (deep) for (const d of DEEP_SCOPES) if (has(d.module)) scopes.push(d.scope);
    return scopes;
  });

  // Debounced fetch; a sequence counter drops stale responses.
  let seq = 0;
  $effect(() => {
    const query = nq;
    const scopes = serverScopes.join(',');
    const my = ++seq;
    if (!query || !scopes) {
      serverHits = [];
      loading = false;
      return;
    }
    loading = true;
    const timer = setTimeout(async () => {
      try {
        const res = await fetch(
          `/api/search?q=${encodeURIComponent(query)}&scopes=${encodeURIComponent(scopes)}`,
          { headers: { 'content-type': 'application/json' } }
        );
        redirectIfUnauthorized(res);
        const body = res.ok ? await res.json() : { hits: [] };
        if (my === seq) serverHits = body.hits ?? [];
      } catch {
        if (my === seq) serverHits = [];
      } finally {
        if (my === seq) loading = false;
      }
    }, 200);
    return () => clearTimeout(timer);
  });

  // One flat list, ordered by type: Modules, Settings, Resources, then deep scopes.
  const items = $derived.by(() => {
    const scopeBase = Object.fromEntries(DEEP_SCOPES.map((d) => [d.scope, d.base]));
    scopeBase['resources'] = '/resources';
    const order = ['resources', ...DEEP_SCOPES.map((d) => d.scope)];
    const server = [...serverHits]
      .sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind))
      .map((h) => ({ type: h.kind, name: h.name, sub: h.sub, target: scopeBase[h.kind] }))
      .filter((h) => h.target);
    return [...moduleHits, ...settingHits, ...server];
  });

  $effect(() => {
    items;
    cursor = 0;
  });

  const showPanel = $derived(open && nq.length > 0);

  function pick(item) {
    open = false;
    q = '';
    inputEl?.blur();
    goto(item.target);
  }

  // Keep the keyboard-highlighted row visible inside the scrollable list.
  function revealCursor() {
    requestAnimationFrame(() =>
      document.getElementById(`gsearch-opt-${cursor}`)?.scrollIntoView({ block: 'nearest' })
    );
  }

  function onKey(e) {
    if (e.key === 'Escape') {
      open = false;
      inputEl?.blur();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (items.length) {
        cursor = cursor >= items.length - 1 ? 0 : cursor + 1;
        revealCursor();
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (items.length) {
        cursor = cursor <= 0 ? items.length - 1 : cursor - 1;
        revealCursor();
      }
    } else if (e.key === 'Enter' && items[cursor]) {
      pick(items[cursor]);
    }
  }

  // ⌘K / Ctrl+K focuses the search from anywhere; "/" too, outside editable fields.
  function onWindowKey(e) {
    const inField =
      /^(INPUT|TEXTAREA|SELECT)$/.test(e.target?.tagName) || e.target?.isContentEditable;
    if (((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') || (e.key === '/' && !inField)) {
      e.preventDefault();
      inputEl?.focus();
      open = true;
    }
  }

  function onWindowClick(e) {
    if (rootEl && !rootEl.contains(e.target)) open = false;
  }

  // Emphasize the matched substring inside a hit name.
  function split(name) {
    const i = name.toLowerCase().indexOf(nq);
    if (i < 0 || !nq) return [name, '', ''];
    return [name.slice(0, i), name.slice(i, i + nq.length), name.slice(i + nq.length)];
  }
</script>

<svelte:window on:keydown={onWindowKey} on:click={onWindowClick} />

<div class="gsearch" bind:this={rootEl}>
  <!-- The input is the only visible box; the action buttons overlay it. -->
  <input
    class="gs-input"
    bind:this={inputEl}
    bind:value={q}
    placeholder={$t('search.placeholder')}
    role="combobox"
    aria-expanded={showPanel}
    aria-controls="gsearch-list"
    aria-activedescendant={items[cursor] ? `gsearch-opt-${cursor}` : undefined}
    onfocus={() => (open = true)}
    onkeydown={onKey}
  />
  <div class="gs-actions">
    {#if q}
      <button class="gs-mini" title={$t('search.clear')} aria-label={$t('search.clear')}
        onclick={() => { q = ''; inputEl?.focus(); }}>
        <Icon name="x" size={13} />
      </button>
    {:else}
      <kbd class="gs-kbd">{isMac ? '⌘K' : 'Ctrl K'}</kbd>
    {/if}
    <button
      class="gs-mini gs-deep"
      class:on={deep}
      title={$t('search.deep')}
      aria-label={$t('search.deep')}
      aria-pressed={deep}
      onclick={toggleDeep}
    >
      <Icon name="layers" size={14} />
    </button>
  </div>

  {#if showPanel}
    <div class="gs-panel">
      <ul id="gsearch-list" role="listbox">
        {#each items as item, i (item.type + item.target + item.name + i)}
          {@const [pre, hit, post] = split(item.name)}
          <li>
            <button
              id="gsearch-opt-{i}"
              class="gs-row"
              class:cursor={i === cursor}
              role="option"
              aria-selected={i === cursor}
              tabindex="-1"
              onclick={() => pick(item)}
              onmouseenter={() => (cursor = i)}
            >
              <span class="gs-name">{pre}<em>{hit}</em>{post}</span>
              <span class="gs-kind">
                {$t(`search.type.${item.type}`)}{item.sub ? ` · ${item.sub}` : ''}
              </span>
            </button>
          </li>
        {/each}
        {#if items.length === 0}
          <li class="gs-none">{loading ? $t('search.searching') : $t('search.none')}</li>
        {/if}
      </ul>
      {#if !deep}
        <button class="gs-hint" onclick={toggleDeep}>
          <Icon name="layers" size={13} />
          {$t('search.deepHint')}
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .gsearch {
    position: relative;
    width: 100%;
    max-width: 460px;
  }

  /* Single visible box: the input itself. Overrides the element-level control
     styling (height/radius/padding) and leaves room for the overlaid controls. */
  .gs-input {
    width: 100%;
    height: 32px;
    border-radius: 999px;
    padding: 0 72px 0 var(--space-4);
    font-size: var(--text-sm);
  }

  .gs-actions {
    position: absolute;
    right: 5px;
    top: 16px;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .gs-kbd {
    padding: 1px 6px;
    margin-right: 2px;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--muted);
    font-family: inherit;
    font-size: var(--text-xs);
    line-height: 1.4;
  }

  .gs-mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .gs-mini:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }
  /* The deep toggle is the only accent spend in the field: on = wider scope. */
  .gs-mini.gs-deep.on {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .gs-panel {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    z-index: var(--z-dropdown);
    padding: var(--space-1);
    background: var(--surface);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-2);
  }
  /* The list scrolls inside a fixed cap; the panel grows only up to it. */
  .gs-panel ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 320px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .gs-row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }
  .gs-row.cursor {
    background: var(--surface-2);
  }
  .gs-name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .gs-name em {
    font-style: normal;
    color: var(--accent);
  }
  .gs-kind {
    font-size: var(--text-xs);
    color: var(--muted);
  }

  .gs-none {
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: var(--text-xs);
  }

  /* Footer nudge toward the wider scope, styled as ambient help, not a result row. */
  .gs-hint {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    margin-top: var(--space-1);
    padding: var(--space-2) var(--space-3);
    border: none;
    border-top: 1px solid var(--border);
    border-radius: 0 0 var(--radius) var(--radius);
    background: transparent;
    color: var(--muted);
    font-family: inherit;
    font-size: var(--text-xs);
    text-align: left;
    cursor: pointer;
  }
  .gs-hint:hover {
    color: var(--text);
  }
</style>
