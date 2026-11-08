<script>
  // Asset templates: one tile per template on a hairline grid (built-ins are read only).
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { assetTypeLabel } from './api.js';
  import { t } from '$lib/i18n';

  let { templates = [], onadd = () => {}, onedit = () => {}, ondelete = () => {} } = $props();
</script>

{#if templates.length === 0}
  <EmptyState icon="layers" title={$t('wealth.page.noTemplatesTitle')} description={$t('wealth.page.noTemplatesBody')}>
    {#snippet action()}
      <button class="primary" onclick={onadd}><Icon name="plus" size={15} /> {$t('wealth.page.newTemplate')}</button>
    {/snippet}
  </EmptyState>
{:else}
  <div class="tpl-grid">
    {#each templates as tpl (tpl.id)}
      <div class="tcard">
        <div class="tcard-head">
          <span class="tname">{tpl.name}</span>
          {#if tpl.is_builtin}<span class="badge">{$t('wealth.page.builtIn')}</span>{/if}
        </div>
        {#if tpl.description}<p class="tdesc">{tpl.description}</p>{/if}
        <p class="count">{$t('wealth.page.fieldsCount', { count: tpl.fields?.length ?? 0, type: assetTypeLabel(tpl.asset_type) })}</p>
        {#if !tpl.is_builtin}
          <div class="tcard-actions">
            <button class="link" onclick={() => onedit(tpl)}>{$t('wealth.page.editLink')}</button>
            <button class="link danger" onclick={() => ondelete(tpl)}>{$t('wealth.page.deleteLink')}</button>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .tpl-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--hairline);
    background: var(--border);
    border: var(--hairline) solid var(--border);
  }
  .tcard {
    background: var(--bg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: background-color 160ms ease;
  }
  .tcard:hover {
    background: var(--surface-hover);
  }
  .tcard-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .tname {
    font-size: 13px;
    font-weight: 600;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .tdesc {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
    line-height: var(--lh-base);
  }
  .count {
    margin: 0;
    color: var(--dim);
    font-size: 11px;
  }
  .tcard-actions {
    display: flex;
    gap: var(--space-3);
    margin-top: auto;
    padding-top: var(--space-2);
  }
  .link {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
  }
  .link:hover {
    color: var(--text);
  }
  .link.danger {
    color: var(--red-ink);
  }
  @media (max-width: 767px) {
    .tpl-grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .tcard-actions {
      flex-wrap: wrap;
    }
    .link {
      min-height: 44px;
    }
  }
</style>
