<script>
  // `api` block: one call to the app's own API.
  //
  // The endpoint list is the MCP catalog, so what is offered here is exactly what a
  // workflow is allowed to reach. Picking an endpoint fetches its request-body schema on
  // demand: the schemas are far larger than the listing, and only one is ever needed.
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ExprField from './ExprField.svelte';
  import { automatorApi } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let { config = $bindable({}), catalog = null, sources = [] } = $props();

  let schema = $state(null);
  let schemaError = $state('');
  // The endpoint whose schema is already loaded. A GET has no body schema, so `schema`
  // stays null on success: without this marker the fetch effect would re-fire forever.
  let schemaFor = $state('');
  let bodyText = $state(config.body ? JSON.stringify(config.body, null, 2) : '');
  let bodyError = $state('');
  let schemaOpen = $state(false);

  const endpoints = $derived(catalog?.endpoints ?? []);
  const options = $derived(
    endpoints.map((e) => ({ value: `${e.method} ${e.path}`, label: `${e.method} ${e.path}` }))
  );
  const current = $derived(`${config.method ?? 'GET'} ${config.path ?? ''}`);
  const picked = $derived(
    endpoints.find((e) => e.method === config.method && e.path === config.path) ?? null
  );
  const writes = $derived(!!picked && picked.method !== 'GET' && !picked.compute);

  function pick(value) {
    const [method, ...rest] = value.split(' ');
    config.method = method;
    config.path = rest.join(' ');
  }

  async function loadSchema() {
    const key = current;
    schema = null;
    schemaError = '';
    schemaFor = key;
    if (!config.path || config.path.includes('{{')) return;
    try {
      const r = await automatorApi.schema(config.method, config.path.split('?')[0]);
      if (schemaFor === key) schema = r.schema;
    } catch (e) {
      if (schemaFor === key) schemaError = e.message;
    }
  }

  // The body is edited as text so a half-typed object never destroys what was there.
  function syncBody() {
    bodyError = '';
    const raw = bodyText.trim();
    if (!raw) {
      config.body = null;
      return;
    }
    try {
      config.body = JSON.parse(raw);
    } catch (e) {
      bodyError = e.message;
    }
  }

  // Debounced: the path is a free-text field, so a keystroke must not be a request.
  $effect(() => {
    if (current === schemaFor) return;
    const handle = setTimeout(loadSchema, 400);
    return () => clearTimeout(handle);
  });
</script>

<div class="stack">
  <Select
    label={$t('automator.api.endpoint')}
    options={options}
    value={current}
    onchange={(e) => pick(e.currentTarget.value)}
  />

  <ExprField
    label={$t('automator.api.path')}
    bind:value={config.path}
    placeholder="/api/journal/trades?limit=20"
    hint={$t('automator.api.pathHint')}
    {sources}
  />

  {#if picked}
    <p class="desc">{picked.desc}</p>
  {/if}
  {#if writes}
    <p class="warn">{$t('automator.api.writeWarn')}</p>
  {/if}

  {#if (config.method ?? 'GET') !== 'GET'}
    <div class="body">
      <div class="bodyhead">
        <span class="lbl">{$t('automator.api.body')}</span>
        {#if schema}
          <button type="button" class="link" onclick={() => (schemaOpen = !schemaOpen)}>
            {schemaOpen ? $t('automator.api.hideSchema') : $t('automator.api.showSchema')}
          </button>
        {/if}
      </div>
      <textarea bind:value={bodyText} onblur={syncBody} rows="7" spellcheck="false"></textarea>
      <p class="hint">{$t('automator.api.bodyVaultHint')}</p>
      <ErrorText error={bodyError} />
      {#if schemaOpen && schema}
        <pre class="schema">{JSON.stringify(schema, null, 2)}</pre>
      {/if}
      <ErrorText error={schemaError} />
    </div>
  {/if}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .desc {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .warn {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--amber-ink, var(--amber));
  }
  .bodyhead {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-1);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .hint {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .link {
    background: none;
    border: 0;
    color: var(--accent);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: 0;
  }
  textarea {
    width: 100%;
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .schema {
    max-height: 200px;
    overflow: auto;
    background: var(--surface-2);
    padding: var(--space-2);
    font-size: 10px;
    margin: var(--space-2) 0 0;
  }
</style>
