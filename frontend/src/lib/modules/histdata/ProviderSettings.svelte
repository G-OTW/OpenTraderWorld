<script>
  import Icon from '$lib/ui/Icon.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import UsagePie from './UsagePie.svelte';
  // Connector manager. A connector is a named instance of a provider — several instances
  // of the same provider can coexist, each with its own credentials and optional request
  // limit. Credentials are write-only: we only ever know which secret names are set.
  // Master/detail: left pane lists connectors, right pane edits the selected one.
  import { histdataApi } from './api.js';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // `scope` namespaces the connector list per owning module ('histdata' | 'watchlists');
  // creations land in that namespace and the selection memory is kept per scope.
  let { connectors = [], providers = [], scope = 'histdata', onchanged } = $props();

  // Remember which connector was open across refresh (per-browser).
  const SEL_KEY = `otw.${scope}.connector.v1`;
  let selectedId = $state(
    (() => {
      try {
        return localStorage.getItem(SEL_KEY);
      } catch {
        return null;
      }
    })()
  );
  $effect(() => {
    try {
      if (selectedId) localStorage.setItem(SEL_KEY, selectedId);
    } catch {
      /* non-fatal */
    }
  });
  const selected = $derived(connectors.find((c) => c.id === selectedId) ?? connectors[0] ?? null);

  // A connector is "ready" if keyless or all required secrets are set.
  const ready = (c) => !c.required_secrets.length || c.required_secrets.every((n) => c.set_secrets.includes(n));

  let error = $state('');

  // ── Add connector ──
  let adding = $state(false);
  let addProvider = $state('');
  let addName = $state('');
  let addTrack = $state(false);
  let addUnlimited = $state(false);
  let addMax = $state('');
  let addPeriod = $state('day');
  // Vault items picked for the provider's required secrets, keyed by secret name —
  // the keys are entered in the same form and saved right after creation.
  let addVault = $state({});
  const addCap = $derived(providers.find((x) => x.provider === addProvider) ?? null);

  // Prefill the name with the provider label (edit freely afterwards).
  function onAddProviderChange() {
    const p = providers.find((x) => x.provider === addProvider);
    if (p && !addName.trim()) addName = p.label;
  }
  function limitBody(track, unlimited, max, period) {
    if (!track) return { enabled: false, max_requests: null, period: 'day' };
    return { enabled: true, max_requests: unlimited ? null : Number(max), period };
  }
  async function createConnector() {
    error = '';
    if (!addProvider || !addName.trim()) return;
    if (addTrack && !addUnlimited && !(Number(addMax) >= 1)) {
      error = $t('histdata.connectors.errMax');
      return;
    }
    try {
      const c = await histdataApi.createConnector({
        provider: addProvider,
        name: addName.trim(),
        scope,
        limit: limitBody(addTrack, addUnlimited, addMax, addPeriod)
      });
      // Keys picked in the form land on the fresh connector in the same flow.
      for (const [secretName, item] of Object.entries(addVault)) {
        if (item) await histdataApi.setSecret(c.id, secretName, '', item);
      }
      adding = false;
      addProvider = '';
      addName = '';
      addTrack = false;
      addUnlimited = false;
      addMax = '';
      addVault = {};
      selectedId = c.id;
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Rename + limit editing (re-seeded whenever the selection changes) ──
  let nameDraft = $state('');
  let limTrack = $state(false);
  let limUnlimited = $state(false);
  let limMax = $state('');
  let limPeriod = $state('day');
  $effect(() => {
    const c = selected;
    nameDraft = c?.name ?? '';
    limTrack = !!c?.quota;
    limUnlimited = !!c?.quota && c.quota.max_requests == null;
    limMax = c?.quota?.max_requests ?? '';
    limPeriod = c?.quota?.period ?? 'day';
  });

  async function rename() {
    error = '';
    const nm = nameDraft.trim();
    if (!selected || !nm || nm === selected.name) return;
    try {
      await histdataApi.updateConnector(selected.id, { name: nm });
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }
  async function saveLimit() {
    error = '';
    if (!selected) return;
    if (limTrack && !limUnlimited && !(Number(limMax) >= 1)) {
      error = $t('histdata.connectors.errMax');
      return;
    }
    try {
      await histdataApi.updateConnector(selected.id, {
        limit: limitBody(limTrack, limUnlimited, limMax, limPeriod)
      });
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Delete ──
  let confirmOpen = $state(false);
  async function removeConnector() {
    if (!selected) return;
    try {
      await histdataApi.deleteConnector(selected.id);
      selectedId = null;
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Secrets: the picked vault item per "connectorId:secretName" ──
  // Entries are created lazily on first pick, so a key can be missing at render time.
  // VaultPicker's bindable prop therefore declares no fallback (binding `undefined` to a
  // prop that has one throws props_invalid_value); we normalize with ?? when reading.
  let vaultDrafts = $state({});
  async function saveSecret(id, name) {
    const key = `${id}:${name}`;
    const vaultItem = vaultDrafts[key];
    if (!vaultItem) return;
    await histdataApi.setSecret(id, name, '', vaultItem);
    vaultDrafts[key] = null;
    onchanged?.();
  }
  // Vault reference already stored for a secret, if any (drives the "vault" badge).
  const vaultRef = (c, name) => (c.secrets ?? []).find((s) => s.name === name)?.vault_item_id;
  async function clearSecret(id, name) {
    await histdataApi.deleteSecret(id, name);
    onchanged?.();
  }

  function quotaLine(c) {
    if (!c.quota) return '';
    const period = $t(`common.period.${c.quota.period}`);
    const max = c.quota.max_requests == null ? '∞' : c.quota.max_requests;
    return `${c.quota.used} / ${max} · ${period}`;
  }
  function resetsAt(c) {
    if (!c.quota?.resets_at) return '';
    try {
      return new Date(c.quota.resets_at).toLocaleString(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short'
      });
    } catch {
      return '';
    }
  }
</script>

<div class="split">
  <div class="left">
    <button class="addbtn" class:active={adding} onclick={() => (adding = !adding)}>
      <Icon name="plus" size={14} /> {$t('histdata.connectors.add')}
    </button>
    <ul class="list">
      {#each connectors as c (c.id)}
        <li>
          <button
            class="row"
            class:active={selected?.id === c.id}
            onclick={() => (selectedId = c.id)}
          >
            {#if c.quota && c.quota.max_requests != null}
              <UsagePie used={c.quota.used} max={c.quota.max_requests} title={quotaLine(c)} />
            {:else}
              <span class="dot" class:ok={ready(c)}></span>
            {/if}
            <span class="names">
              <span class="nm">{c.name}</span>
              <span class="prov">{c.label}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </div>

  <div class="detail">
    {#if adding}
      <div class="addform">
        <h3>{$t('histdata.connectors.add')}</h3>
        <label class="fld">
          {$t('histdata.connectors.provider')}
          <select bind:value={addProvider} onchange={onAddProviderChange}>
            <option value="" disabled>{$t('histdata.download.selectPlaceholder')}</option>
            {#each providers as p (p.provider)}
              <option value={p.provider}>{p.label}</option>
            {/each}
          </select>
        </label>
        <label class="fld">
          {$t('histdata.connectors.name')}
          <input bind:value={addName} placeholder={$t('histdata.connectors.namePlaceholder')} />
        </label>
        {#if addCap}
          {#if addCap.required_secrets?.length}
            <!-- Everything the provider needs, right in the form: pick each key from the
                 vault (or create one inline); saved onto the connector at creation. -->
            <div class="secrets">
              {#each addCap.required_secrets as secretName (secretName)}
                <div class="secret">
                  <span class="sname">{secretName}</span>
                  <div class="spicker">
                    <VaultPicker bind:vaultItemId={addVault[secretName]} hasSecret={false} />
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <p class="note">{$t('histdata.providers.noCredentials')}</p>
          {/if}
          {#if addCap.rate_limit}<p class="rate">{addCap.rate_limit}</p>{/if}
        {/if}
        <div class="limit">
          <label class="chk">
            <input type="checkbox" bind:checked={addTrack} />
            {$t('histdata.connectors.trackUsage')}
          </label>
          {#if addTrack}
            <label class="chk">
              <input type="checkbox" bind:checked={addUnlimited} />
              {$t('histdata.connectors.unlimited')}
            </label>
            {#if !addUnlimited}
              <input class="num" type="number" min="1" bind:value={addMax} placeholder="500" />
            {/if}
            <span class="per">{$t('histdata.connectors.per')}</span>
            <select bind:value={addPeriod}>
              {#each ['minute', 'hour', 'day', 'week', 'month'] as p (p)}
                <option value={p}>{$t(`common.period.${p}`)}</option>
              {/each}
            </select>
          {/if}
        </div>
        <div class="addactions">
          <button class="ghost" onclick={() => (adding = false)}>{$t('common.cancel')}</button>
          <button class="primary" onclick={createConnector} disabled={!addProvider || !addName.trim()}>
            {$t('histdata.connectors.create')}
          </button>
        </div>
      </div>
    {:else if !selected}
      <p class="note">{$t('histdata.connectors.none')}</p>
    {:else}
      {@const c = selected}
      <div class="head">
        <a class="name" href={c.website} target="_blank" rel="noreferrer">{c.name}</a>
        <span class="provlbl">{c.label}</span>
        {#if !c.required_secrets.length}<span class="tag free">{$t('histdata.providers.keyless')}</span>{/if}
        {#if c.docs_url}
          <a class="docs" href={c.docs_url} target="_blank" rel="noreferrer" title={$t('histdata.providers.openDocs')}>{$t('histdata.providers.apiDocs')} <Icon name="external-link" size={11} /></a>
        {/if}
      </div>
      {#if c.rate_limit}<p class="rate">{c.rate_limit}</p>{/if}

      <div class="fldrow">
        <span class="sname">{$t('histdata.connectors.name')}</span>
        <input bind:value={nameDraft} />
        <button onclick={rename} disabled={!nameDraft.trim() || nameDraft.trim() === c.name}>
          {$t('histdata.connectors.rename')}
        </button>
      </div>

      {#if c.required_secrets.length}
        <!-- Keys live in the central vault; "From vault" below plugs one in per secret. -->
        <p class="vaulthint">
          <Icon name="lock" size={12} />
          <span>
            {$t('histdata.providers.vaultHint')}
            <a href="/settings#vault">{$t('histdata.providers.vaultHintLink')}</a>
          </span>
        </p>
        <div class="secrets">
          {#each c.required_secrets as name (name)}
            {@const isSet = c.set_secrets.includes(name)}
            <div class="secret">
              <span class="sname">{name}</span>
              <span class="state" class:set={isSet}>
                {#if isSet && vaultRef(c, name)}<Icon name="lock" size={11} /> {$t('vault.picker.plugged')}{:else}{isSet ? $t('histdata.providers.set') : $t('histdata.providers.notSet')}{/if}
              </span>
              <div class="spicker">
                <VaultPicker
                  bind:vaultItemId={vaultDrafts[`${c.id}:${name}`]}
                  hasSecret={isSet}
                />
              </div>
              <button onclick={() => saveSecret(c.id, name)}>{$t('common.save')}</button>
              {#if isSet}
                <button class="danger" onclick={() => clearSecret(c.id, name)}>{$t('common.clear')}</button>
              {/if}
            </div>
          {/each}
        </div>
      {:else}
        <p class="note">{$t('histdata.providers.noCredentials')}</p>
      {/if}

      <div class="limitbox">
        <div class="limithead">
          <span>{$t('histdata.connectors.limitTitle')}</span>
          {#if c.quota}
            <span class="usage">
              {#if c.quota.max_requests != null}
                <UsagePie used={c.quota.used} max={c.quota.max_requests} title={quotaLine(c)} />
              {/if}
              {quotaLine(c)}
              <span class="resets">· {$t('histdata.connectors.resets', { date: resetsAt(c) })}</span>
            </span>
          {/if}
        </div>
        <div class="limit">
          <label class="chk">
            <input type="checkbox" bind:checked={limTrack} />
            {$t('histdata.connectors.trackUsage')}
          </label>
          {#if limTrack}
            <label class="chk">
              <input type="checkbox" bind:checked={limUnlimited} />
              {$t('histdata.connectors.unlimited')}
            </label>
            {#if !limUnlimited}
              <input class="num" type="number" min="1" bind:value={limMax} placeholder="500" />
            {/if}
            <span class="per">{$t('histdata.connectors.per')}</span>
            <select bind:value={limPeriod}>
              {#each ['minute', 'hour', 'day', 'week', 'month'] as p (p)}
                <option value={p}>{$t(`common.period.${p}`)}</option>
              {/each}
            </select>
          {/if}
          <button onclick={saveLimit}>{$t('common.save')}</button>
        </div>
      </div>

      <div class="dangerzone">
        <button class="danger" onclick={() => (confirmOpen = true)}>
          <Icon name="trash" size={13} /> {$t('histdata.connectors.delete')}
        </button>
      </div>
    {/if}
    <ErrorText error={error} copyable />
  </div>
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('histdata.connectors.delete')}
  message={$t('histdata.connectors.deleteMessage', { name: selected?.name ?? '' })}
  confirmLabel={$t('histdata.connectors.delete')}
  danger
  onconfirm={removeConnector}
/>

<style>
  .split {
    display: grid;
    grid-template-columns: 250px 1fr;
    gap: var(--space-4);
    align-items: start;
  }
  .left {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-right: 1px solid var(--border);
    padding-right: var(--space-3);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.active {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--red);
    flex: none;
  }
  .dot.ok {
    background: var(--green);
  }
  .names {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .nm {
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prov {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .addbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    width: 100%;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    padding: var(--space-2) var(--space-3);
  }
  .addbtn:hover {
    background: var(--surface-2);
  }
  .addbtn.active {
    background: var(--surface-2);
  }
  .detail {
    min-height: 120px;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .name {
    font-weight: var(--fw-medium);
    font-size: var(--text-md);
    color: var(--text);
    text-decoration: none;
  }
  .provlbl {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .tag {
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: var(--radius);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  /* Text-only colored tags — no filled pill. */
  .free {
    color: var(--green);
  }
  .docs {
    font-size: var(--text-xs);
    color: var(--muted);
    text-decoration: none;
    margin-left: auto;
  }
  .rate {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.4;
    max-width: 60ch;
  }
  .note {
    color: var(--muted);
    font-size: var(--text-base);
  }
  /* Quiet inline hint above the credential rows — informative, not an alert. */
  .vaulthint {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) 0 var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .vaulthint :global(svg) {
    flex: none;
    opacity: 0.7;
  }
  .vaulthint a {
    color: var(--accent);
    text-decoration: none;
  }
  .vaulthint a:hover {
    text-decoration: underline;
  }
  .secrets {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .secret,
  .fldrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  /* The vault picker stacks a mode row above the input; top-align its row. */
  .secret {
    align-items: flex-start;
  }
  .secret .state,
  .secret .sname,
  .secret button {
    margin-top: 4px;
  }
  .spicker {
    flex: 1;
    min-width: 0;
  }
  .sname {
    width: 110px;
    font-size: var(--text-base);
    color: var(--muted);
  }
  .state {
    width: 60px;
    font-size: var(--text-xs);
    color: var(--red);
  }
  .state.set {
    color: var(--green);
  }
  .fldrow input {
    flex: 1;
    max-width: 320px;
  }
  button {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .danger {
    color: var(--red);
    border-color: var(--red);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .limitbox {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: 560px;
  }
  .limithead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .usage {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-weight: var(--fw-normal);
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .resets {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .limit {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
  }
  .chk input {
    margin: 0;
  }
  .num {
    width: 90px;
  }
  .per {
    color: var(--muted);
  }
  .dangerzone {
    margin-top: var(--space-2);
  }
  .addform {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 480px;
  }
  .addform h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .addactions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .primary {
    background: transparent;
    color: var(--text);
    border: var(--hairline) solid var(--border-control);
    font-weight: var(--fw-medium);
    padding: var(--space-2) var(--space-4);
  }
  .primary:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .ghost {
    background: transparent;
  }
</style>
