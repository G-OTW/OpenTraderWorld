<script>
  // Economic Calendar module — TradingView's economic-calendar widget.
  import EconomicCalendar from '$lib/modules/economics/EconomicCalendar.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import { t } from '$lib/i18n';
</script>

<div class="econ-page">
  <header class="head">
    <h1>{$t('economics.page.title')}</h1>
    <div class="head-actions">
      <QuickReminderButton title={$t('economics.page.addReminder')} />
    </div>
  </header>
  <div class="content">
    <EconomicCalendar colorTheme="dark" />
  </div>
</div>

<style>
  .econ-page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  .head-actions {
    margin-left: auto;
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .content {
    flex: 1;
    min-height: 0;
  }
</style>
