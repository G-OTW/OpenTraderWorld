<script>
  // A new robot: from a backtest run (`?run=`), a saved strategy (`?strategy=`) or nothing.
  import { page } from '$app/state';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import RobotWizard from '$lib/modules/robots/RobotWizard.svelte';
  import { t } from '$lib/i18n';

  const runId = page.url.searchParams.get('run');
  const strategyId = page.url.searchParams.get('strategy');
</script>

<RequireModule module="robots">
  <div class="page">
    <PageHeader eyebrow={$t('robots.title')} title={$t('robots.wizard.title')} subtitle={$t('robots.wizard.subtitle')}>
      {#snippet actions()}<a class="btn sm" href="/robots">{$t('robots.wizard.cancel')}</a>{/snippet}
    </PageHeader>
    <RobotWizard {runId} {strategyId} />
  </div>
</RequireModule>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  @media (max-width: 767px) {
    .page {
      min-width: 0;
      overflow-x: clip;
      padding: var(--space-4) var(--space-4) calc(72px + env(safe-area-inset-bottom));
    }
  }
</style>
