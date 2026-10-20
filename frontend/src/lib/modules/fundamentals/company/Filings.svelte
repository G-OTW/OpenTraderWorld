<script>
  // SEC filings for one company, filtered by form type.
  import Section from '../Section.svelte';
  import DocumentList from '../DocumentList.svelte';
  import { fundamentalsApi } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let rows = $state([]);

  $effect(() => {
    fundamentalsApi.filings(company.ticker).then((x) => (rows = x));
  });
</script>

<Section key="company.filings" title={$t('fundamentals.tab.filings')} description={$t('fundamentals.doc.filingsDesc')} source="SEC EDGAR">
  <DocumentList docs={rows} />
</Section>
