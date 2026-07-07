<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { newsApi } from './api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  // props: feed (null = create), dashboardId (target for new sources),
  // onsaved(feed), oncancel()
  let { feed = null, dashboardId = null, onsaved = () => {}, oncancel = () => {} } = $props();

  const editing = $derived(!!feed);

  // Core fields
  let name = $state(feed?.name ?? '');
  let kind = $state(feed?.kind ?? 'rss');
  let intervalSecs = $state(feed?.interval_secs ?? 900);
  let enabled = $state(feed?.enabled ?? true);

  // RSS config
  let rssUrl = $state(feed?.config?.url ?? '');

  // API config (with GUI-mapped JSON paths)
  let apiUrl = $state(feed?.config?.url ?? '');
  let apiMethod = $state(feed?.config?.method ?? 'GET');
  let itemsPath = $state(feed?.config?.items_path ?? '');
  let titlePath = $state(feed?.config?.title_path ?? 'title');
  let urlPath = $state(feed?.config?.url_path ?? 'url');
  let datePath = $state(feed?.config?.date_path ?? '');
  let summaryPath = $state(feed?.config?.summary_path ?? '');
  let idPath = $state(feed?.config?.id_path ?? '');

  // Headers & query as editable key/value lists.
  let headers = $state(objToPairs(feed?.config?.headers));
  let query = $state(objToPairs(feed?.config?.query));

  // Secrets: existing names (values never loaded) + a draft to add.
  let secretNames = $state([]);
  let newSecretName = $state('');
  let newSecretValue = $state('');

  let error = $state('');
  let saving = $state(false);

  $effect(() => {
    if (feed?.id) newsApi.listSecrets(feed.id).then((n) => (secretNames = n)).catch(() => {});
  });

  function objToPairs(obj) {
    if (!obj) return [{ k: '', v: '' }];
    const pairs = Object.entries(obj).map(([k, v]) => ({ k, v: String(v) }));
    return pairs.length ? pairs : [{ k: '', v: '' }];
  }
  function pairsToObj(pairs) {
    const o = {};
    for (const { k, v } of pairs) if (k.trim()) o[k.trim()] = v;
    return o;
  }
  function addPair(list) {
    return [...list, { k: '', v: '' }];
  }

  function buildConfig() {
    if (kind === 'rss') return { url: rssUrl.trim() };
    return {
      url: apiUrl.trim(),
      method: apiMethod,
      headers: pairsToObj(headers),
      query: pairsToObj(query),
      items_path: itemsPath.trim(),
      title_path: titlePath.trim(),
      url_path: urlPath.trim(),
      date_path: datePath.trim(),
      summary_path: summaryPath.trim(),
      id_path: idPath.trim()
    };
  }

  // When the backend reports an identical source, we surface this dialog instead
  // of a native confirm() so the user can explicitly reuse it or create a copy.
  let duplicate = $state(null); // { feed_id, feed_name, dashboard_names }

  async function save() {
    error = '';
    saving = true;
    try {
      const config = buildConfig();
      const interval_secs = Number(intervalSecs);
      let saved;
      if (editing) {
        await newsApi.updateFeed(feed.id, { name, config, enabled, interval_secs });
        saved = { ...feed, name, config, enabled, interval_secs };
      } else if (dashboardId) {
        // New source goes into the current dashboard. The backend dedup-checks:
        // if an identical source exists, it returns { duplicate } and we open
        // the reuse/copy dialog rather than blindly creating a duplicate.
        const res = await newsApi.addDashboardSource(dashboardId, { name, kind, config, interval_secs });
        if (res.duplicate) {
          duplicate = res.duplicate;
          saving = false;
          return; // resolved by reuseExisting() / createCopy()
        }
        saved = res.feed ?? { id: res.feed_id, name, kind, config, interval_secs };
      } else {
        saved = await newsApi.createFeed({ name, kind, config, interval_secs });
      }
      onsaved(saved);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Link the already-existing source into this dashboard (no duplicate items).
  async function reuseExisting() {
    saving = true;
    try {
      const interval_secs = Number(intervalSecs);
      const res = await newsApi.addDashboardSource(dashboardId, { feed_id: duplicate.feed_id, interval_secs });
      duplicate = null;
      onsaved(res.feed ?? { id: res.feed_id });
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
  // Create a separate copy of the source anyway.
  async function createCopy() {
    saving = true;
    try {
      const config = buildConfig();
      const interval_secs = Number(intervalSecs);
      const res = await newsApi.addDashboardSource(dashboardId, { name, kind, config, interval_secs, force: true });
      duplicate = null;
      onsaved(res.feed ?? { id: res.feed_id, name, kind, config, interval_secs });
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function addSecret() {
    if (!feed?.id || !newSecretName.trim()) return;
    try {
      await newsApi.setSecret(feed.id, newSecretName.trim(), newSecretValue);
      secretNames = await newsApi.listSecrets(feed.id);
      newSecretName = '';
      newSecretValue = '';
    } catch (e) {
      error = e.message;
    }
  }
  async function removeSecret(nm) {
    await newsApi.deleteSecret(feed.id, nm);
    secretNames = secretNames.filter((s) => s !== nm);
  }
</script>

<div class="feed-form">
  <label class="row">
    <span>{$t('news.feedForm.name')}</span>
    <input bind:value={name} placeholder={$t('news.feedForm.namePlaceholder')} />
  </label>

  <label class="row">
    <span>{$t('news.feedForm.type')}</span>
    {#if editing}
      <input value={kind} disabled />
    {:else}
      <select bind:value={kind}>
        <option value="rss">{$t('news.feedForm.typeRss')}</option>
        <option value="api">{$t('news.feedForm.typeApi')}</option>
      </select>
    {/if}
  </label>

  <label class="row">
    <span>{$t('news.feedForm.pollEvery')}</span>
    <span class="interval">
      <input type="number" min="30" bind:value={intervalSecs} /> {$t('news.feedForm.seconds')}
    </span>
  </label>

  {#if editing}
    <label class="row">
      <span>{$t('news.feedForm.enabled')}</span>
      <input type="checkbox" bind:checked={enabled} />
    </label>
  {/if}

  {#if kind === 'rss'}
    <label class="row">
      <span>{$t('news.feedForm.feedUrl')}</span>
      <input bind:value={rssUrl} placeholder="https://example.com/rss.xml" />
    </label>
  {:else}
    <div class="section">{$t('news.feedForm.request')}</div>
    <label class="row">
      <span>{$t('news.feedForm.endpoint')}</span>
      <input bind:value={apiUrl} placeholder="https://api.example.com/news" />
    </label>
    <label class="row">
      <span>{$t('news.feedForm.method')}</span>
      <select bind:value={apiMethod}>
        <option>GET</option>
        <option>POST</option>
      </select>
    </label>

    <div class="kv-block">
      <div class="kv-head">{$t('news.feedForm.headers')} <small>{$t('news.feedForm.headersHint')}</small></div>
      {#each headers as h, i}
        <div class="kv">
          <input placeholder="Authorization" bind:value={h.k} />
          <input placeholder="Bearer {'{{secret:token}}'}" bind:value={h.v} />
        </div>
      {/each}
      <button class="add-kv" onclick={() => (headers = addPair(headers))}>{$t('news.feedForm.addHeader')}</button>
    </div>

    <div class="kv-block">
      <div class="kv-head">{$t('news.feedForm.queryParams')}</div>
      {#each query as p, i}
        <div class="kv">
          <input placeholder="apiKey" bind:value={p.k} />
          <input placeholder="{'{{secret:api_key}}'}" bind:value={p.v} />
        </div>
      {/each}
      <button class="add-kv" onclick={() => (query = addPair(query))}>{$t('news.feedForm.addParam')}</button>
    </div>

    <div class="section">{$t('news.feedForm.fieldMapping')} <small>{$t('news.feedForm.fieldMappingHint')}</small></div>
    <label class="row"><span>{$t('news.feedForm.itemsPath')}</span><input bind:value={itemsPath} placeholder={$t('news.feedForm.itemsPathPlaceholder')} /></label>
    <label class="row"><span>{$t('news.feedForm.titlePath')}</span><input bind:value={titlePath} placeholder="title" /></label>
    <label class="row"><span>{$t('news.feedForm.urlPath')}</span><input bind:value={urlPath} placeholder="url" /></label>
    <label class="row"><span>{$t('news.feedForm.datePath')}</span><input bind:value={datePath} placeholder={$t('news.feedForm.datePathPlaceholder')} /></label>
    <label class="row"><span>{$t('news.feedForm.summaryPath')}</span><input bind:value={summaryPath} placeholder={$t('news.feedForm.optional')} /></label>
    <label class="row"><span>{$t('news.feedForm.idPath')}</span><input bind:value={idPath} placeholder={$t('news.feedForm.idPathPlaceholder')} /></label>
  {/if}

  {#if editing}
    <div class="section">{$t('news.feedForm.secrets')} <small>{$t('news.feedForm.secretsHint')}</small></div>
    {#if secretNames.length}
      <div class="secrets">
        {#each secretNames as nm}
          <span class="secret-chip"><Icon name="check-circle" size={11} /> {nm}<button onclick={() => removeSecret(nm)} title={$t('news.feedForm.removeSecret')}><Icon name="x" size={13} /></button></span>
        {/each}
      </div>
    {/if}
    <div class="kv">
      <input placeholder={$t('news.feedForm.secretNamePlaceholder')} bind:value={newSecretName} />
      <input type="password" placeholder={$t('news.feedForm.secretValuePlaceholder')} bind:value={newSecretValue} />
      <button class="add-kv" onclick={addSecret} disabled={!newSecretName.trim()}>{$t('common.save')}</button>
    </div>
  {:else}
    <p class="hint">{$t('news.feedForm.saveFirstHint')}</p>
  {/if}

  {#if error}<p class="error" title="click to copy" use:copyLog={error}>{error}</p>{/if}

  <div class="actions">
    <button class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button class="primary" onclick={save} disabled={saving}>{saving ? $t('common.saving') : $t('news.feedForm.saveFeed')}</button>
  </div>
</div>

{#if duplicate}
  <Modal open title={$t('news.feedForm.dupTitle')} onclose={() => (duplicate = null)}>
    <p class="dup-msg">
      {#if duplicate.dashboard_names?.length}
        {$t('news.feedForm.dupMsgIn', { names: duplicate.dashboard_names.join(', ') })}
      {:else}
        {$t('news.feedForm.dupMsg')}
      {/if}
    </p>
    <p class="dup-msg">
      {$t('news.feedForm.dupQuestion')}
    </p>
    {#snippet footer()}
      <button class="ghost" onclick={() => (duplicate = null)} disabled={saving}>{$t('common.cancel')}</button>
      <button class="ghost" onclick={createCopy} disabled={saving}>{$t('news.feedForm.createCopy')}</button>
      <button class="primary" onclick={reuseExisting} disabled={saving}>{$t('news.feedForm.reuseExisting')}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .feed-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: grid;
    grid-template-columns: 110px 1fr;
    align-items: center;
    gap: 10px;
  }
  .row > span {
    color: var(--muted);
    font-size: 0.82rem;
  }
  input,
  select {
    width: 100%;
  }
  .interval {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .interval input {
    width: 90px;
  }
  .section {
    margin-top: 8px;
    color: var(--text);
    font-size: 0.8rem;
    font-weight: 600;
    border-bottom: 1px solid var(--border);
    padding-bottom: 4px;
  }
  .section small,
  .kv-head small {
    color: var(--muted);
    font-weight: 400;
  }
  .kv-block {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .kv-head {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .kv {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .add-kv {
    align-self: flex-start;
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.78rem;
    padding: 3px 8px;
  }
  .add-kv:hover {
    color: var(--text);
  }
  .secrets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .secret-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--surface-2);
    border-radius: 12px;
    color: var(--text);
    font-size: 0.78rem;
    padding: 2px 8px;
  }
  .secret-chip button {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.7rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .dup-msg {
    color: var(--text);
    font-size: 0.88rem;
    line-height: 1.5;
    margin: 0 0 8px;
  }
  .dup-msg:last-of-type {
    margin-bottom: 0;
  }
  .error {
    color: var(--red, #ef4444);
    font-size: 0.82rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
</style>
