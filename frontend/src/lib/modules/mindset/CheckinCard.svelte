<script>
  import Icon from '$lib/ui/Icon.svelte';
  // One phase's check-in (pre-mortem or post-mortem): renders that phase's prompts as
  // controls — 1–5 segmented scale, single-choice pills, multi-tag chips, free text — over a
  // local copy of the answers map. Explicit Save (upsert); a dirty dot shows unsaved edits.
  import { mindsetApi } from './api.js';
  import { t } from '$lib/i18n';

  let { phase, date, prompts = [], entry = null, onsaved = () => {} } = $props();

  let answers = $state({});
  let dirty = $state(false);
  let saving = $state(false);
  let savedFlash = $state(false);
  let error = $state('');

  // Reset the local map whenever the (date, entry) context changes.
  $effect(() => {
    date;
    answers = { ...(entry?.answers ?? {}) };
    dirty = false;
    error = '';
  });

  let mine = $derived(prompts.filter((p) => p.phase === phase.key));
  let answeredCount = $derived(
    mine.filter((p) => {
      const v = answers[p.id];
      return v != null && v !== '' && !(Array.isArray(v) && v.length === 0);
    }).length
  );

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
      await mindsetApi.saveEntry(date, phase.key, answers);
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

<section class="card">
  <header>
    <span class="icon">{phase.icon}</span>
    <div class="ttl">
      <h2>{phase.label}</h2>
      <p class="hint">{phase.hint}</p>
    </div>
    <span class="count">{answeredCount}/{mine.length}</span>
  </header>

  <div class="prompts">
    {#each mine as p (p.id)}
      <div class="prompt">
        <span class="plabel">{p.label}</span>

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
                class:on={answers[p.id] === opt}
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
                class:on={Array.isArray(answers[p.id]) && answers[p.id].includes(opt)}
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
            oninput={(e) => set(p.id, e.target.value)}
          ></textarea>
        {/if}
      </div>
    {/each}
    {#if mine.length === 0}
      <p class="muted small">{$t('mindset.checkinCard.noPromptsHint')}</p>
    {/if}
  </div>

  {#if error}<p class="err">{error}</p>{/if}

  <footer>
    {#if dirty}<span class="dirty">{$t('mindset.checkinCard.unsaved')}</span>{/if}
    {#if savedFlash}<span class="saved"><Icon name="check" size={12} /> {$t('mindset.checkinCard.saved')}</span>{/if}
    <button class="btn primary" onclick={save} disabled={saving || mine.length === 0}>
      {saving ? $t('mindset.checkinCard.savingEllipsis') : entry ? $t('mindset.checkinCard.update') : $t('mindset.checkinCard.saveCheckin')}
    </button>
  </footer>
</section>

<style>
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }
  .icon {
    font-size: 1.3rem;
  }
  .ttl {
    flex: 1;
  }
  h2 {
    font-size: 1.05rem;
    font-weight: 700;
  }
  .hint {
    font-size: 0.78rem;
    color: var(--muted);
  }
  .count {
    font-size: 0.75rem;
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
  }
  .prompts {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .prompt {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .plabel {
    font-size: 0.84rem;
    color: var(--text);
    font-weight: 500;
  }
  .scale {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .end {
    font-size: 0.72rem;
    color: var(--muted);
    min-width: 0;
  }
  .steps {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .steps button {
    width: 38px;
    height: 30px;
    background: var(--surface-2);
    border: none;
    border-left: 1px solid var(--border);
    color: var(--muted);
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
  }
  .steps button:first-child {
    border-left: none;
  }
  .steps button.on {
    background: var(--accent);
    color: #fff;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, var(--surface-2));
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
  }
  .dirty {
    font-size: 0.75rem;
    color: var(--amber);
  }
  .saved {
    font-size: 0.75rem;
    color: var(--green);
  }
  .btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--accent);
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.82rem;
  }
  .err {
    color: var(--red);
    font-size: 0.85rem;
  }
</style>
