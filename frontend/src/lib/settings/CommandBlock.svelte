<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // A copy-to-clipboard command block for operator/host commands.
  let { command } = $props();
  let copied = $state(false);

  async function copy() {
    let ok = false;
    // navigator.clipboard is undefined outside secure contexts (plain-HTTP LAN
    // access over an IP), so fall back to a hidden-textarea execCommand copy.
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(command);
        ok = true;
      }
    } catch {
      /* fall through to legacy path */
    }
    if (!ok) {
      try {
        const ta = document.createElement('textarea');
        ta.value = command;
        ta.setAttribute('readonly', '');
        ta.style.position = 'fixed';
        ta.style.top = '-1000px';
        document.body.appendChild(ta);
        ta.select();
        ok = document.execCommand('copy');
        document.body.removeChild(ta);
      } catch {
        /* clipboard unavailable */
      }
    }
    if (ok) {
      copied = true;
      setTimeout(() => (copied = false), 1500);
    }
  }
</script>

<div class="block">
  <pre>{command}</pre>
  <button class="copy" onclick={copy}>
    <Icon name={copied ? 'check' : 'copy'} size={12} />
    {copied ? $t('common.copied') : $t('common.copy')}
  </button>
</div>

<style>
  .block {
    position: relative;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    margin: var(--space-2) 0;
  }
  pre {
    margin: 0;
    padding: 12px 70px 12px 12px;
    overflow-x: auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.8rem;
    color: var(--text);
    white-space: pre;
  }
  .copy {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    position: absolute;
    top: 8px;
    right: 8px;
    z-index: 1;
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: 0.74rem;
    cursor: pointer;
  }
  .copy:hover {
    color: var(--text);
  }
</style>
