<script>
  // The block reference rail: the native block types on top, then every endpoint of the
  // app a workflow is allowed to call, grouped by module and searchable.
  //
  // The endpoint list is generated from the same allowlist the MCP gateway uses, so it can
  // only ever offer what a workflow is actually permitted to reach.
  import Icon from '$lib/ui/Icon.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { KIND_META, KIND_ORDER } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let { catalog = null, onadd = () => {} } = $props();

  let search = $state('');
  let openModule = $state('');

  const query = $derived(search.trim().toLowerCase());

  const kinds = $derived(
    KIND_ORDER.filter(
      (k) =>
        !query ||
        k.includes(query) ||
        $t(`automator.kind.${k}`).toLowerCase().includes(query) ||
        $t(`automator.kind.${k}.desc`).toLowerCase().includes(query)
    )
  );

  const modules = $derived.by(() => {
    const endpoints = catalog?.endpoints ?? [];
    const labels = new Map((catalog?.modules ?? []).map((m) => [m.id, m.label]));
    const groups = new Map();
    for (const e of endpoints) {
      if (query && !`${e.method} ${e.path} ${e.desc}`.toLowerCase().includes(query)) continue;
      if (!groups.has(e.module)) groups.set(e.module, []);
      groups.get(e.module).push(e);
    }
    return [...groups.entries()]
      .map(([id, list]) => ({ id, label: labels.get(id) ?? id, endpoints: list }))
      .sort((a, b) => a.label.localeCompare(b.label));
  });

  function dragKind(ev, kind) {
    ev.dataTransfer.setData('text/otw-block', kind);
    ev.dataTransfer.effectAllowed = 'copy';
  }

  // An endpoint drops as an `api` block already pointed at it: the whole reason the
  // catalog is in the rail rather than buried in the block form.
  function dragEndpoint(ev, endpoint) {
    ev.dataTransfer.setData('text/otw-block', 'api');
    ev.dataTransfer.setData('text/otw-endpoint', JSON.stringify(endpoint));
    ev.dataTransfer.effectAllowed = 'copy';
  }
</script>

<aside class="rail">
  <div class="search">
    <Input bind:value={search} placeholder={$t('automator.palette.search')} />
  </div>

  <div class="scroll">
    <p class="grouptitle">{$t('automator.palette.blocks')}</p>
    <ul class="blocks">
      {#each kinds as kind (kind)}
        <li>
          <button
            class="block"
            draggable="true"
            ondragstart={(e) => dragKind(e, kind)}
            onclick={() => onadd(kind)}
            title={$t(`automator.kind.${kind}.desc`)}
          >
            <Icon name={KIND_META[kind].icon} size={14} />
            <span>
              <strong>{$t(`automator.kind.${kind}`)}</strong>
              <em>{$t(`automator.kind.${kind}.desc`)}</em>
            </span>
          </button>
        </li>
      {/each}
    </ul>

    <p class="grouptitle">{$t('automator.palette.endpoints')}</p>
    {#if !catalog}
      <p class="empty">{$t('common.loading')}</p>
    {:else if !modules.length}
      <p class="empty">{$t('automator.palette.noMatch')}</p>
    {:else}
      {#each modules as mod (mod.id)}
        <section class="mod">
          <button
            class="modhead"
            onclick={() => (openModule = openModule === mod.id ? '' : mod.id)}
            aria-expanded={openModule === mod.id || !!query}
          >
            <Icon name={openModule === mod.id || query ? 'chevron-down' : 'chevron-right'} size={12} />
            {mod.label}
            <span class="count">{mod.endpoints.length}</span>
          </button>
          {#if openModule === mod.id || query}
            <ul class="eps">
              {#each mod.endpoints as e (e.method + e.path)}
                <li>
                  <button
                    class="ep"
                    draggable="true"
                    ondragstart={(ev) => dragEndpoint(ev, e)}
                    onclick={() => onadd('api', e)}
                    title={e.desc}
                  >
                    <span class="verb {e.method.toLowerCase()}">{e.method}</span>
                    <span class="path">{e.path}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .rail {
    width: 268px;
    flex: none;
    border-left: 0.5px solid var(--border);
    display: flex;
    flex-direction: column;
    background: var(--surface);
    min-height: 0;
  }
  .search {
    padding: var(--space-2);
    border-bottom: 0.5px solid var(--border);
  }
  .scroll {
    overflow-y: auto;
    padding: var(--space-2) 0 var(--space-6);
    flex: 1;
  }
  .grouptitle {
    margin: var(--space-3) var(--space-3) var(--space-1);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .block {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: none;
    border: 0;
    text-align: left;
    color: var(--text);
    cursor: grab;
  }
  .block:hover,
  .ep:hover,
  .modhead:hover {
    background: var(--surface-2);
  }
  .block span {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .block strong {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .block em {
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .modhead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: none;
    border: 0;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .count {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .ep {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 3px var(--space-3) 3px var(--space-6);
    background: none;
    border: 0;
    text-align: left;
    cursor: grab;
  }
  .verb {
    font-size: 9px;
    font-family: var(--mono);
    color: var(--dim);
    width: 34px;
    flex: none;
  }
  .verb.get {
    color: var(--green);
  }
  .verb.delete {
    color: var(--red);
  }
  .path {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    margin: 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
