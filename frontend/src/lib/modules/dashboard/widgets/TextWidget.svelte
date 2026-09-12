<script>
  // Free-text widget: renders the configured body as simple paragraphs. The title lives in
  // the shell header (config.title); the body is plain text with blank-line paragraphs.
  // Light formatting (size, color, highlight, weight/italic) applies to the whole note:
  // it is a note, not a document, so per-run markup would cost more than it buys.
  import { t } from '$lib/i18n';

  let { item } = $props();
  const body = $derived(item.config?.body ?? '');
  const paras = $derived(body.split(/\n{2,}/).map((p) => p.trim()).filter(Boolean));
  const size = $derived(item.config?.size ?? 'base');
  const color = $derived(item.config?.color ?? 'muted');
  const highlight = $derived(item.config?.highlight ?? 'none');
  const style = $derived(item.config?.textStyle ?? 'normal');
</script>

{#if paras.length}
  <div class="text s-{size} c-{color} t-{style}" class:hl={highlight !== 'none'} style:--hl="var(--{highlight === 'none' ? 'surface-2' : highlight})">
    {#each paras as p}
      <p><span>{p}</span></p>
    {/each}
  </div>
{:else}
  <p class="w-state">{$t('dashboard.widgets.text.empty')}</p>
{/if}

<style>
  /* A note is prose, so it gets prose measure and leading rather than the dense
     row rhythm the data widgets use. */
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 68ch;
    font-size: var(--text-sm);
    line-height: var(--lh-base);
    color: var(--muted);
    white-space: pre-wrap;
  }
  .text p {
    margin: 0;
  }

  .s-xs { font-size: var(--text-xs); }
  .s-sm { font-size: var(--text-sm); }
  .s-base { font-size: var(--text-base); }
  .s-lg { font-size: var(--text-lg); }
  .s-xl { font-size: var(--text-xl); }

  .c-muted { color: var(--muted); }
  .c-text { color: var(--text); }
  .c-accent { color: var(--accent); }
  .c-green { color: var(--green); }
  .c-red { color: var(--red); }
  .c-amber { color: var(--amber); }

  .t-bold { font-weight: var(--fw-semibold); }
  .t-italic { font-style: italic; }
  .t-bold-italic { font-weight: var(--fw-semibold); font-style: italic; }

  /* The tint hugs the text lines, not the widget body, so a short note does not paint
     a full-width block. Cloned decoration keeps the padding on every wrapped line. */
  .hl span {
    background: color-mix(in srgb, var(--hl) 22%, transparent);
    padding: 0.1em 0.3em;
    border-radius: var(--radius);
    box-decoration-break: clone;
    -webkit-box-decoration-break: clone;
  }
</style>
