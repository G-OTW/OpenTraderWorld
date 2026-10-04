<script>
  // Fundamentals: Documents. Every stored filing and transcript across companies, with a
  // full-text search run by the backend (titles now, bodies once they are fetched).
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import Section from '$lib/modules/fundamentals/Section.svelte';
  import DocumentList from '$lib/modules/fundamentals/DocumentList.svelte';
  import TranscriptReader from '$lib/modules/fundamentals/TranscriptReader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fundamentalsApi } from '$lib/modules/fundamentals/api.js';
  import { t } from '$lib/i18n';

  let docs = $state([]);
  let q = $state('');
  let open = $state(null);

  let error = $state('');

  $effect(() => {
    const text = q.trim().length > 1 ? q : '';
    const timer = setTimeout(() => {
      fundamentalsApi
        .documents(text)
        .then((d) => ((docs = d), (error = '')))
        .catch((e) => (error = e.message));
    }, 250);
    return () => clearTimeout(timer);
  });
</script>

<Shell title={$t('fundamentals.docs.title')} subtitle={$t('fundamentals.docs.subtitle')} page="documents">
  {#if open}
    <div><Button size="sm" icon="arrow-left" onclick={() => (open = null)}>{$t('fundamentals.back')}</Button></div>
    <TranscriptReader doc={open} />
  {:else}
    <Section title={$t('fundamentals.docs.library')} description={$t('fundamentals.docs.libraryDesc', { n: docs.length })}>
      {#snippet actions()}
        <input class="q" type="search" placeholder={$t('fundamentals.docs.search')} bind:value={q} />
      {/snippet}
      {#if error}<ErrorText {error} />{/if}
      <DocumentList {docs} showTicker onopen={(d) => (open = d)} />
    </Section>
  {/if}
</Shell>

<style>
  .q {
    width: min(320px, 100%);
  }
</style>
