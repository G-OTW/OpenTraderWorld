<script>
  // One template's check-in: renders its prompts as controls — 1–5 segmented scale,
  // single-choice pills, multi-tag chips, free text — over a local copy of the answers map.
  // Explicit Save (upsert); a dirty dot shows unsaved edits.
  //
  // The card folds: a day can carry several check-ins now, so a finished one gets out of
  // the way instead of pushing the next one off screen.
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { mindsetApi, isAnswered } from './api.js';
  import { t } from '$lib/i18n';

  let {
    template,
    date,
    prompts = [],
    entry = null,
    accent = 'var(--border)',
    open = true,
    ontoggle = () => {},
    onsaved = () => {}
  } = $props();

  let answers = $state({});
  let dirty = $state(false);
  let saving = $state(false);
  let savedFlash = $state(false);
  let error = $state('');
  let showNotes = $state(false);

  // Reset the local map whenever the (date, entry) context changes.
  $effect(() => {
    date;
    answers = { ...(entry?.answers ?? {}) };
    dirty = false;
    error = '';
  });

  const answeredCount = $derived(prompts.filter((p) => isAnswered(answers[p.id])).length);
  const complete = $derived(prompts.length > 0 && answeredCount === prompts.length);

  function set(promptId, value) {
    answers = { ...answers, [promptId]: value };
    dirty = true;
  }
  function toggleTag(promptId, tag) {
    const cur = Array.isArray(answers[promptId]) ? answers[promptId] : [];
    set(promptId, cur.includes(tag) ? cur.filter((t) => t !== tag) : [...cur, tag]);
  }

  async function save() {
    saving = true;
    error = '';
    try {
      await mindsetApi.saveEntry(date, template.id, answers);
      dirty = false;
      savedFlash = true;
      setTimeout(() => (savedFlash = false), 1500);
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<section class="card" class:complete style:--cat={accent}>
  <header>
    <button class="head" onclick={ontoggle} aria-expanded={open} aria-controls="ci-{template.id}">
      <Icon name={open ? 'chevron-down' : 'chevron-right'} size={13} />
      <span class="ttl">
        <h2>{template.name}</h2>
        {#if template.description}<p class="hint">{template.description}</p>{/if}
      </span>
    </button>

    {#if template.notes}
      <button
        class="iconbtn"
        class:on={showNotes}
        onclick={() => (showNotes = !showNotes)}
        title={$t('mindset.card.notes')}
        aria-label={$t('mindset.card.notes')}
      >
        <Icon name="file-text" size={13} />
      </button>
    {/if}
    <span class="count" class:done={complete}>{answeredCount}/{prompts.length}</span>
  </header>

  {#if showNotes && template.notes}
    <!-- Sanitised server-side at write time (ammonia allowlist). -->
    <div class="notes">{@html template.notes}</div>
  {/if}

  {#if open}
    <div class="prompts" id="ci-{template.id}">
      {#if prompts.length === 0}
        <p class="empty-hint">{$t('mindset.checkinCard.noPromptsHint')}</p>
      {/if}

      {#each prompts as p (p.id)}
        <div class="prompt">
          <span class="plabel">{p.label}</span>
          {#if p.hint}<span class="phint">{p.hint}</span>{/if}

          {#if p.kind === 'scale'}
            <div class="scale">
              {#if p.config?.low}<span class="end">{p.config.low}</span>{/if}
              <div class="steps">
                {#each [1, 2, 3, 4, 5] as n (n)}
                  <button
                    type="button"
                    class:on={answers[p.id] === n}
                    onclick={() => set(p.id, answers[p.id] === n ? null : n)}
                  >
                    {n}
                  </button>
                {/each}
              </div>
              {#if p.config?.high}<span class="end">{p.config.high}</span>{/if}
            </div>
          {:else if p.kind === 'choice'}
            <div class="chips">
              {#each p.config?.options ?? [] as opt (opt)}
                <button
                  type="button"
                  class="chip"
                  class:active={answers[p.id] === opt}
                  onclick={() => set(p.id, answers[p.id] === opt ? null : opt)}
                >
                  {opt}
                </button>
              {/each}
            </div>
          {:else if p.kind === 'tags'}
            <div class="chips">
              {#each p.config?.options ?? [] as opt (opt)}
                <button
                  type="button"
                  class="chip"
                  class:active={Array.isArray(answers[p.id]) && answers[p.id].includes(opt)}
                  onclick={() => toggleTag(p.id, opt)}
                >
                  {opt}
                </button>
              {/each}
            </div>
          {:else}
            <textarea
              rows="2"
              placeholder={$t('mindset.checkinCard.writeItDown')}
              value={answers[p.id] ?? ''}
              oninput={(e) => set(p.id, e.currentTarget.value)}
            ></textarea>
          {/if}
        </div>
      {/each}

      <ErrorText error={error} />

      <div class="foot">
        {#if dirty}
          <span class="dot" aria-hidden="true"></span>
          <span class="state">{$t('mindset.checkinCard.unsaved')}</span>
        {:else if savedFlash}
          <span class="state ok">{$t('mindset.checkinCard.saved')}</span>
        {/if}
        <div class="spacer"></div>
        <button class="btn primary" onclick={save} disabled={saving || prompts.length === 0}>
          {saving
            ? $t('mindset.checkinCard.savingEllipsis')
            : entry
              ? $t('mindset.checkinCard.update')
              : $t('mindset.checkinCard.saveCheckin')}
        </button>
      </div>
    </div>
  {/if}
</section>

<style>
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    /* The category colour is the card's left edge, matching the routines board. */
    border-left: 2px solid var(--cat);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .card.complete {
    border-color: var(--green);
    border-left-color: var(--green);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .head {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .ttl {
    min-width: 0;
  }
  h2 {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .iconbtn {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius);
    display: inline-flex;
    flex: none;
  }
  .iconbtn:hover,
  .iconbtn.on {
    color: var(--text);
    background: var(--surface-2);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    flex: none;
  }
  .count.done {
    color: var(--green);
  }

  .notes {
    font-size: var(--text-sm);
    color: var(--muted);
    padding-left: var(--space-3);
    border-left: 2px solid var(--border);
  }
  .notes :global(p) {
    margin: 0 0 var(--space-1);
  }
  .notes :global(ul),
  .notes :global(ol) {
    margin: 0;
    padding-left: var(--space-4);
  }
  .notes :global(a) {
    color: var(--accent);
  }

  .prompts {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .empty-hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .prompt {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .plabel {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .phint {
    font-size: var(--text-xs);
    color: var(--muted);
    margin-top: -4px;
  }

  .scale {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .scale .end {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .steps {
    display: flex;
    gap: var(--space-1);
  }
  .steps button {
    width: 38px;
    height: 30px;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .steps button.on {
    color: var(--text);
    background: var(--surface);
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  textarea {
    width: 100%;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-top: var(--space-1);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
  }
  .state {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .state.ok {
    color: var(--green);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
