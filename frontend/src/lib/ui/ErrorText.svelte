<script>
  // The one way an error is shown inline. Before this, 60 components re-declared their own
  // `.err` rule and only two of them carried role="alert" — the rest appeared on screen and
  // said nothing to a screen reader.
  //
  //   <ErrorText {error} />                 <!-- plain -->
  //   <ErrorText {error} copyable />        <!-- adds a "copy" control for long stack text -->
  //
  // Renders nothing when `error` is falsy, so callers need no {#if} wrapper.
  //
  // This is the BLOCK error: a whole fetch or action failed. A FIELD error is a different,
  // quieter thing — no icon, --text-xs, sitting under its input. It also cannot use this
  // component, because a <label> may not contain a <p>. Write it inline instead:
  //
  //   <input class:invalid={e} aria-invalid={e ? 'true' : undefined}
  //          aria-describedby={e ? 'x-err' : undefined} oninput={() => (e = '')} />
  //   {#if e}<span id="x-err" class="err" role="alert">{e}</span>{/if}
  //
  // On `copyable`: the copy affordance is a nested <button>, NOT `use:copyLog` on the
  // container. That action sets role="button" when the element has no role, which silently
  // overrode the alert — an error message was announced as "button". A single element cannot
  // be both, so the roles are split across two: the container alerts, the button copies.
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  let { error = '', copyable = false, compact = false } = $props();

  let copied = $state(false);
  let timer;

  async function copy() {
    const payload = String(error ?? '').trim();
    if (!payload) return;
    let ok = false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(payload);
        ok = true;
      }
    } catch {
      /* fall through to the legacy path */
    }
    if (!ok) {
      // navigator.clipboard is undefined outside a secure context (plain-HTTP LAN access).
      try {
        const ta = document.createElement('textarea');
        ta.value = payload;
        ta.setAttribute('readonly', '');
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        ok = document.execCommand('copy');
        ta.remove();
      } catch {
        /* clipboard unavailable — leave the message on screen to select by hand */
      }
    }
    if (ok) {
      copied = true;
      clearTimeout(timer);
      timer = setTimeout(() => (copied = false), 1200);
    }
  }

  // The flash clears itself; cancel it so it cannot fire on a destroyed component.
  $effect(() => () => clearTimeout(timer));
</script>

{#if error}
  <!-- role="alert" carries an implicit aria-live="assertive": an error interrupts, where a
       status waits its turn. It belongs on the container, never on the copy button. -->
  <p class="ui-error" class:compact role="alert">
    <span class="ico" aria-hidden="true"><Icon name="alert-triangle" size={compact ? 12 : 13} /></span>
    <span class="msg">{error}</span>
    {#if copyable}
      <button
        class="copy"
        type="button"
        onclick={copy}
        title={$t('common.clickToCopy')}
        aria-label={$t('common.copyError')}
      >
        <Icon name={copied ? 'check' : 'copy'} size={12} />
      </button>
    {/if}
  </p>
{/if}

<style>
  /* --red-ink, not --red. The raw hue is a fill colour: as small text it measures 4.83:1 on
     --surface (passes) but only 4.35:1 on --surface-2 (fails), and errors appear on both.
     --red-ink clears 4.5:1 against either, in both themes. */
  .ui-error {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    color: var(--red-ink);
    font-size: var(--text-base);
    line-height: var(--lh-base);
  }
  .ui-error.compact {
    font-size: var(--text-sm);
  }

  .ico {
    display: inline-flex;
    flex: none;
    /* Nudge the icon onto the first line's baseline rather than the box's top. */
    margin-top: 0.15em;
  }

  .msg {
    flex: 1;
    min-width: 0;
    /* A stack trace must wrap rather than stretch the layout. */
    overflow-wrap: anywhere;
  }

  .copy {
    flex: none;
    display: inline-flex;
    align-items: center;
    background: none;
    border: none;
    padding: 2px;
    color: currentColor;
    opacity: 0.7;
    cursor: pointer;
    border-radius: var(--radius);
  }
  .copy:hover {
    opacity: 1;
  }
</style>
