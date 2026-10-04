<script>
  // The module's fixed top menu. Both pages open with it, so switching between the day
  // board and the template library is one click from anywhere in the module and the
  // current page is always legible.
  //
  // Links, not tabs: each page is a real route with its own URL, so it is bookmarkable
  // and the back button behaves. The `.tabs` look is borrowed for familiarity only.
  import { page } from '$app/stores';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // props: links ([{ href, icon, key }]), label (aria-label i18n key)
  let { links = [], label = 'routines.nav.label' } = $props();

  // Exact match only: a module root must not light up while a sub-page is open.
  const current = $derived($page.url.pathname.replace(/\/$/, '') || links[0]?.href);
</script>

<nav class="modnav" aria-label={$t(label)}>
  {#each links as l (l.href)}
    <a href={l.href} class:active={current === l.href} aria-current={current === l.href ? 'page' : undefined}>
      <Icon name={l.icon} size={14} />
      {$t(l.key)}
    </a>
  {/each}
</nav>

<style>
  .modnav {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    border-bottom: 0.5px solid var(--border);
    margin-bottom: var(--space-4);
  }

  a {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: var(--text-base);
    text-decoration: none;
    /* The underline is the selected state, so it has to exist unpainted on every tab —
       otherwise the row shifts by a pixel when one becomes active. */
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  a:hover {
    color: var(--text);
  }
  a.active {
    color: var(--text);
    font-weight: var(--fw-medium);
    border-bottom-color: var(--accent);
  }
  a:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    border-radius: var(--radius);
  }
</style>
