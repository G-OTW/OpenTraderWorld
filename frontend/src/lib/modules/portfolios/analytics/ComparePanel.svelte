<script>
  // `compare`: this book and another on one curve, both rebased to 100 on the first day
  // they both have. Only common days are drawn: a younger book is not flat before it began.
  import { t } from '$lib/i18n';
  import { fmtFixed } from '$lib/format.js';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import Unavailable from './Unavailable.svelte';
  import Panel from './Panel.svelte';
  import { portfoliosApi } from '../api.js';

  let { portfolioId, window = 'inception', onfix = null } = $props();

  let others = $state([]);
  let other = $state('');
  let data = $state(null);
  let loading = $state(false);
  let error = $state(null);

  $effect(() => {
    portfoliosApi
      .list()
      .then((r) => {
        others = r.filter((p) => p.id !== portfolioId);
        if (!other && others.length) other = others[0].id;
      })
      .catch((e) => (error = e.message));
  });

  $effect(() => {
    if (!other) return;
    loading = true;
    portfoliosApi
      .compare(portfolioId, other, window)
      .then((r) => (data = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  const pointsOf = (vals) => (data?.series?.dates ?? []).map((ts, i) => [ts, vals?.[i] ?? null]);
</script>

<ErrorText {error} />
{#if others.length === 0}
  <Panel title={$t('portfolios.analytics.block.compare')} height={240} unavailable={{ reason: $t('portfolios.compare.noOther'), icon: 'git-compare' }} />
{:else if data && data.status !== 'ok' && !loading}
  <Unavailable missing={data.missing ?? []} {onfix} />
{:else}
  <Panel
    title={$t('portfolios.analytics.block.compare')}
    subtitle={$t('portfolios.compare.hint')}
    height={260}
    loading={loading && !data}
  >
    {#snippet actions()}
      <div class="pick">
        <span>{$t('portfolios.compare.against')}</span>
        <Select bind:value={other} options={others.map((p) => ({ value: p.id, label: p.name }))} />
      </div>
    {/snippet}
    {#if data}
      <TimeSeries
        series={[
          { key: 'a', label: data.a.name, points: pointsOf(data.series.a) },
          { key: 'b', label: data.b.name, points: pointsOf(data.series.b) }
        ]}
        baseline={100}
        height={260}
        change="pct"
        format={(v) => fmtFixed(v, 1)}
        axisFormat={(v) => fmtFixed(v, 0)}
        sensitive={false}
      />
    {/if}
  </Panel>
{/if}

<style>
  .pick {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
    color: var(--dim);
    min-width: 220px;
  }
  .pick :global(.field),
  .pick > :global(:last-child) {
    flex: 1;
    min-width: 0;
  }
</style>
