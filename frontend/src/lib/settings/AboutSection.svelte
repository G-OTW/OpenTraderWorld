<script>
  // About: project links + a share call-to-action. URLs are fixed at build time — the
  // project has one canonical website/repo, so no server round-trip is needed here
  // (only the version comes from the API).
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  const WEBSITE = 'https://opentraderworld.com';
  const GITHUB = 'https://github.com/G-OTW/OpenTraderWorld';
  const DOCS_URL = 'https://g-otw.github.io/OpenTraderWorld/';

  // Links to the canonical site carry a `ref` so the site can count inbound clicks
  // coming from a self-hosted app (aggregate only, no user data). Kept only on the
  // opentraderworld.com URLs — GitHub/Pages are third-party and not measured here.
  const WEBSITE_REF = `${WEBSITE}/?ref=app-website`;
  const COMMUNITY_DOCS_URL = `${WEBSITE}/docs?ref=app-docs`;

  const links = [
    { icon: 'globe', labelKey: 'settings.about.website', url: WEBSITE_REF },
    { icon: 'github', labelKey: 'settings.about.github', url: GITHUB },
    { icon: 'bug', labelKey: 'settings.about.issues', url: `${GITHUB}/issues` },
    { icon: 'book', labelKey: 'settings.about.docs', url: DOCS_URL },
    { icon: 'book-open', labelKey: 'settings.about.communityDocs', url: COMMUNITY_DOCS_URL }
  ];

  let version = $state('');
  onMount(async () => {
    try {
      version = await settingsApi.version();
    } catch {
      /* version line just stays empty */
    }
  });

  // Share targets. `navigator.share` is the native sheet (mobile/macOS); the explicit
  // buttons cover desktop browsers without it.
  const canNativeShare = typeof navigator !== 'undefined' && !!navigator.share;
  let copied = $state(false);
  let copyTimer;

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(WEBSITE);
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 2000);
    } catch {
      /* clipboard denied — nothing to show */
    }
  }

  async function nativeShare(text) {
    try {
      await navigator.share({ title: 'OpenTraderWorld', text, url: WEBSITE });
    } catch {
      /* user dismissed the sheet */
    }
  }
</script>

<div class="section">
  <h2>{$t('settings.about.title')}</h2>
  <p class="version">
    OpenTraderWorld
    {#if version}<strong>v{version}</strong>{/if}
  </p>
  <p class="muted">{$t('settings.about.subtitle')}</p>

  <ul class="links">
    {#each links as l (l.labelKey)}
      {#if l.url}
        <li>
          <a href={l.url} target="_blank" rel="noopener noreferrer">
            <span class="ico"><Icon name={l.icon} size={16} /></span>
            <span class="label">{$t(l.labelKey)}</span>
            <span class="url">{l.url.replace('https://', '').replace(/\?ref=[^&]*/, '')}</span>
            <span class="ext"><Icon name="external-link" size={13} /></span>
          </a>
        </li>
      {:else}
        <li class="soon-row">
          <span class="ico"><Icon name={l.icon} size={16} /></span>
          <span class="label">{$t(l.labelKey)}</span>
          <span class="soon">{$t('settings.about.comingSoon')}</span>
        </li>
      {/if}
    {/each}
  </ul>

  <div class="share">
    <div class="share-head">
      <Icon name="share-2" size={16} />
      <strong>{$t('settings.about.shareTitle')}</strong>
    </div>
    <p class="muted small">{$t('settings.about.shareBody')}</p>
    <div class="share-actions">
      <button class="btn" onclick={copyLink}>
        <Icon name={copied ? 'check' : 'copy'} size={14} />
        {copied ? $t('settings.about.shareCopied') : $t('settings.about.shareCopy')}
      </button>
      <a
        class="btn"
        href={`mailto:?subject=${encodeURIComponent($t('settings.about.shareSubject'))}&body=${encodeURIComponent(`${$t('settings.about.shareText')}\n\n${WEBSITE}`)}`}
      >
        <Icon name="mail" size={14} />
        {$t('settings.about.shareEmail')}
      </a>
      <a
        class="btn"
        href={`https://x.com/intent/post?text=${encodeURIComponent($t('settings.about.shareText'))}&url=${encodeURIComponent(WEBSITE)}`}
        target="_blank"
        rel="noopener noreferrer"
      >
        <span class="x-glyph">𝕏</span>
        {$t('settings.about.shareX')}
      </a>
      {#if canNativeShare}
        <button class="btn" onclick={() => nativeShare($t('settings.about.shareText'))}>
          <Icon name="share-2" size={14} />
          {$t('settings.about.shareMore')}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .section {
    max-width: 680px;
  }
  h2 {
    margin: 0 0 var(--space-2);
    font-size: 1.1rem;
    color: var(--text);
  }
  .version {
    color: var(--text);
    font-size: 0.9rem;
    margin: 0 0 var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: 0.86rem;
    line-height: 1.5;
  }
  .small {
    font-size: 0.82rem;
  }

  .links {
    list-style: none;
    margin: var(--space-4) 0 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    overflow: hidden;
  }
  .links li + li {
    border-top: 1px solid var(--border);
  }
  .links a,
  .soon-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 10px var(--space-3);
    font-size: 0.88rem;
    color: var(--text);
    text-decoration: none;
  }
  .links a:hover {
    background: var(--surface-2);
  }
  .ico {
    display: inline-flex;
    color: var(--muted);
    flex-shrink: 0;
  }
  .label {
    font-weight: 500;
    white-space: nowrap;
  }
  .url {
    color: var(--muted);
    font-size: 0.8rem;
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ext {
    display: inline-flex;
    color: var(--muted);
    flex-shrink: 0;
  }
  .soon-row {
    color: var(--muted);
  }
  .soon {
    margin-left: auto;
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 8px;
  }

  .share {
    margin-top: var(--space-4);
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius);
    padding: var(--space-3);
  }
  .share-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
    font-size: 0.9rem;
  }
  .share .muted {
    margin: var(--space-1) 0 var(--space-3);
  }
  .share-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 0.82rem;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    text-decoration: none;
  }
  .btn:hover {
    border-color: var(--accent);
  }
  .x-glyph {
    font-size: 0.85rem;
    line-height: 1;
  }
</style>
