<script>
  // Mindset widget: today's check-in status, one row per active template. Shows whether
  // each is filled; the header redirect opens the full check-in. (Filling happens in the
  // module.) A day can hold several check-ins now, so this lists templates rather than the
  // two fixed phases it used to assume.
  import { mindsetApi, PHASES, isAnswered } from '$lib/modules/mindset/api.js';
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

  const templates = $derived(day?.templates ?? []);

  /// A check-in counts as done once every one of its prompts carries an answer.
  function answered(tpl) {
    const answers = (day?.entries ?? []).find((x) => x.template_id === tpl.id)?.answers ?? {};
    const prompts = (day?.prompts ?? []).filter((p) => p.template_id === tpl.id);
    return prompts.length > 0 && prompts.every((p) => isAnswered(answers[p.id]));
  }

  const iconOf = (phase) => PHASES.find((p) => p.key === phase)?.icon ?? '';
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.mindset.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if day === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else}
  <ul class="phases">
    {#each templates as tpl (tpl.id)}
      <li class="phase">
        <span class="ic">{iconOf(tpl.phase)}</span>
        <span class="lbl">{tpl.name}</span>
        <span class="status" class:done={answered(tpl)}>
          {answered(tpl)
            ? $t('dashboard.widgets.mindset.done')
            : $t('dashboard.widgets.mindset.notFilled')}
        </span>
      </li>
    {/each}
    {#if templates.length === 0}
      <li class="hint">{$t('mindset.page.noTemplates')}</li>
    {/if}
  </ul>
{/if}

<style>
  .sk {
    padding: var(--space-1) 0;
  }
  /* Preview, loading and empty text — not an error. This was grouped with a
     now-removed .err rule and inherited its red. */
  .hint {
    color: var(--dim);
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
    color: var(--dim);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: 1px var(--space-2);
  }
  .status.done {
    color: var(--green);
    border-color: var(--green);
  }
</style>
