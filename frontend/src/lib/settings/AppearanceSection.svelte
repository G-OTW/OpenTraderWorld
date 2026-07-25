<script>
  // Appearance — the app's primary color (accent). A free color picker plus preset
  // shortcuts; the choice applies live app-wide (accent store stamps --accent on <html>)
  // and persists to the backend. See $lib/theme/accent.svelte.js.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { accent } from '$lib/theme/accent.svelte.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // The live value (null = theme default). Bound to the picker via a concrete hex.
  let saving = $state(false);
  let ok = $state('');
  let error = $state('');

  // The <input type=color> needs a concrete hex; when on the default, show the resolved
  // theme accent so the swatch isn't blank.
  let picked = $state('#c9a45c');

  onMount(() => {
    if (accent.value) picked = accent.value;
    else {
      const c = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim();
      if (c) picked = c;
    }
  });

  async function persist(hex) {
    ok = '';
    error = '';
    saving = true;
    try {
      // Empty string clears back to the theme default on the backend.
      await settingsApi.setDefaults({ accent: hex ?? '' });
      ok = $t('settings.appearance.saved');
    } catch (e) {
      error = e.message;
      accent.set(accent.value); // keep DOM consistent with last committed value on failure
    } finally {
      saving = false;
    }
  }

  function choose(hex) {
    accent.set(hex); // applies live immediately
    if (hex) picked = hex;
    persist(hex);
  }

  function onPick(e) {
    picked = e.currentTarget.value;
    choose(picked);
  }

  const isDefault = $derived(accent.value == null);
</script>

<div class="section">
  <h2>{$t('settings.appearance.title')}</h2>
  <p class="muted small">{$t('settings.appearance.subtitle')}</p>

  <div class="block">
    <span class="label">{$t('settings.appearance.presets')}</span>
    <div class="swatches">
      {#each accent.presets as p (p.name)}
        {@const active = p.hex == null ? isDefault : accent.value === p.hex}
        <button
          class="swatch"
          class:active
          style:--sw={p.hex ?? 'var(--accent)'}
          title={p.hex == null ? $t('settings.appearance.default') : p.name}
          aria-label={p.name}
          onclick={() => choose(p.hex)}
        ></button>
      {/each}
    </div>
  </div>

  <div class="block">
    <span class="label">{$t('settings.appearance.custom')}</span>
    <div class="picker">
      <input type="color" value={picked} oninput={onPick} aria-label={$t('settings.appearance.accent')} />
      <code class="hex">{picked}</code>
      {#if !isDefault}
        <button class="link" onclick={() => choose(null)}>{$t('settings.appearance.reset')}</button>
      {/if}
    </div>
  </div>

  <ErrorText {error} />
  {#if ok && !saving}<p class="ok">{ok}</p>{/if}
</div>

<style>
  .section {
    max-width: 480px;
  }
  h2 {
    margin: 0 0 var(--space-1);
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  .block {
    margin-top: var(--space-6);
  }
  .label {
    display: block;
    font-size: var(--text-sm);
    color: var(--text);
    margin-bottom: var(--space-2);
  }
  .swatches {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .swatch {
    width: 28px;
    height: 28px;
    border-radius: 0;
    background: var(--sw);
    border: var(--hairline) solid var(--border-control);
    cursor: pointer;
    padding: 0;
    outline-offset: 3px;
  }
  .swatch.active {
    border: 1.5px solid var(--text);
    outline: 1.5px solid var(--sw);
    outline-offset: 2px;
  }
  .picker {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .picker input[type='color'] {
    width: 40px;
    height: 32px;
    padding: 0;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    background: var(--surface);
    cursor: pointer;
  }
  .hex {
    font-size: var(--text-sm);
    font-family: var(--mono);
    color: var(--dim);
    text-transform: uppercase;
  }
  .link {
    background: none;
    border: none;
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
    text-decoration: none;
  }
  .link:hover {
    text-decoration: underline;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .ok {
    color: var(--green-ink);
    font-size: var(--text-base);
    margin: var(--space-3) 0 0;
  }
</style>
