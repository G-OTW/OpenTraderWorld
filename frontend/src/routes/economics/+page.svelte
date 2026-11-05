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
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .content {
    flex: 1;
    min-height: 0;
  }
  /* Phone: the page scrolls and the calendar takes about one screen of height (it scrolls
     its own list), so it is never cut at the bottom of a short or landscape viewport. */
  @media (max-width: 767px) {
    .econ-page {
      padding: var(--space-3) var(--space-3) calc(72px + env(safe-area-inset-bottom));
      gap: var(--space-3);
      overflow-y: auto;
    }
    h1 {
      min-width: 0;
      overflow-wrap: anywhere;
    }
    .content {
      flex: none;
      height: max(300px, calc(100dvh - 140px));
    }
  }
</style>
