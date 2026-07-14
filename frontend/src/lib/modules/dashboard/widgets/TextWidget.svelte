<script>
  // Free-text widget: renders the configured body as simple paragraphs. The title lives in
  // the shell header (config.title); the body is plain text with blank-line paragraphs.
  import { t } from '$lib/i18n';

  let { item } = $props();
  const body = $derived(item.config?.body ?? '');
  const paras = $derived(body.split(/\n{2,}/).map((p) => p.trim()).filter(Boolean));
</script>

{#if paras.length}
  <div class="text">
    {#each paras as p}
      <p>{p}</p>
    {/each}
  </div>
{:else}
  <p class="empty">{$t('dashboard.widgets.text.empty')}</p>
{/if}

<style>
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-size: var(--text-base);
    line-height: 1.5;
    white-space: pre-wrap;
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-base);
  }
</style>
