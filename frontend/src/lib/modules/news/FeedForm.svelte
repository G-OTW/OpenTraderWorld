<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { newsApi } from './api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

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

  // Request quota (API feeds): config.rate_limit = { max: n|null, period } when tracked.
  // Absent = tracking off; max null = unlimited (usage still counted and displayed).
  const rl = feed?.config?.rate_limit;
  let rlTrack = $state(!!rl);
  let rlUnlimited = $state(!!rl && rl.max == null);
  let rlMax = $state(rl?.max ?? '');
  let rlPeriod = $state(rl?.period ?? 'day');

  let error = $state('');
  let saving = $state(false);

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
    const config = {
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
    if (rlTrack) {
      config.rate_limit = { max: rlUnlimited ? null : Number(rlMax), period: rlPeriod };
    }
    return config;
  }

  // When the backend reports an identical source, we surface this dialog instead
  // of a native confirm() so the user can explicitly reuse it or create a copy.
  let duplicate = $state(null); // { feed_id, feed_name, dashboard_names }

  async function save() {
    error = '';
    if (kind === 'api' && rlTrack && !rlUnlimited && !(Number(rlMax) >= 1)) {
      error = $t('news.feedForm.rateLimitErr');
      return;
    }
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
    <p class="vault-tip"><Icon name="lock" size={12} /> {$t('news.feedForm.vaultTip')}</p>
    <label class="row">
      <span>{$t('news.feedForm.endpoint')}</span>
      <input bind:value={apiUrl} placeholder="https://api.example.com/news?apikey={'{{AlphaVantage.apikey}}'}" />
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
          <input placeholder="Bearer {'{{MyService.apikey}}'}" bind:value={h.v} />
        </div>
      {/each}
      <button class="add-kv" onclick={() => (headers = addPair(headers))}>{$t('news.feedForm.addHeader')}</button>
    </div>

    <div class="kv-block">
      <div class="kv-head">{$t('news.feedForm.queryParams')}</div>
      {#each query as p, i}
        <div class="kv">
          <input placeholder="apiKey" bind:value={p.k} />
          <input placeholder="{'{{AlphaVantage.apikey}}'}" bind:value={p.v} />
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

    <div class="section">{$t('news.feedForm.rateLimit')} <small>{$t('news.feedForm.rateLimitHint')}</small></div>
    <div class="rl">
      <label class="chk">
        <input type="checkbox" bind:checked={rlTrack} />
        {$t('news.feedForm.trackUsage')}
      </label>
      {#if rlTrack}
        <div class="rl-limit">
          <label class="chk">
            <input type="checkbox" bind:checked={rlUnlimited} />
            {$t('news.feedForm.unlimited')}
          </label>
          {#if !rlUnlimited}
            <input class="rl-max" type="number" min="1" bind:value={rlMax} placeholder="25" aria-label={$t('news.feedForm.maxRequests')} />
          {/if}
          <span class="rl-per">{$t('news.feedForm.per')}</span>
          <select class="rl-period" bind:value={rlPeriod}>
            {#each ['minute', 'hour', 'day', 'week', 'month'] as p (p)}
              <option value={p}>{$t(`common.period.${p}`)}</option>
            {/each}
          </select>
        </div>
      {/if}
    </div>
  {/if}

  <ErrorText error={error} copyable />

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
    font-size: var(--text-sm);
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
    font-size: var(--text-sm);
  }
  .interval input {
    width: 90px;
  }
  .section {
    margin-top: 8px;
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    border-bottom: 1px solid var(--border);
    padding-bottom: 4px;
  }
  .section small,
  .kv-head small {
    color: var(--muted);
    font-weight: var(--fw-normal);
  }
  .kv-block {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .kv-head {
    color: var(--muted);
    font-size: var(--text-sm);
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
    font-size: var(--text-sm);
    padding: 3px 8px;
  }
  .add-kv:hover {
    color: var(--text);
  }
  .vault-tip {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
    padding: 6px 8px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.4;
  }
  .vault-tip :global(svg) {
    flex-shrink: 0;
    margin-top: 2px;
  }
  .rl {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .rl-limit {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: nowrap;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
  }
  .chk input {
    width: auto;
    margin: 0;
  }
  .rl .rl-max {
    flex: 0 0 72px;
    width: 72px;
  }
  .rl .rl-period {
    flex: 0 0 auto;
    width: auto;
    min-width: 96px;
  }
  .rl-per {
    color: var(--muted);
  }
  .dup-msg {
    color: var(--text);
    font-size: var(--text-base);
    line-height: 1.5;
    margin: 0 0 8px;
  }
  .dup-msg:last-of-type {
    margin-bottom: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
</style>
