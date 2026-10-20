<script>
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { resourcesApi, thumbFallback } from './api.js';
  import { uploadFile, pickFile } from '$lib/modules/editor/files-api.js';
  // Add/edit a resource: name, link, description, thumbnail, and its category.
  let {
    initial = null,
    categories = [],
    defaultCategoryId = null,
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  let r = $state(blank());
  // Thumbnail work is asynchronous and can fail on the remote side; both states show on
  // the tile itself rather than as a toast, because that is what they describe.
  let busy = $state(''); // '' | 'auto' | 'upload'
  let thumbError = $state('');

  function blank() {
    const base = {
      category_id: defaultCategoryId ?? categories[0]?.id ?? '',
      name: '',
      link: '',
      description: '',
      thumb_url: ''
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        link: initial.link ?? '',
        description: initial.description ?? '',
        thumb_url: initial.thumb_url ?? ''
      };
    }
    return base;
  }

  const fallback = $derived(thumbFallback(r.name));

  async function autoThumb() {
    if (!r.link.trim() || busy) return;
    busy = 'auto';
    thumbError = '';
    try {
      const url = await resourcesApi.preview(r.link.trim());
      if (url) r.thumb_url = url;
      else thumbError = $t('resources.form.thumbNotFound');
    } catch (e) {
      thumbError = e?.message || $t('resources.form.thumbFailed');
    } finally {
      busy = '';
    }
  }

  async function uploadThumb() {
    if (busy) return;
    const file = await pickFile('image/*');
    if (!file) return;
    busy = 'upload';
    thumbError = '';
    try {
      const meta = await uploadFile(file);
      r.thumb_url = meta.url;
    } catch (e) {
      thumbError = e?.message || $t('resources.form.thumbFailed');
    } finally {
      busy = '';
    }
  }

  function submit() {
    onsubmit({
      category_id: r.category_id,
      name: r.name,
      link: r.link,
      description: r.description,
      thumb_url: r.thumb_url
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

  <div class="field">
    <span>{$t('resources.form.category')}</span>
    <Dropdown
      bind:value={r.category_id}
      ariaLabel={$t('resources.form.category')}
      options={categories.map((c) => ({ value: c.id, label: c.name }))}
    />
  </div>

  <label class="field">
    <span>{$t('resources.form.link')}</span>
    <input bind:value={r.link} placeholder={$t('resources.form.linkPlaceholder')} />
  </label>

  <div class="field">
    <span>{$t('resources.form.thumbnail')}</span>
    <div class="thumb-row">
      <div class="tile" class:busy={!!busy}>
        {#if r.thumb_url}
          <img src={r.thumb_url} alt="" />
        {:else}
          <span class="ph" style:--h={fallback.hue}>{fallback.initials}</span>
        {/if}
      </div>
      <div class="thumb-side">
        <div class="thumb-actions">
          <button
            type="button"
            class="ghost"
            onclick={autoThumb}
            disabled={!r.link.trim() || !!busy}
            title={r.link.trim() ? '' : $t('resources.form.thumbNoLink')}
          >
            <Icon name="download" size={13} />
            {busy === 'auto' ? $t('resources.form.thumbFetching') : $t('resources.form.thumbAuto')}
          </button>
          <button type="button" class="ghost" onclick={uploadThumb} disabled={!!busy}>
            {$t('resources.form.thumbUpload')}
          </button>
          {#if r.thumb_url}
            <button type="button" class="ghost" onclick={() => ((r.thumb_url = ''), (thumbError = ''))}>
              {$t('common.remove')}
            </button>
          {/if}
        </div>
        <input
          class="thumb-url"
          bind:value={r.thumb_url}
          placeholder={$t('resources.form.thumbUrlPlaceholder')}
        />
        {#if thumbError}<span class="err">{thumbError}</span>{/if}
      </div>
    </div>
  </div>

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
  .thumb-row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }
  /* Same 16:9 crop as the gallery tile, so what you see here is what the card shows. */
  .tile {
    flex: none;
    width: 128px;
    aspect-ratio: 16 / 9;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    overflow: hidden;
    display: grid;
    place-items: center;
  }
  .tile.busy {
    opacity: 0.5;
  }
  .tile img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .ph {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    color: hsl(var(--h) 50% 55%);
    background: hsl(var(--h) 45% 50% / 0.15);
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
  }
  .thumb-side {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .thumb-actions {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .thumb-actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .thumb-url {
    width: 100%;
    font-size: var(--text-sm);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
  }
</style>
