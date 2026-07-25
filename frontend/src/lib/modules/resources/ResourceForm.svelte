<script>
  import { t } from '$lib/i18n';
  // Add/edit a resource: name, link, description, and its category.
  let {
    initial = null,
    categories = [],
    defaultCategoryId = null,
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  let r = $state(blank());

  function blank() {
    const base = {
      category_id: defaultCategoryId ?? categories[0]?.id ?? '',
      name: '',
      link: '',
      description: ''
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        link: initial.link ?? '',
        description: initial.description ?? ''
      };
    }
    return base;
  }

  function submit() {
    onsubmit({
      category_id: r.category_id,
      name: r.name,
      link: r.link,
      description: r.description
    });
  }
</script>

<form
  class="res-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <label class="field">
    <span>{$t('resources.form.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={r.name} autofocus placeholder={$t('resources.form.namePlaceholder')} />
  </label>

  <label class="field">
    <span>{$t('resources.form.category')}</span>
    <select bind:value={r.category_id}>
      {#each categories as c}<option value={c.id}>{c.name}</option>{/each}
    </select>
  </label>

  <label class="field">
    <span>{$t('resources.form.link')}</span>
    <input bind:value={r.link} placeholder={$t('resources.form.linkPlaceholder')} />
  </label>

  <label class="field">
    <span>{$t('resources.form.description')}</span>
    <textarea bind:value={r.description} rows="3" placeholder={$t('resources.form.descriptionPlaceholder')}></textarea>
  </label>

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('resources.form.addResource')}</button>
  </div>
</form>

<style>
  .res-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
