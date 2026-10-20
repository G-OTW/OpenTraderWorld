<script>
  // One earnings call, read by speaker: prepared remarks then Q&A, analysts set apart
  // from management, with an in-document search that highlights and counts matches.
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Section from './Section.svelte';
  import { fundamentalsApi } from './api.js';
  import { t } from '$lib/i18n';

  let { doc } = $props();
  let segs = $state([]);
  let words = $state(null);
  let loading = $state(true);
  let error = $state('');
  let q = $state('');
  let section = $state('');

  // The stored text, or fetched from the provider the first time the call is opened.
  $effect(() => {
    loading = true;
    error = '';
    fundamentalsApi
      .transcript(doc.id)
      .then((d) => {
        segs = d.segments;
        words = d.words;
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  const shown = $derived(segs.filter((s) => !section || s.section === section));
  const hits = $derived(q.length > 1 ? shown.reduce((n, s) => n + (s.text.toLowerCase().split(q.toLowerCase()).length - 1), 0) : 0);

  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  function mark(text) {
    const safe = esc(text);
    if (q.length < 2) return safe;
    const re = new RegExp(esc(q).replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
    return safe.replace(re, (m) => `<mark>${m}</mark>`);
  }
</script>

<Section title={doc.title} description={`${doc.filed_at}${words ? ` · ${words.toLocaleString()} ${$t('fundamentals.tr.words')}` : ''}`} source={doc.source}>
  <div class="fd-row bar">
    <button class="chip" class:active={!section} onclick={() => (section = '')}>{$t('fundamentals.all')}</button>
    <button class="chip" class:active={section === 'prepared'} onclick={() => (section = 'prepared')}>{$t('fundamentals.tr.prepared')}</button>
    <button class="chip" class:active={section === 'qa'} onclick={() => (section = 'qa')}>{$t('fundamentals.tr.qa')}</button>
    <input type="search" placeholder={$t('fundamentals.tr.search')} bind:value={q} />
    {#if q.length > 1}<span class="fd-muted">{$t('fundamentals.tr.hits', { n: hits })}</span>{/if}
  </div>
  {#if error}<ErrorText {error} />{/if}
  {#if loading}<Skeleton rows={6} />{/if}
  <div class="body">
    {#each shown as s (s.ordinal)}
      <div class="seg {s.role}">
        <div class="who">
          <strong>{s.speaker}</strong>
          <Badge tone={s.role === 'analyst' ? 'accent' : 'neutral'}>{$t(`fundamentals.tr.role.${s.role}`)}</Badge>
        </div>
        <!-- eslint-disable-next-line svelte/no-at-html-tags: escaped above, only <mark> is added -->
        <p>{@html mark(s.text)}</p>
      </div>
    {/each}
  </div>
</Section>

<style>
  .bar {
    margin-bottom: var(--space-6);
  }
  .bar input {
    flex: 1;
    min-width: 160px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-height: 70vh;
    overflow-y: auto;
  }
  .seg {
    padding-left: var(--space-4);
    max-width: 78ch;
    border-left: 2px solid var(--border);
  }
  .seg.analyst {
    border-left-color: var(--accent);
  }
  .seg.operator {
    opacity: 0.7;
  }
  .who {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  p {
    margin: var(--space-1) 0 0;
    font-size: var(--text-sm);
    line-height: 1.7;
  }
  p :global(mark) {
    background: color-mix(in srgb, var(--amber) 35%, transparent);
    color: inherit;
    border-radius: 2px;
  }
</style>
