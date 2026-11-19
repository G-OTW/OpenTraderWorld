<script>
  // Why a block has no answer, in the user's words.
  //
  // Never a zero-filled table: a reader cannot tell a measured zero from a missing one, and
  // both occur here. Each reason carries what to do about it, because "no history" and "no
  // benchmark" are fixed in completely different places. It is drawn as a panel at a
  // chart's height, so the tab does not jump between an answer and its absence.
  import { t } from '$lib/i18n';
  import Panel from './Panel.svelte';

  let { missing = [], sample = null, onfix = null, title = '', height = 240 } = $props();

  // Reasons that have a button behind them. The rest are stated and left alone.
  const FIXABLE = { no_history: 'history', no_bars: 'history', no_benchmark: 'benchmark', no_targets: 'targets' };
  const first = $derived(missing[0] ?? 'unknown');
  const action = $derived(FIXABLE[first] ?? null);
  const how = $derived(
    $t(`portfolios.analytics.missingHow.${first}`) +
      (sample?.rows ? ` ${$t('portfolios.analytics.sampleRows', { n: sample.rows })}.` : '')
  );
</script>

<Panel
  {title}
  {height}
  unavailable={{
    title: $t(`portfolios.analytics.missing.${first}`),
    reason: how,
    icon: 'info',
    action: action && onfix ? { label: $t(`portfolios.analytics.fix.${action}`), onclick: () => onfix(action) } : null
  }}
/>
