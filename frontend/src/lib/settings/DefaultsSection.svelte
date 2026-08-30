<script>
  // Global defaults — currency and timezone. Modules will read these later for their own
  // defaults; for now they are just stored.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { t, locale, LOCALES, setLocale } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
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
    <div class="field lang">
      <span>{$t('settings.defaults.language')}</span>
      <Dropdown
        value={language}
        onpick={changeLanguage}
        ariaLabel={$t('settings.defaults.language')}
        options={LOCALES.map((l) => ({ value: l.code, label: `${l.flag} ${l.label}` }))}
      />
      <span class="muted small hint">{$t('settings.defaults.languageHint')}</span>
    </div>

    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <div class="field">
        <span>{$t('settings.defaults.currency')}</span>
        <Dropdown
          bind:value={currency}
          ariaLabel={$t('settings.defaults.currency')}
          options={CURRENCIES.map((c) => ({ value: c, label: c }))}
        />
      </div>
      <div class="field">
        <span>{$t('settings.defaults.timezone')}</span>
        <Dropdown
          bind:value={timezone}
          ariaLabel={$t('settings.defaults.timezone')}
          searchPlaceholder={$t('settings.defaults.timezone')}
          options={zones.map((z) => ({ value: z, label: z }))}
        />
      </div>

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
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
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
    border-bottom: var(--hairline) solid var(--border);
  }
  .hint {
    margin-top: var(--space-1);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .ok {
    color: var(--green-ink);
    font-size: var(--text-base);
    margin: 0;
  }
</style>
