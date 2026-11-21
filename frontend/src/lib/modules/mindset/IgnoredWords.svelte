<script>
  // The word cloud's ignored-words list: trading vocabulary that says nothing about the
  // state of mind. A user pref (`mindset.cloudIgnore`), unset = DEFAULT_IGNORED. Variants
  // of a listed word (plural, participle) are ignored with it.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import TagInput from '$lib/ui/TagInput.svelte';
  import { pref } from '$lib/prefs.svelte.js';
  import { t } from '$lib/i18n';
  import { DEFAULT_IGNORED, IGNORED_PREF } from './text.js';

  const ignored = pref(IGNORED_PREF, null);

  let open = $state(false);
  let draft = $state([]);
  let input = $state();

  function edit() {
    draft = [...(ignored.value ?? DEFAULT_IGNORED)];
    open = true;
  }

  function save() {
    input?.flush();
    const words = [...new Set(draft.map((w) => w.trim().toLowerCase()).filter(Boolean))];
    const same = words.length === DEFAULT_IGNORED.length && words.every((w) => DEFAULT_IGNORED.includes(w));
    ignored.value = same ? null : words;
    open = false;
  }
</script>

<Button variant="ghost" size="sm" icon="filter" onclick={edit}>{$t('mindset.history.ignoredBtn')}</Button>

<Modal bind:open title={$t('mindset.history.ignoredTitle')} size="md">
  <p class="hint">{$t('mindset.history.ignoredHint')}</p>
  <TagInput bind:this={input} bind:tags={draft} listId="mindset-ignored" placeholder={$t('mindset.history.ignoredPlaceholder')} />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (draft = [...DEFAULT_IGNORED])}>{$t('mindset.history.ignoredReset')}</Button>
    <Button onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={save}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .hint {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>
