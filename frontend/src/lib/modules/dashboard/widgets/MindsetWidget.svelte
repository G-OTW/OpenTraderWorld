<script>
  // Mindset widget: today's pre/post-mortem check-in status. Shows whether each phase is
  // filled; the header redirect opens the full check-in. (Filling happens in the module.)
  import { mindsetApi, PHASES } from '$lib/modules/mindset/api.js';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();

  // Read the day at fetch time, not at module init: a dashboard left open across
  // midnight would otherwise keep asking for yesterday. dateKey() builds the key from
  // local parts — `toLocaleDateString('en-CA')` worked only because that locale happens
  // to order its parts as ISO does.
  let day = $state(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      day = await mindsetApi.day(dateKey());
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  function answered(phaseKey) {
    const e = (day?.entries ?? []).find((x) => x.phase === phaseKey);
    return e && e.answers && Object.keys(e.answers).length > 0;
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.mindset.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if day === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else}
  <ul class="phases">
    {#each PHASES as p (p.key)}
      <li class="phase">
        <span class="ic">{p.icon}</span>
        <span class="lbl">{p.label}</span>
        <span class="status" class:done={answered(p.key)}>{answered(p.key) ? $t('dashboard.widgets.mindset.done') : $t('dashboard.widgets.mindset.notFilled')}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .sk {
    padding: var(--space-1) 0;
  }
  /* Preview, loading and empty text — not an error. This was grouped with a
     now-removed .err rule and inherited its red. */
  .hint {
    color: var(--muted);
  }
  .phases {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .phase {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
  }
  .lbl {
    flex: 1;
  }
  .status {
    font-size: var(--text-xs);
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px var(--space-2);
  }
  .status.done {
    color: var(--green);
    border-color: var(--green);
  }
</style>
