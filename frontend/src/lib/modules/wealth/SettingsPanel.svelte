<script>
  // MyWealth settings: the weekly notice of aged valuations (through the channel broker) and
  // the CPI series the Real curve is deflated with.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { wealthApi } from './api.js';
  import { t } from '$lib/i18n';

  let { settings = $bindable() } = $props();

  let channels = $state([]);
  let series = $state([]);
  let error = $state('');
  let saved = $state(false);
  let form = $state({ stale_notify: 'off', stale_channels: [], cpi_series_id: '' });

  $effect(() => {
    form = {
      stale_notify: settings?.stale_notify ?? 'off',
      stale_channels: [...(settings?.stale_channels ?? [])],
      cpi_series_id: settings?.cpi_series_id ?? ''
    };
  });

  $effect(() => {
    loadChannels();
    wealthApi.real().then((r) => (series = r.series)).catch(() => (series = []));
  });

  async function loadChannels() {
    try {
      channels = await wealthApi.channels();
    } catch (e) {
      error = e.message;
    }
  }

  function toggleChannel(id) {
    const s = new Set(form.stale_channels);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    form.stale_channels = [...s];
  }

  async function save() {
    error = '';
    saved = false;
    try {
      settings = await wealthApi.savePreferences({
        goal_value: settings.goal_value ?? null,
        goal_date: settings.goal_date ?? null,
        monthly_expenses: settings.monthly_expenses ?? null,
        stale_notify: form.stale_notify,
        stale_channels: form.stale_channels,
        cpi_series_id: form.cpi_series_id || null
      });
      saved = true;
    } catch (e) {
      error = e.message;
    }
  }
</script>

<div class="settings">
  <section class="box">
    <h3>{$t('wealth.settings.staleTitle')}</h3>
    <p class="muted">{$t('wealth.settings.staleIntro')}</p>
    <div class="row">
      <div class="field">
        <span>{$t('wealth.settings.staleNotify')}</span>
        <Dropdown
          bind:value={form.stale_notify}
          ariaLabel={$t('wealth.settings.staleNotify')}
          options={[
            { value: 'off', label: $t('wealth.settings.off') },
            { value: 'weekly', label: $t('wealth.settings.weekly') }
          ]}
        />
      </div>
      <ChannelButton module="wealth" onchanged={loadChannels} />
    </div>
    {#if form.stale_notify === 'weekly'}
      {#if channels.length === 0}
        <p class="muted">{$t('wealth.settings.noChannels')}</p>
      {:else}
        <div class="chips" role="group" aria-label={$t('wealth.settings.channels')}>
          <span class="muted">{$t('wealth.settings.channels')}</span>
          {#each channels as c (c.id)}
            <button class="chip" class:active={form.stale_channels.includes(c.id)} aria-pressed={form.stale_channels.includes(c.id)} onclick={() => toggleChannel(c.id)}>{c.name}</button>
          {/each}
        </div>
        <p class="muted">{form.stale_channels.length ? $t('wealth.settings.onlyPicked') : $t('wealth.settings.allGranted')}</p>
      {/if}
      {#if settings?.stale_last_at}<p class="muted">{$t('wealth.settings.lastRun', { at: new Date(settings.stale_last_at).toLocaleString() })}</p>{/if}
    {/if}
  </section>

  <section class="box">
    <h3>{$t('wealth.real.settingsTitle')}</h3>
    {#if series.length === 0}
      <p class="muted">{$t('wealth.real.none')}</p>
    {:else}
      <div class="field">
        <span>{$t('wealth.real.series')}</span>
        <Dropdown
          bind:value={form.cpi_series_id}
          ariaLabel={$t('wealth.real.series')}
          options={[{ value: '', label: $t('wealth.real.default') }, ...series.map((s) => ({ value: s.id, label: `${s.title} (${s.provider} ${s.code})` }))]}
        />
      </div>
    {/if}
  </section>

  <ErrorText {error} />
  <div class="actions">
    {#if saved}<span class="muted">{$t('wealth.settings.saved')}</span>{/if}
    <button class="primary" onclick={save}>{$t('common.save')}</button>
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 760px;
  }
  .box {
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h3 {
    font-size: 13px;
    font-weight: 600;
    margin: 0;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
    margin: 0;
  }
  .row {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .field {
    min-width: 200px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-3);
  }
  @media (max-width: 767px) {
    .chip,
    .actions button {
      min-height: 44px;
    }
  }
</style>
