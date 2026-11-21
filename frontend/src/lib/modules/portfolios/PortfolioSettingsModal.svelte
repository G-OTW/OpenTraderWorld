<script>
  // Alert settings of one portfolio: the concentration limit, the drift alert, the digest,
  // and the channels each goes to. Channels are the shared broker's, filtered to the ones
  // granted to Portfolio Tracker; none picked means every granted channel.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Select from '$lib/ui/Select.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { portfoliosApi } from './api.js';

  let { portfolio, onclose, onsaved } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state({
    max_weight_pct: portfolio.max_weight_pct ?? '',
    concentration_notify: portfolio.concentration_notify ?? false,
    drift_notify: portfolio.drift_notify ?? false,
    alert_channels: [...(portfolio.alert_channels ?? [])],
    digest: portfolio.digest ?? 'off',
    digest_channels: [...(portfolio.digest_channels ?? [])]
  });
  let channels = $state([]);
  let error = $state(null);
  let saving = $state(false);

  async function loadChannels() {
    try {
      channels = await channelsApi.list('portfolios');
    } catch {
      channels = [];
    }
  }
  $effect(() => {
    loadChannels();
  });
  const options = $derived(channels.map((c) => ({ value: c.id, label: `${c.name} (${c.kind})` })));

  async function save() {
    saving = true;
    error = null;
    try {
      const p = await portfoliosApi.update(portfolio.id, {
        max_weight_pct: form.max_weight_pct === '' ? null : Number(form.max_weight_pct),
        concentration_notify: form.concentration_notify,
        drift_notify: form.drift_notify,
        alert_channels: form.alert_channels,
        digest: form.digest,
        digest_channels: form.digest_channels
      });
      onsaved?.(p);
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal open size="md" title={$t('portfolios.settings.title')} {onclose}>
  <div class="form">
  <p class="sub">{$t('portfolios.concentration.title')}</p>
  <Field label={$t('portfolios.concentration.max')} hint={$t('portfolios.concentration.hint')}>
    <input type="number" step="any" min="0" max="100" bind:value={form.max_weight_pct} />
  </Field>
  <label class="check"><input type="checkbox" bind:checked={form.concentration_notify} /> {$t('portfolios.concentration.notify')}</label>

  <p class="sub">{$t('portfolios.drift.title')}</p>
  <label class="check"><input type="checkbox" bind:checked={form.drift_notify} /> {$t('portfolios.drift.notify')}</label>
  <div class="channels">
    <Field label={$t('portfolios.settings.alertChannels')}>
      <MultiSelect bind:value={form.alert_channels} {options} allLabel={$t('portfolios.settings.allChannels')} width="100%" />
    </Field>
    <ChannelButton module="portfolios" size="sm" onchanged={loadChannels} />
  </div>

  <p class="sub">{$t('portfolios.digest.title')}</p>
  <Field label={$t('portfolios.digest.cadence')} hint={$t('portfolios.digest.hint')}>
    <Select
      bind:value={form.digest}
      options={['off', 'daily', 'weekly'].map((d) => ({ value: d, label: $t(`portfolios.digest.${d}`) }))}
    />
  </Field>
  <Field label={$t('portfolios.settings.digestChannels')}>
    <MultiSelect bind:value={form.digest_channels} {options} allLabel={$t('portfolios.settings.allChannels')} width="100%" />
  </Field>

  <ErrorText {error} />

  </div>

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={save} disabled={saving}>
      {saving ? $t('common.saving') : $t('common.save')}
    </button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* A section opens on a hairline, not on a second frame. */
  .sub {
    margin: var(--space-2) 0 0;
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
    font-size: 13px;
    font-weight: 600;
  }
  .sub:first-child {
    margin-top: 0;
    padding-top: 0;
    border-top: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
  }
  .channels {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
  }
  .channels > :global(:first-child) {
    flex: 1;
  }
</style>
