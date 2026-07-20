<script>
  // The title block every route opens with. Replaces the ad-hoc <h1> each module
  // grew its own way, so page titles share one size, one weight, one rhythm.
  //
  //   <PageHeader title="Journal" subtitle="42 trades">
  //     {#snippet actions()}<Button variant="primary">New trade</Button>{/snippet}
  //   </PageHeader>
  // props: title, subtitle, actions (snippet), children (extra row under the title)
  let { title = '', subtitle = '', actions, children } = $props();
</script>

<header class="page-header">
  <div class="row">
    <div class="titles">
      <h1>{title}</h1>
      {#if subtitle}<p>{subtitle}</p>{/if}
    </div>
    {#if actions}
      <div class="actions">{@render actions()}</div>
    {/if}
  </div>
  {@render children?.()}
</header>

<style>
  .page-header {
    margin-bottom: var(--space-6);
  }

  .row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .titles {
    min-width: 0;
  }

  h1 {
    margin: 0;
    color: var(--text);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
    line-height: var(--lh-tight);
  }
  p {
    margin: var(--space-1) 0 0;
    color: var(--dim);
    font-size: var(--text-sm);
    line-height: var(--lh-tight);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
  }
</style>
