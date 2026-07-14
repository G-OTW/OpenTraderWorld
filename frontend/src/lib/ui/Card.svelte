<script>
  // Level-1 surface: content sitting on the page. Border, no shadow — the two
  // together are the tell of a template. Shadows belong to level 2 (Modal,
  // dropdowns), which float over the page rather than resting on it.
  //
  //   <Card title="Open positions">{#snippet actions()}<Button …/>{/snippet} … </Card>
  // props: title, subtitle, padded, actions (snippet), children
  let { title = '', subtitle = '', padded = true, actions, children, ...rest } = $props();
</script>

<!-- `ui-card`, not `card`: the global .card layer (theme/components.css) still
     dresses 17 un-migrated files, and two rules on one class is how a restyle
     turns into a debugging session. -->
<section class="ui-card" {...rest}>
  {#if title || actions}
    <header>
      <div class="titles">
        {#if title}<h2>{title}</h2>{/if}
        {#if subtitle}<p>{subtitle}</p>{/if}
      </div>
      {#if actions}
        <div class="actions">{@render actions()}</div>
      {/if}
    </header>
  {/if}

  <div class="body" class:padded>
    {@render children?.()}
  </div>
</section>

<style>
  .ui-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    min-width: 0; /* let it shrink inside a grid track */
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-4);
    border-bottom: 1px solid var(--border);
  }
  .titles {
    min-width: 0;
  }

  h2 {
    margin: 0;
    color: var(--text);
    font-size: var(--text-md);
    font-weight: var(--fw-semibold);
    line-height: var(--lh-tight);
  }
  p {
    margin: var(--space-1) 0 0;
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: var(--lh-tight);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
  }

  /* Tables and lists want to bleed to the border; everything else wants padding. */
  .body.padded {
    padding: var(--space-4);
  }
</style>
