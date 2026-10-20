<script>
  // A titled, collapsible block. Every Fundamentals panel is one, so each says what it
  // shows (title + one-line description + source) and the user can fold it away.
  //
  //   <Section key="macro.curve" title="Yield curve" description="..." source="Treasury">
  //     {#snippet actions()}...{/snippet}
  //     ...content
  //   </Section>
  // props: key (collapse memory, `page.section`), title, description, source,
  //        actions (snippet), children
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import { isCollapsed, toggleCollapsed } from './prefs.svelte.js';
  import { t } from '$lib/i18n';

  let { key = '', title = '', description = '', source = '', actions, children } = $props();

  const folded = $derived(key ? isCollapsed(key) : false);
</script>

<section class="fd-section" class:folded>
  <header>
    <button class="toggle" onclick={() => key && toggleCollapsed(key)} aria-expanded={!folded} disabled={!key}>
      {#if key}<Icon name={folded ? 'chevron-right' : 'chevron-down'} size={14} />{/if}
      <span class="titles">
        <span class="title">{title}{#if source}<Badge>{source}</Badge>{/if}</span>
        {#if description && !folded}<span class="desc">{description}</span>{/if}
      </span>
    </button>
    {#if actions && !folded}
      <div class="actions">{@render actions()}</div>
    {/if}
  </header>
  {#if !folded}
    <div class="body">{@render children?.()}</div>
  {:else}
    <span class="sr-only">{$t('fundamentals.collapsed')}</span>
  {/if}
</section>

<style>
  .fd-section {
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--fd-radius, 8px);
    min-width: 0;
    box-shadow: var(--shadow-1);
  }
  .fd-section.folded header {
    border-bottom: 0;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    padding: var(--fd-pad, var(--space-6));
    border-bottom: var(--hairline) solid var(--border);
    border-radius: var(--fd-radius, 8px) var(--fd-radius, 8px) 0 0;
    background: color-mix(in srgb, var(--surface-2) 55%, var(--surface));
  }
  .toggle {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    min-width: 0;
    flex: 1 1 240px;
  }
  .toggle:disabled {
    cursor: default;
  }
  .toggle:not(:disabled):hover .title {
    color: var(--accent);
  }
  .toggle :global(svg) {
    margin-top: 3px;
    color: var(--dim);
    flex-shrink: 0;
  }
  .titles {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .title {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .desc {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.6;
    max-width: 80ch;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .body {
    padding: var(--fd-pad, var(--space-6));
    min-width: 0;
    overflow-x: auto;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
