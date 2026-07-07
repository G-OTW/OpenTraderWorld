<script>
  // Mindset widget: today's pre/post-mortem check-in status. Shows whether each phase is
  // filled; the header redirect opens the full check-in. (Filling happens in the module.)
  import { mindsetApi, PHASES } from '$lib/modules/mindset/api.js';
  import { t } from '$lib/i18n';

  let { item, editing } = $props();

  const today = new Date().toLocaleDateString('en-CA');
  let day = $state(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      day = await mindsetApi.day(today);
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
  <p class="err">{err}</p>
{:else if day === null}
  <p class="hint">{$t('common.loading')}</p>
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
  .hint,
  .err {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .err {
    color: var(--red);
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
    font-size: 0.85rem;
  }
  .lbl {
    flex: 1;
  }
  .status {
    font-size: 0.72rem;
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
