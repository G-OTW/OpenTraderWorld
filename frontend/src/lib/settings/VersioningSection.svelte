<script>
  // Versioning: the global switches for editor files and saved strategies. On, the user can
  // then turn versioning on per file or per strategy and save versions by hand. Off, the
  // feature is hidden; what was saved is kept or deleted, as the user answers.
  import { onMount } from 'svelte';
  import { versioningApi } from '$lib/versioning/api.js';
  import { versioning } from '$lib/versioning/state.svelte.js';
  import { fmtBytes } from '$lib/settings/api.js';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ChoiceModal from '$lib/ui/ChoiceModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  const SCOPES = ['editor', 'strategies'];

  let cfg = $state(null); // { editor, strategies, usage: { editor: {versions, bytes}, ... } }
  let error = $state('');
  let busy = $state(false);
  let pending = $state(null); // scope awaiting an answer
  let onOpen = $state(false);
  let offOpen = $state(false);

  onMount(load);

  async function load() {
    try {
      cfg = await versioningApi.settings();
    } catch (e) {
      error = e.message;
    }
  }

  function flip(scope, e) {
    // The switch moves only once the user answered the modal.
    e.currentTarget.checked = cfg[scope];
    pending = scope;
    if (cfg[scope]) offOpen = true;
    else onOpen = true;
  }

  async function apply(enabled, purge = false) {
    const scope = pending;
    pending = null;
    if (!scope) return;
    busy = true;
    error = '';
    try {
      cfg = await versioningApi.setEnabled(scope, enabled, purge);
      versioning.apply(cfg);
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="section">
  <h2>{$t('settings.versioning.title')}</h2>
  <p class="muted small">{$t('settings.versioning.subtitle')}</p>

  {#if !cfg}
    <div class="rows" aria-busy="true">
      {#each SCOPES as s (s)}<Skeleton height="4.5rem" />{/each}
    </div>
  {:else}
    <div class="rows">
      {#each SCOPES as scope (scope)}
        <div class="row">
          <div class="txt">
            <span class="name">{$t(`settings.versioning.${scope}.name`)}</span>
            <span class="muted small">{$t(`settings.versioning.${scope}.desc`)}</span>
            <span class="muted small usage">
              {$t('settings.versioning.usage', {
                n: cfg.usage[scope].versions,
                size: fmtBytes(cfg.usage[scope].bytes)
              })}
            </span>
          </div>
          <label class="switch" class:on={cfg[scope]}>
            <input type="checkbox" checked={cfg[scope]} disabled={busy} onchange={(e) => flip(scope, e)} />
            <span>{cfg[scope] ? $t('settings.versioning.on') : $t('settings.versioning.off')}</span>
          </label>
        </div>
      {/each}
    </div>
  {/if}
  <ErrorText {error} />
</div>

<ConfirmModal
  bind:open={onOpen}
  title={$t('settings.versioning.onTitle')}
  message={$t(`settings.versioning.onMessage.${pending ?? 'editor'}`)}
  confirmLabel={$t('settings.versioning.turnOn')}
  cancelLabel={$t('common.cancel')}
  onconfirm={() => apply(true)}
  oncancel={() => (pending = null)}
/>

<ChoiceModal
  bind:open={offOpen}
  title={$t('settings.versioning.offTitle')}
  message={$t(`settings.versioning.offMessage.${pending ?? 'editor'}`, {
    n: pending ? cfg?.usage[pending].versions ?? 0 : 0
  })}
  cancelLabel={$t('common.cancel')}
  choices={[
    { value: 'keep', label: $t('settings.versioning.offKeep'), variant: 'btn' },
    { value: 'purge', label: $t('settings.versioning.offPurge'), variant: 'danger' }
  ]}
  onpick={(v) => apply(false, v === 'purge')}
  oncancel={() => (pending = null)}
/>

<style>
  .section {
    max-width: 620px;
  }
  h2 {
    margin: 0 0 var(--space-1);
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface);
  }
  .txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .name {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .usage {
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .switch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 96px;
    min-height: 36px;
    justify-content: center;
    flex: none;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--muted);
    cursor: pointer;
  }
  .switch.on {
    border-color: color-mix(in srgb, var(--green) 45%, var(--border-control));
    background: color-mix(in srgb, var(--green) 7%, var(--surface));
    color: var(--green-ink);
  }
  .switch:has(input:disabled) {
    cursor: default;
    opacity: 0.65;
  }
</style>
