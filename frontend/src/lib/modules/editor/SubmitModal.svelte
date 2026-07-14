<script>
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { submitDoc } from './submit-api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // props:
  //   open (bindable) — modal visibility
  //   doc  — { title, icon, layout, html, source_json } snapshot to submit
  let { open = $bindable(false), doc = null } = $props();

  // Common UI languages (native names — proper nouns, not translated); the value is the
  // code sent to the pipeline.
  const LANGUAGES = [
    { code: 'en', label: 'English' },
    { code: 'fr', label: 'Français' },
    { code: 'es', label: 'Español' },
    { code: 'de', label: 'Deutsch' },
    { code: 'it', label: 'Italiano' },
    { code: 'pt', label: 'Português' }
  ];

  let language = $state('en');
  let categories = $state([]);
  let catDraft = $state('');
  let showAuthor = $state(false);
  let authorName = $state('');
  let authorEmail = $state('');
  let submitting = $state(false);
  let result = $state(null); // null | 'ok' | error string

  // The submission date is stamped server-side; we show today for the user's reference only.
  const today = new Date().toLocaleDateString(undefined, { dateStyle: 'long' });

  // Reset transient state each time the modal opens.
  $effect(() => {
    if (open) {
      language = 'en';
      categories = [];
      catDraft = '';
      showAuthor = false;
      authorName = '';
      authorEmail = '';
      submitting = false;
      result = null;
    }
  });

  function addCategory() {
    const c = catDraft.trim();
    if (c && !categories.includes(c)) categories = [...categories, c];
    catDraft = '';
  }

  function onCatKeydown(e) {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      addCategory();
    }
  }

  function removeCategory(c) {
    categories = categories.filter((x) => x !== c);
  }

  async function submit() {
    if (!doc || submitting) return;
    submitting = true;
    result = null;
    try {
      await submitDoc({
        title: doc.title,
        icon: doc.icon ?? null,
        layout: doc.layout ?? 'normal',
        html: doc.html,
        source_json: doc.source_json,
        language,
        categories,
        author_name: showAuthor && authorName.trim() ? authorName.trim() : null,
        author_email: showAuthor && authorEmail.trim() ? authorEmail.trim() : null
      });
      result = 'ok';
    } catch (e) {
      result = e?.message ?? $t('editor.submitModal.submissionFailed');
    } finally {
      submitting = false;
    }
  }
</script>

<Modal bind:open title={$t('editor.submitModal.title')} size="md">
  {#if result === 'ok'}
    <div class="done">
      <span class="done-ico"><Icon name="check" size={20} /></span>
      <p>{$t('editor.submitModal.submittedBody')}</p>
    </div>
  {:else}
    <div class="form">
      <p class="lead">{$t('editor.submitModal.lead')}</p>

      <label class="field">
        <span class="lbl">{$t('editor.submitModal.language')}</span>
        <select bind:value={language}>
          {#each LANGUAGES as l (l.code)}
            <option value={l.code}>{l.label}</option>
          {/each}
        </select>
      </label>

      <div class="field">
        <span class="lbl">{$t('editor.submitModal.categories')}</span>
        <div class="cat-input">
          <input
            type="text"
            bind:value={catDraft}
            onkeydown={onCatKeydown}
            placeholder={$t('editor.submitModal.categoryPlaceholder')}
          />
          <button type="button" class="add" onclick={addCategory} disabled={!catDraft.trim()}>
            {$t('editor.submitModal.add')}
          </button>
        </div>
        {#if categories.length}
          <div class="chips">
            {#each categories as c (c)}
              <span class="chip">
                {c}
                <button type="button" onclick={() => removeCategory(c)} aria-label={$t('editor.submitModal.removeCategory', { name: c })}>
                  <Icon name="x" size={12} />
                </button>
              </span>
            {/each}
          </div>
        {/if}
      </div>

      <div class="field">
        <span class="lbl">{$t('editor.submitModal.date')}</span>
        <span class="date">{today}</span>
      </div>

      {#if showAuthor}
        <label class="field">
          <span class="lbl">{$t('editor.submitModal.name')} <em>{$t('editor.submitModal.optional')}</em></span>
          <input type="text" bind:value={authorName} placeholder={$t('editor.submitModal.yourName')} />
        </label>
        <label class="field">
          <span class="lbl">{$t('editor.submitModal.email')} <em>{$t('editor.submitModal.optional')}</em></span>
          <input type="email" bind:value={authorEmail} placeholder="you@example.com" />
        </label>
      {:else}
        <button type="button" class="link" onclick={() => (showAuthor = true)}>
          {$t('editor.submitModal.addNameEmail')}
        </button>
      {/if}

      {#if result && result !== 'ok'}
        <ErrorText error={result} />
      {/if}
    </div>
  {/if}

  {#snippet footer()}
    {#if result === 'ok'}
      <button class="primary" onclick={() => (open = false)}>{$t('editor.submitModal.close')}</button>
    {:else}
      <button class="ghost" onclick={() => (open = false)} disabled={submitting}>{$t('common.cancel')}</button>
      <button class="primary" onclick={submit} disabled={submitting}>
        {submitting ? $t('editor.submitModal.submitting') : $t('editor.submitModal.submit')}
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .lead {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lbl {
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    color: var(--text);
  }
  .lbl em {
    font-weight: var(--fw-normal);
    color: var(--muted);
    font-style: normal;
  }
  select,
  input {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 8px;
    font-size: var(--text-base);
    width: 100%;
  }
  .cat-input {
    display: flex;
    gap: var(--space-2);
  }
  .cat-input input {
    flex: 1;
  }
  .add {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 0 12px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .add:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-top: var(--space-1);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px 4px 2px 8px;
    font-size: var(--text-sm);
    color: var(--text);
  }
  .chip button {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    display: inline-flex;
    padding: 2px;
    border-radius: 4px;
  }
  .chip button:hover {
    color: var(--text);
    background: var(--surface);
  }
  .date {
    font-size: var(--text-base);
    color: var(--muted);
  }
  .link {
    align-self: flex-start;
    background: transparent;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
  }
  .done {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .done-ico {
    color: var(--green);
    flex-shrink: 0;
  }
  .done p {
    margin: 0;
    font-size: var(--text-base);
    color: var(--text);
  }
  /* .primary / .ghost inherit the app-wide button styling from components.css. */
</style>
