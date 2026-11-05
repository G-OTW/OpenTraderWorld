<script>
  // Volatility tab: the IV surface and IV history panels of Quant, reused as they are and
  // spent on the connectors granted to Options.
  import OptionSurfacePanel from '$lib/modules/quant/OptionSurfacePanel.svelte';
  import IvHistoryPanel from '$lib/modules/quant/IvHistoryPanel.svelte';
  import InfoNote from './InfoNote.svelte';
  import { optionsApi } from './api.js';
  import { t } from '$lib/i18n';
</script>

<div class="vol">
  <section>
    <h2>{$t('options.vol.surface')} <InfoNote note={$t('options.vol.modelNote')} /></h2>
    <OptionSurfacePanel module="options" api={optionsApi.surface} />
  </section>
  <section>
    <h2>{$t('options.vol.history')}</h2>
    <IvHistoryPanel module="options" api={optionsApi.ivHistory} />
  </section>
</div>

<style>
  .vol {
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
    min-width: 0;
    container: quant-page / inline-size;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
  }
  @media (max-width: 767px) {
    .vol {
      gap: var(--space-6);
    }
  }
</style>
