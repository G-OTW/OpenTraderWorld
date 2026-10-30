<script>
  // Chart settings quick menu: the switches a trader flips mid-session, one click each, and
  // a way into the full sheet (`onmore`) for colours and strokes. Edits the shared `settings`
  // object (bindable); the parent persists it server-side.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `at`: viewport position ({ top | bottom, right }) when the menu opens from inside a clipped box (a
  // chart pane), so it is drawn fixed rather than cut by the pane's edge.
  // `onapplyall`: copy this chart's style to every chart of the workspace.
  // `oninstrument`: opens the instrument facts scripts read (`syminfo.*`), when a chart is open.
  let { settings = $bindable(), onmore, onapplyall, oninstrument = null, at = null } = $props();

  const toggle = (k) => (settings[k] = !settings[k]);
</script>

{#snippet check(on)}
  <span class="chk">{#if on}<Icon name="check" size={14} />{/if}</span>
{/snippet}

<div
  class="menu"
  role="menu"
  style={at
    ? `position:fixed;${at.bottom != null ? `top:auto;bottom:${at.bottom}px` : `top:${at.top}px`};right:${at.right}px`
    : undefined}
>
  <button role="menuitemradio" aria-checked={settings.scale !== 'log'} onclick={() => (settings.scale = 'linear')}>
    {@render check(settings.scale !== 'log')}
    <span>{$t('histviz.settings.linear')}</span>
  </button>
  <button role="menuitemradio" aria-checked={settings.scale === 'log'} onclick={() => (settings.scale = 'log')}>
    {@render check(settings.scale === 'log')}
    <span>{$t('histviz.settings.logarithmic')}</span>
  </button>

  <div class="sep"></div>
  <button
    role="menuitem"
    onclick={() => (settings.yAxisSide = settings.yAxisSide === 'right' ? 'left' : 'right')}
  >
    <span class="chk"></span>
    <span>
      {settings.yAxisSide === 'right' ? $t('histviz.settings.moveLeft') : $t('histviz.settings.moveRight')}
    </span>
  </button>

  <div class="sep"></div>
  {#each [['grid', 'gridH'], ['gridV', 'gridV'], ['crosshair', 'crosshair'], ['tooltip', 'tooltip']] as [k, label] (k)}
    <button role="menuitemcheckbox" aria-checked={!!settings[k]} onclick={() => toggle(k)}>
      {@render check(!!settings[k])}
      <span>{$t(`histviz.settings.${label}`)}</span>
    </button>
  {/each}

  <div class="sep"></div>
  {#each [['lastPrice', 'lastPrice'], ['lastPriceLine', 'lastPriceLine'], ['countdown', 'countdown']] as [k, label] (k)}
    <button
      role="menuitemcheckbox"
      aria-checked={!!settings[k]}
      disabled={k !== 'lastPrice' && !settings.lastPrice}
      onclick={() => toggle(k)}
    >
      {@render check(!!settings[k])}
      <span>{$t(`histviz.settings.${label}`)}</span>
    </button>
  {/each}

  <div class="sep"></div>
  {#each [['daySeparators', 'daySeparators'], ['prevClose', 'prevClose'], ['bidAsk', 'bidAsk']] as [k, label] (k)}
    <button role="menuitemcheckbox" aria-checked={!!settings[k]} onclick={() => toggle(k)}>
      {@render check(!!settings[k])}
      <span>{$t(`histviz.settings.${label}`)}</span>
    </button>
  {/each}

  <div class="sep"></div>
  {#if onapplyall}
    <button role="menuitem" onclick={() => onapplyall()}>
      <span class="chk"><Icon name="copy" size={14} /></span>
      <span>{$t('histviz.settings.applyAll')}</span>
    </button>
  {/if}
  {#if oninstrument}
    <button role="menuitem" onclick={() => oninstrument()}>
      <span class="chk"><Icon name="info" size={14} /></span>
      <span>{$t('histviz.instrument.title')}</span>
    </button>
  {/if}
  <button role="menuitem" onclick={() => onmore?.()}>
    <span class="chk"><Icon name="settings" size={14} /></span>
    <span>{$t('histviz.settings.more')}</span>
  </button>
</div>

<style>
  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: var(--z-dropdown);
    width: 260px;
    max-height: min(78vh, 640px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    padding: var(--space-1) 0;
    box-shadow: var(--shadow-3);
  }
  button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-radius: 0;
    color: var(--text);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--surface-2);
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .chk {
    display: inline-flex;
    width: 16px;
    flex: none;
    color: var(--text);
  }
  .sep {
    margin: var(--space-1) 0;
    border-top: 1px solid var(--border);
  }
</style>
