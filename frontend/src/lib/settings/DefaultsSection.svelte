<script>
  // Global defaults — currency and timezone. Modules will read these later for their own
  // defaults; for now they are just stored.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { t, locale, LOCALES, setLocale } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  const CURRENCIES = ['USD', 'EUR', 'GBP', 'JPY', 'CHF', 'CAD', 'AUD', 'CNY', 'HKD', 'SGD'];

  let currency = $state('USD');
  let timezone = $state('UTC');
  let language = $state($locale);
  let zones = $state(['UTC']);
  let loading = $state(true);
  let saving = $state(false);
  let ok = $state('');
  let error = $state('');

  onMount(async () => {
    // Browser-known timezones (offline, no network); fall back to a small list.
    try {
      zones = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : ['UTC'];
    } catch {
      zones = ['UTC'];
    }
    try {
      const d = await settingsApi.getDefaults();
      currency = d.default_currency;
      timezone = d.default_timezone;
      if (d.locale) language = d.locale;
      if (!zones.includes(timezone)) zones = [timezone, ...zones];
    } finally {
      loading = false;
    }
  });

  async function save() {
    ok = '';
    error = '';
    saving = true;
    try {
      await settingsApi.setDefaults({ default_currency: currency, default_timezone: timezone });
      ok = $t('settings.defaults.saved');
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Language applies live (re-renders the whole app) and persists on its own.
  function changeLanguage(code) {
    language = code;
    setLocale(code);
  }
</script>

<div class="section">
  <h2>{$t('settings.defaults.title')}</h2>
  <p class="muted small">{$t('settings.defaults.subtitle')}</p>
  {#if loading}
    <!-- Label + control pairs, the shape of the fields below. -->
    <div class="form" aria-busy="true">
      {#each Array.from({ length: 5 }, (_, i) => i) as i (i)}
        <div class="field">
          <Skeleton height="0.85rem" width="28%" />
          <Skeleton height="2.25rem" />
        </div>
      {/each}
    </div>
  {:else}
    <label class="field lang">
      <span>{$t('settings.defaults.language')}</span>
      <select value={language} onchange={(e) => changeLanguage(e.currentTarget.value)}>
        {#each LOCALES as l}<option value={l.code}>{l.flag} {l.label}</option>{/each}
      </select>
      <span class="muted small hint">{$t('settings.defaults.languageHint')}</span>
    </label>

    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <label class="field">
        <span>{$t('settings.defaults.currency')}</span>
        <select bind:value={currency}>
          {#each CURRENCIES as c}<option value={c}>{c}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{$t('settings.defaults.timezone')}</span>
        <select bind:value={timezone}>
          {#each zones as z}<option value={z}>{z}</option>{/each}
        </select>
      </label>

      <ErrorText error={error} />
      {#if ok}<p class="ok">{ok}</p>{/if}

      <div class="actions">
        <button type="submit" class="primary" disabled={saving}>
          {saving ? $t('common.saving') : $t('common.save')}
        </button>
      </div>
    </form>
  {/if}
</div>

<style>
  .section {
    max-width: 480px;
  }
  h2 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-md);
    color: var(--text);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }
  .lang {
    margin-top: var(--space-4);
    margin-bottom: var(--space-6);
    padding-bottom: var(--space-4);
    border-bottom: 1px solid var(--border);
  }
  .hint {
    margin-top: var(--space-1);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
  .ok {
    color: var(--green-ink);
    font-size: var(--text-base);
    margin: 0;
  }
</style>
