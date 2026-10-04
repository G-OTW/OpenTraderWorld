<script>
  // Frame shared by every Fundamentals page: module menu, title, the data-broker and
  // notification-channel shortcuts, and the Customize dialog (density, which sections
  // show and in what order, plus the page's own options).
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConnectorButton from '$lib/connectors/ConnectorButton.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import OrderList from './OrderList.svelte';
  import { NAV_LINKS } from './nav.js';
  import { prefs, ordered, setLayout, resetPage } from './prefs.svelte.js';
  import { t } from '$lib/i18n';

  // props: title, subtitle, page (prefs key), sections ([{ id, label }] in declared order),
  //        options (snippet, page-specific settings in the dialog), extra (snippet, header
  //        actions), children
  let { title = '', subtitle = '', page = '', sections = [], options, extra, children } = $props();

  let customizing = $state(false);

  const sectionItems = $derived(
    ordered(page, sections.map((s) => s.id)).map((id) => sections.find((s) => s.id === id))
  );
  const hidden = $derived(prefs.layout[page]?.hidden ?? []);
</script>

<RequireModule module="fundamentals">
  <div class="page" class:compact={prefs.density === 'compact'}>
    <ModuleNav links={NAV_LINKS} label="fundamentals.nav.label" />
    <PageHeader {title} {subtitle}>
      {#snippet actions()}
        {@render extra?.()}
        <Button size="sm" icon="settings" onclick={() => (customizing = true)}>{$t('fundamentals.cust.open')}</Button>
        <ConnectorButton module="fundamentals" size="sm" />
        <ChannelButton module="fundamentals" size="sm" />
      {/snippet}
    </PageHeader>
    {@render children?.()}
  </div>
</RequireModule>

<Modal bind:open={customizing} title={$t('fundamentals.cust.title', { page: title })} size="md">
  <div class="cust">
    <div class="group">
      <h3>{$t('fundamentals.cust.density')}</h3>
      <div class="fd-row">
        {#each ['comfortable', 'compact'] as d (d)}
          <button class="chip" class:active={prefs.density === d} onclick={() => (prefs.density = d)}>{$t(`fundamentals.cust.density.${d}`)}</button>
        {/each}
      </div>
    </div>

    {#if sections.length}
      <div class="group">
        <h3>{$t('fundamentals.cust.sections')}</h3>
        <p class="hint">{$t('fundamentals.cust.sectionsHint')}</p>
        <OrderList items={sectionItems} {hidden} onchange={(order, h) => setLayout(page, order, h)} />
      </div>
    {/if}

    {@render options?.()}
  </div>
  {#snippet footer()}
    <Button variant="ghost" icon="rotate-ccw" onclick={() => resetPage(page)}>{$t('fundamentals.cust.reset')}</Button>
    <Button variant="primary" onclick={() => (customizing = false)}>{$t('fundamentals.cust.done')}</Button>
  {/snippet}
</Modal>

<style>
  .page {
    --fd-gap: 20px;
    --fd-pad: 20px;
    --fd-radius: 8px;
    --fd-item-pad: 14px;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6) var(--space-8) var(--space-8);
    gap: var(--fd-gap);
    overflow-y: auto;
    min-width: 0;
    background: var(--bg);
    scrollbar-gutter: stable;
  }
  .page.compact {
    --fd-gap: var(--space-4);
    --fd-pad: var(--space-4);
    --fd-item-pad: 10px;
    padding: var(--space-6);
  }
  .page :global(.page-header) {
    margin-bottom: 0;
  }
  .page > :global(*) {
    flex-shrink: 0;
    min-width: 0;
  }

  .cust {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .cust :global(h3) {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .cust :global(.hint) {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
  }

  /* Layout helpers shared by the module's pages. */
  .page :global(.fd-grid) {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 420px), 1fr));
    gap: var(--fd-gap);
  }
  .page :global(.fd-grid > *) {
    min-width: 0;
  }
  .page :global(.fd-stack) {
    display: flex;
    flex-direction: column;
    gap: var(--fd-gap);
    min-width: 0;
  }
  .page :global(.fd-stats) {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 160px), 1fr));
    gap: var(--space-3);
  }
  .page :global(.statcard) {
    padding: var(--fd-item-pad);
    gap: var(--space-3);
    border-radius: var(--fd-radius);
    background: var(--surface-2);
    border-color: var(--border);
  }
  .page :global(.statcard .label) {
    color: var(--muted);
    font-size: 10px;
    line-height: 1.4;
  }
  .page :global(.statcard .value) {
    font-size: 24px;
    letter-spacing: -0.04em;
  }
  .page.compact :global(.statcard .value) {
    font-size: 20px;
  }
  .page :global(.fd-group + .fd-group) {
    margin-top: var(--fd-pad);
  }
  .page :global(.fd-group > h4) {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0 0 var(--space-3);
    font-size: 11px;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .page :global(.fd-group > h4::after) {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--border);
  }
  .cust :global(.fd-row),
  .page :global(.fd-row) {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .page :global(.fd-toolbar) {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3) var(--space-6);
    margin-bottom: var(--space-4);
    padding: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--fd-radius);
    background: var(--surface-2);
  }
  .page :global(.fd-label) {
    font-size: 11px;
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-right: var(--space-1);
  }
  .page :global(.fd-scroll) {
    overflow-x: auto;
    min-width: 0;
    max-width: 100%;
  }
  .page :global(.chip) {
    border-radius: 5px;
  }
  /* A chip in an open-by-ticker toolbar sits beside the input and its button: one height. */
  .page :global(form.fd-row .chip) {
    height: var(--control-h);
  }
  .page :global(.chip.active) {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    border-color: color-mix(in srgb, var(--accent) 42%, var(--border));
    color: var(--text);
  }
  .cust :global(.fd-muted),
  .page :global(.fd-muted) {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .page :global(.fd-note) {
    margin: var(--space-4) 0 0;
    color: var(--dim);
    font-size: var(--text-sm);
  }
  .page :global(.fd-up) {
    color: var(--green-ink, var(--green));
  }
  .page :global(.fd-down) {
    color: var(--red-ink, var(--red));
  }
  .page :global(.fd-kv) {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.5fr);
    gap: 0 var(--space-6);
    font-size: var(--text-sm);
    margin: 0;
  }
  .page :global(.fd-kv dt) {
    color: var(--muted);
  }
  .page :global(.fd-kv dt),
  .page :global(.fd-kv dd) {
    padding-block: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    overflow-wrap: anywhere;
  }
  .page :global(.fd-kv dt:last-of-type),
  .page :global(.fd-kv dd:last-of-type) {
    border-bottom: 0;
  }
  .page :global(.fd-kv dd) {
    margin: 0;
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .page :global(table.tbl td),
  .page :global(table.tbl th) {
    padding-inline: var(--space-4);
  }
  .page :global(table.tbl th) {
    background: var(--surface-2);
    color: var(--muted);
    font-size: 10px;
    letter-spacing: 0.05em;
    padding-block: var(--space-3);
    white-space: nowrap;
  }
  .page :global(table.tbl td) {
    border-bottom: var(--hairline) solid var(--border);
    line-height: 1.5;
  }
  .page :global(table.tbl tbody tr:last-child td) {
    border-bottom: 0;
  }
  .page :global(table.tbl tbody tr:hover td) {
    background: var(--surface-2);
  }
  .page :global(table.tbl td.num) {
    font-family: var(--mono);
    white-space: nowrap;
  }
  .page:not(.compact) :global(table.tbl td) {
    height: 46px;
  }

  @media (max-width: 640px) {
    .page,
    .page.compact {
      padding: var(--space-4);
      --fd-gap: var(--space-4);
      --fd-pad: var(--space-4);
    }
  }
</style>
