<script>
  // Chart cache: how long this browser keeps the bars a chart loaded (until the tab closes, or
  // 1 h to 7 days), and whether a logout clears them. Kept per browser, like the cache itself
  // (see histviz/seriesCache.js).
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import { RETENTIONS, seriesCacheConfig, configureSeriesCache } from '$lib/modules/histviz/seriesCache.js';

  let cfg = $state(seriesCacheConfig());

  function set(patch) {
    configureSeriesCache({ ...cfg, ...patch });
    cfg = seriesCacheConfig();
  }
</script>

<div class="section">
  <h2>{$t('settings.chartCache.title')}</h2>
  <p class="muted small">{$t('settings.chartCache.subtitle')}</p>

  <div class="rows">
    <div class="row">
      <div class="txt">
        <span class="name">{$t('settings.chartCache.retention')}</span>
        <span class="muted small">{$t('settings.chartCache.retentionDesc')}</span>
      </div>
      <span class="ctl">
        <Dropdown
          value={cfg.retention}
          ariaLabel={$t('settings.chartCache.retention')}
          float
          options={Object.keys(RETENTIONS).map((v) => ({ value: v, label: $t(`settings.chartCache.keep_${v}`) }))}
          onpick={(v) => set({ retention: v })}
        />
      </span>
    </div>
    <div class="row">
      <div class="txt">
        <span class="name">{$t('settings.chartCache.clearOnLogout')}</span>
        <span class="muted small">{$t('settings.chartCache.clearOnLogoutDesc')}</span>
      </div>
      <label class="switch" class:on={cfg.clearOnLogout}>
        <input
          type="checkbox"
          checked={cfg.clearOnLogout}
          aria-label={$t('settings.chartCache.clearOnLogout')}
          onchange={(e) => set({ clearOnLogout: e.currentTarget.checked })}
        />
        <span>{cfg.clearOnLogout ? $t('settings.chartCache.on') : $t('settings.chartCache.off')}</span>
      </label>
    </div>
  </div>
</div>

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
  .ctl {
    flex: none;
    min-width: 200px;
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
</style>
