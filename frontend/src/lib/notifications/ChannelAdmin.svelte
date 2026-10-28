<script>
  // Notification broker admin — the one place channels are created, credentialed and
  // granted.
  //
  // A channel is a destination the user owns (their SMTP account, their Telegram bot, a
  // Slack/Discord webhook). Two dials decide whether anything reaches it: `enabled` (the
  // user's on/off switch) and the module grants (who may push to it). Secrets are
  // write-only: we only ever know whether one is set.
  //
  // Self-loading on purpose: the same component is the Settings section and the modal
  // every notifying module opens, so hosts never have to fetch the list to show it.
  // `onchanged` fires after every mutation so a host can refresh its *own* picker.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import ConfigField from './ConfigField.svelte';
  import { modules as moduleRegistry } from '$lib/modules/registry.js';
  import { channelsApi, CHANNEL_KINDS, channelMeta, ALL_MODULES, isReady } from './api.js';
  import { t } from '$lib/i18n';

  // `module` = the module the admin was opened from: new channels are granted to it by
  // default, and its row is marked in the grant editor.
  let { module = null, onchanged } = $props();

  let channels = $state([]);
  let moduleIds = $state([]);
  let loading = $state(true);
  let error = $state('');
  // Per-channel transient send result: { id, ok: true|false|null, text }.
  let testMsg = $state(null);

  // `security` is a producer without a module: the app itself, reporting the handful of
  // events an exposed instance should not have to be watching the logs to notice (a
  // sign-in from an unrecognised source, a minted or first-used agent token, a change of
  // network exposure). Everything else in the list is a real module.
  const moduleName = (id) =>
    id === 'security'
      ? $t('notifChannels.producer.security')
      : (moduleRegistry.find((m) => m.id === id)?.name ?? id);
  const kindLabel = (id) => channelMeta(id)?.label ?? id;

  async function load() {
    try {
      const [c, m] = await Promise.all([channelsApi.list(), channelsApi.modules()]);
      channels = c;
      moduleIds = m;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    load();
  });

  async function reload() {
    channels = await channelsApi.list();
    onchanged?.();
  }

  // Remember which channel was open across refresh (per-browser).
  const SEL_KEY = 'otw.notifchannels.selected.v1';
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
  const selected = $derived(channels.find((c) => c.id === selectedId) ?? channels[0] ?? null);

  /** Short label for a channel's reach, shown under its name in the list. */
  function grantLabel(c) {
    const m = c.modules ?? [];
    if (m.includes(ALL_MODULES)) return $t('notifch.grants.all');
    if (!m.length) return $t('notifch.grants.none');
    return m.map(moduleName).join(' · ');
  }

  // ── Add channel ──
  let adding = $state(false);
  let addKind = $state('');
  let addName = $state('');
  let addConfig = $state({});
  let addModules = $state([ALL_MODULES]);
  let addVault = $state(null);
  const addMeta = $derived(addKind ? channelMeta(addKind) : null);

  function openAdd(kind) {
    adding = true;
    addKind = kind;
    const meta = channelMeta(kind);
    addName = meta.label;
    addConfig = Object.fromEntries(
      meta.fields.flatMap((f) =>
        f.vaultable ? [[f.key, ''], [`${f.key}_vault_item`, null]] : [[f.key, '']]
      )
    );
    // Opened from a module: grant that module only. Opened from settings: grant all.
    addModules = module ? [module] : [ALL_MODULES];
    addVault = null;
    error = '';
  }

  async function createChannel() {
    error = '';
    if (!addKind) return;
    if (!addVault) {
      error = $t('notifch.secretRequired');
      return;
    }
    try {
      const c = await channelsApi.create({
        kind: addKind,
        name: addName.trim() || addMeta.label,
        config: addConfig,
        // A channel is created to be used: on by default, one click from off.
        enabled: true,
        modules: addModules,
        secret_vault_item: addVault
      });
      adding = false;
      selectedId = c.id;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Detail form (re-seeded whenever the selection changes) ──
  let nameDraft = $state('');
  let configDraft = $state({});
  let secretDraft = $state(null);
  $effect(() => {
    const c = selected;
    nameDraft = c?.name ?? '';
    // A stored config predates the vaultable fields, so seed the keys the form binds to.
    const seeded = Object.fromEntries(
      (channelMeta(c?.kind)?.fields ?? [])
        .filter((f) => f.vaultable)
        .map((f) => [`${f.key}_vault_item`, null])
    );
    configDraft = { ...seeded, ...(c?.config ?? {}) };
    secretDraft = null;
  });
  const selMeta = $derived(selected ? channelMeta(selected.kind) : null);
  const dirty = $derived(
    !!selected &&
      (nameDraft.trim() !== selected.name ||
        (selMeta?.fields ?? []).some(
          (f) =>
            (configDraft[f.key] ?? '') !== (selected.config?.[f.key] ?? '') ||
            (configDraft[`${f.key}_vault_item`] ?? null) !==
              (selected.config?.[`${f.key}_vault_item`] ?? null)
        ))
  );

  /** PATCH bodies must carry `kind` (the server validates it) — build from the row. */
  const body = (c, over = {}) => ({
    kind: c.kind,
    name: c.name,
    config: c.config,
    enabled: c.enabled,
    ...over
  });

  async function saveDetail() {
    if (!selected) return;
    error = '';
    try {
      await channelsApi.update(
        selected.id,
        body(selected, { name: nameDraft.trim() || selMeta.label, config: configDraft })
      );
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function saveSecret() {
    if (!selected || !secretDraft) return;
    error = '';
    try {
      await channelsApi.update(selected.id, body(selected, { secret_vault_item: secretDraft }));
      secretDraft = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function toggleEnabled(c) {
    error = '';
    // Enabling a channel with no credential would only produce failed sends.
    if (!c.enabled && !c.has_secret) {
      error = $t('notifch.needSecretFirst');
      return;
    }
    try {
      await channelsApi.update(c.id, body(c, { enabled: !c.enabled }));
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Module grants ──
  // "All modules" is exclusive: picking it drops the explicit list, picking a module
  // drops the wildcard. Saved immediately — a grant toggle is not a form.
  async function setGrants(c, next) {
    error = '';
    try {
      await channelsApi.update(c.id, body(c, { modules: next }));
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
  function toggleGrant(c, id) {
    const m = c.modules ?? [];
    if (id === ALL_MODULES) return setGrants(c, [ALL_MODULES]);
    const explicit = m.includes(ALL_MODULES) ? [] : m;
    const next = explicit.includes(id) ? explicit.filter((x) => x !== id) : [...explicit, id];
    return setGrants(c, next);
  }
  function toggleAddGrant(id) {
    if (id === ALL_MODULES) {
      addModules = [ALL_MODULES];
      return;
    }
    const explicit = addModules.includes(ALL_MODULES) ? [] : addModules;
    addModules = explicit.includes(id) ? explicit.filter((x) => x !== id) : [...explicit, id];
  }
  const granted = (list, id) => (list ?? []).includes(id);

  // ── Test ──
  async function test(c) {
    testMsg = { id: c.id, ok: null, text: $t('notifch.testing') };
    try {
      await channelsApi.test(c.id);
      testMsg = { id: c.id, ok: true, text: $t('notifch.testOk') };
    } catch (e) {
      testMsg = { id: c.id, ok: false, text: e.message };
    }
    await reload();
  }

  // ── Delete ──
  let confirmOpen = $state(false);
  async function removeChannel() {
    if (!selected) return;
    try {
      await channelsApi.remove(selected.id);
      selectedId = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
</script>

{#if loading}
  <Skeleton height="220px" />
{:else}
  <div class="split">
    <div class="left">
      <span class="addlbl">{$t('notifch.add')}</span>
      <div class="kinds">
        {#each CHANNEL_KINDS as k (k.id)}
          <button
            class="kind"
            class:active={adding && addKind === k.id}
            onclick={() => (adding && addKind === k.id ? (adding = false) : openAdd(k.id))}
          >
            <Icon name="plus" size={12} />
            {k.label}
          </button>
        {/each}
      </div>
      <ul class="list">
        {#each channels as c (c.id)}
          <li>
            <button class="row" class:active={selected?.id === c.id} onclick={() => (selectedId = c.id)}>
              <span class="dot" class:ok={c.enabled && isReady(c)} class:warn={c.enabled && !isReady(c)}></span>
              <span class="names">
                <span class="nm">{c.name}</span>
                <span class="sub">{kindLabel(c.kind)} · {grantLabel(c)}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
      {#if !channels.length}
        <p class="note">{$t('notifch.none')}</p>
      {/if}
    </div>

    <div class="detail">
      {#if adding && addMeta}
        <div class="addform">
          <h3>{$t('notifch.addKind', { kind: addMeta.label })}</h3>
          <label class="fld">
            {$t('notifch.name')}
            <input bind:value={addName} placeholder={addMeta.label} />
          </label>
          {#each addMeta.fields as f (f.key)}
            <label class="fld">
              <span>{f.label}{#if f.required}<em class="req">*</em>{/if}</span>
              {#if f.vaultable}
                <ConfigField
                  bind:value={addConfig[f.key]}
                  bind:vaultItem={addConfig[`${f.key}_vault_item`]}
                  type={f.type === 'number' ? 'number' : 'text'}
                  placeholder={f.placeholder ?? ''}
                />
              {:else}
                <input
                  type={f.type === 'number' ? 'number' : 'text'}
                  bind:value={addConfig[f.key]}
                  placeholder={f.placeholder ?? ''}
                />
              {/if}
            </label>
          {/each}
          <div class="fld">
            <span>{addMeta.secret.label}<em class="req">*</em></span>
            <VaultPicker bind:vaultItemId={addVault} hasSecret={false} />
            {#if addMeta.secret.help}<small class="help">{addMeta.secret.help}</small>{/if}
          </div>
          <div class="fld">
            {$t('notifch.grants.label')}
            <div class="chips">
              <button class="chip" class:on={granted(addModules, ALL_MODULES)} onclick={() => toggleAddGrant(ALL_MODULES)}>
                {$t('notifch.grants.all')}
              </button>
              {#each moduleIds as id (id)}
                <button class="chip" class:on={granted(addModules, id)} onclick={() => toggleAddGrant(id)}>
                  {moduleName(id)}
                </button>
              {/each}
            </div>
          </div>
          <div class="addactions">
            <Button variant="ghost" onclick={() => (adding = false)}>{$t('common.cancel')}</Button>
            <Button variant="primary" onclick={createChannel} disabled={!addVault}>
              {$t('notifch.create')}
            </Button>
          </div>
        </div>
      {:else if !selected}
        <p class="note">{$t('notifch.none')}</p>
      {:else}
        {@const c = selected}
        <div class="head">
          <span class="name">{c.name}</span>
          <span class="kindlbl">{kindLabel(c.kind)}</span>
          {#if !c.has_secret}<span class="tag warn">{$t('notifch.noSecret')}</span>{/if}
          {#if c.last_ok === false && c.last_error}
            <span class="tag bad" title={c.last_error}>{$t('notifch.lastFailed')}</span>
          {/if}
          <label class="switch" title={$t('notifch.enableSend')}>
            <input type="checkbox" checked={c.enabled} onchange={() => toggleEnabled(c)} />
            <span class="slider"></span>
          </label>
          <span class="onoff">{c.enabled ? $t('notifch.on') : $t('notifch.off')}</span>
          <button class="testbtn" onclick={() => test(c)}>
            <Icon name="send" size={12} /> {$t('notifch.test')}
          </button>
        </div>
        {#if testMsg && testMsg.id === c.id}
          <p class="test" class:ok={testMsg.ok === true} class:bad={testMsg.ok === false}>{testMsg.text}</p>
        {/if}

        <div class="fldrow grants">
          <span class="sname">{$t('notifch.grants.label')}</span>
          <div class="chips">
            <button class="chip" class:on={granted(c.modules, ALL_MODULES)} onclick={() => toggleGrant(c, ALL_MODULES)}>
              {$t('notifch.grants.all')}
            </button>
            {#each moduleIds as id (id)}
              <button
                class="chip"
                class:on={granted(c.modules, id) && !granted(c.modules, ALL_MODULES)}
                class:implied={granted(c.modules, ALL_MODULES)}
                onclick={() => toggleGrant(c, id)}
              >
                {moduleName(id)}
              </button>
            {/each}
          </div>
        </div>
        {#if !(c.modules ?? []).length}
          <p class="warnline">{$t('notifch.grants.noneHint')}</p>
        {/if}

        <div class="form">
          <label class="fldrow">
            <span class="sname">{$t('notifch.name')}</span>
            <input bind:value={nameDraft} />
          </label>
          {#each selMeta?.fields ?? [] as f (f.key)}
            <label class="fldrow">
              <span class="sname">{f.label}</span>
              {#if f.vaultable}
                <ConfigField
                  bind:value={configDraft[f.key]}
                  bind:vaultItem={configDraft[`${f.key}_vault_item`]}
                  type={f.type === 'number' ? 'number' : 'text'}
                  placeholder={f.placeholder ?? ''}
                />
              {:else}
                <input
                  type={f.type === 'number' ? 'number' : 'text'}
                  bind:value={configDraft[f.key]}
                  placeholder={f.placeholder ?? ''}
                />
              {/if}
            </label>
          {/each}
          <div class="formactions">
            <Button onclick={saveDetail} disabled={!dirty}>{$t('common.save')}</Button>
          </div>
        </div>

        <!-- Credentials live in the central vault; the picker below plugs one in. -->
        <p class="vaulthint">
          <Icon name="lock" size={12} />
          <span>
            {$t('notifch.vaultHint')}
            <a href="/settings#vault">{$t('notifch.vaultHintLink')}</a>
          </span>
        </p>
        <div class="secret">
          <span class="sname">{selMeta?.secret.label}</span>
          <span class="state" class:set={c.has_secret}>
            {c.has_secret ? $t('notifch.set') : $t('notifch.notSet')}
          </span>
          <div class="spicker">
            <VaultPicker bind:vaultItemId={secretDraft} hasSecret={c.has_secret} />
          </div>
          <Button onclick={saveSecret} disabled={!secretDraft}>{$t('common.save')}</Button>
        </div>

        <div class="dangerzone">
          <Button variant="danger" icon="trash" onclick={() => (confirmOpen = true)}>
            {$t('notifch.delete')}
          </Button>
        </div>
      {/if}
      <ErrorText error={error} copyable />
    </div>
  </div>
{/if}

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('notifch.delete')}
  message={$t('notifch.deleteMessage', { name: selected?.name ?? '' })}
  confirmLabel={$t('notifch.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={removeChannel}
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
  .addlbl {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .kinds {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-1);
  }
  .kind {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    padding: var(--space-2) var(--space-1);
  }
  .kind:hover,
  .kind.active {
    background: var(--surface-2);
  }
  .kind.active {
    border-color: var(--accent);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    list-style: none;
    margin-top: var(--space-2);
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
  /* Grey = off, green = live, amber = on but missing its credential. */
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .dot.ok {
    background: var(--green);
  }
  .dot.warn {
    background: var(--amber);
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
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    flex-wrap: wrap;
  }
  .name {
    font-weight: var(--fw-medium);
    font-size: var(--text-md);
  }
  .kindlbl {
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
  .tag.warn {
    color: var(--amber);
  }
  .tag.bad {
    color: var(--red);
  }
  /* Reserve the widest translation's width so flipping the switch doesn't shuffle the row. */
  .onoff {
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 4.5em;
  }
  .testbtn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .testbtn:hover {
    background: var(--surface-2);
  }
  .test {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .test.ok {
    color: var(--green);
  }
  .test.bad {
    color: var(--red);
  }
  .note {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .warnline {
    color: var(--amber);
    font-size: var(--text-sm);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .formactions {
    display: flex;
    justify-content: flex-start;
  }
  .fldrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .fldrow input {
    flex: 1;
    max-width: 320px;
  }
  .sname {
    width: 130px;
    flex: none;
    font-size: var(--text-base);
    color: var(--muted);
  }
  .grants {
    align-items: flex-start;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  /* Grant toggles: quiet by default, accent-outlined when the module is allowed.
     `implied` renders the modules a wildcard grant covers without claiming they were
     picked one by one. */
  .chip {
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .chip:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .chip.implied {
    color: var(--muted);
    border-style: dashed;
  }
  /* Quiet inline hint above the credential row — informative, not an alert. */
  .vaulthint {
    display: flex;
    align-items: center;
    gap: var(--space-2);
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
  /* The vault picker stacks a mode row above the input; top-align its row. */
  .secret {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .secret .state,
  .secret .sname,
  .secret :global(.btn) {
    margin-top: 4px;
  }
  .spicker {
    flex: 1;
    min-width: 0;
  }
  .state {
    width: 60px;
    flex: none;
    font-size: var(--text-xs);
    color: var(--red);
  }
  .state.set {
    color: var(--green);
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
  .req {
    color: var(--red);
    font-style: normal;
    margin-left: 2px;
  }
  .help {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .addactions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  /* toggle switch */
  .switch {
    position: relative;
    display: inline-block;
    width: 34px;
    height: 18px;
    flex: none;
    cursor: pointer;
  }
  .switch input {
    opacity: 0;
    width: 0;
    height: 0;
  }
  .slider {
    position: absolute;
    inset: 0;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 999px;
    transition: 0.15s;
  }
  .slider::before {
    content: '';
    position: absolute;
    height: 12px;
    width: 12px;
    left: 2px;
    top: 2px;
    border-radius: 50%;
    background: var(--muted);
    transition: 0.15s;
  }
  .switch input:checked + .slider {
    background: var(--accent);
    border-color: var(--accent);
  }
  .switch input:checked + .slider::before {
    transform: translateX(16px);
    background: var(--surface);
  }

  @media (max-width: 720px) {
    .split {
      grid-template-columns: 1fr;
    }
    .left {
      border-right: none;
      padding-right: 0;
      border-bottom: 1px solid var(--border);
      padding-bottom: var(--space-3);
    }
  }
</style>
