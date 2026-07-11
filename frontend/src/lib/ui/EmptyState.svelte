<script>
  import Icon from './Icon.svelte';

  // An empty list is a question, not a dead end. Answer three things: what belongs
  // here, why it isn't here yet, and the one action that fixes it. A bare
  // "No data" in grey is an admission that nobody thought about this screen.
  //
  //   <EmptyState icon="candlestick" title="No trades yet"
  //               description="Import a statement or log your first trade.">
  //     {#snippet action()}<Button variant="primary" icon="plus">New trade</Button>{/snippet}
  //   </EmptyState>
  //
  // `description` is plain text. When the copy carries markup (an i18n string with
  // <strong>, a link), pass the `body` snippet instead — the styling is identical.
  // props: icon, title, description, body (snippet), action (snippet), compact
  let { icon = 'inbox', title = '', description = '', body, action, compact = false } = $props();
</script>

<!-- `ui-empty`, not `empty`: a global .empty layer still dresses un-migrated pages. -->
<div class="ui-empty" class:compact>
  <span class="icon" aria-hidden="true">
    <Icon name={icon} size={compact ? 20 : 24} strokeWidth={1.5} />
  </span>

  {#if title}<p class="title">{title}</p>{/if}
  {#if body}
    <div class="desc">{@render body()}</div>
  {:else if description}
    <p class="desc">{description}</p>
  {/if}

  {#if action}
    <div class="action">{@render action()}</div>
  {/if}
</div>

<style>
  .ui-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: var(--space-2);
    padding: var(--space-8) var(--space-4);
  }
  .ui-empty.compact {
    padding: var(--space-6) var(--space-4);
  }

  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    margin-bottom: var(--space-1);
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }
  .compact .icon {
    width: 36px;
    height: 36px;
  }

  .title {
    margin: 0;
    color: var(--text);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
  }

  .desc {
    margin: 0;
    max-width: 42ch; /* a readable measure, not the full container width */
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  /* Emphasis inside a `body` snippet (i18n copy names a menu path in <strong>). */
  .desc :global(strong) {
    color: var(--text);
    font-weight: var(--fw-medium);
  }

  .action {
    margin-top: var(--space-2);
  }
</style>
