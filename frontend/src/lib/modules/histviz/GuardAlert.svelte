<script>
  // The guards waiting for an acknowledgement: one window over everything, nothing blocked
  // behind it (no backdrop), dragged anywhere on the screen by its head. It stays until each
  // firing is acknowledged.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { t } from '$lib/i18n';
  import { ruleText } from './guards.js';

  let {
    /** Firings to acknowledge, newest first. */
    events = [],
    guards = [],
    nameOf = (id) => id,
    onack
  } = $props();

  let pos = $state(null);
  let drag = null;
  let box = $state(null);

  function down(e) {
    if (e.button !== 0 || !box) return;
    const b = box.getBoundingClientRect();
    drag = { dx: e.clientX - b.left, dy: e.clientY - b.top, w: b.width, h: b.height };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function move(e) {
    if (!drag) return;
    const x = Math.max(0, Math.min(e.clientX - drag.dx, innerWidth - drag.w));
    const y = Math.max(0, Math.min(e.clientY - drag.dy, innerHeight - drag.h));
    pos = { x, y };
  }
  function up() {
    drag = null;
  }

  const ruleOf = (ev) => ruleText(guards.find((g) => g.id === ev.guard_id)?.rule, $t);
  const when = (iso) => new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
</script>

{#if events.length}
  <div class="gwin" bind:this={box} role="alertdialog" aria-label={$t('histviz.guards.ackTitle')} style={pos ? `left:${pos.x}px;top:${pos.y}px;right:auto` : ''}>
    <div class="ghead" role="presentation" onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up}>
      <Icon name="shield" size={14} />
      <span>{$t('histviz.guards.ackTitle')}</span>
    </div>
    <ul>
      {#each events as ev (ev.id)}
        <li>
          <span class="what">
            <strong>{ruleOf(ev) || $t('histviz.guards.removedGuard')}</strong>
            <span class="muted">{nameOf(ev.account_id)} · {when(ev.at)} · {ev.detail}</span>
          </span>
          <Button size="sm" variant="primary" onclick={() => onack?.(ev.id)}>{$t('histviz.guards.ack')}</Button>
        </li>
      {/each}
    </ul>
    {#if events.length > 1}
      <div class="foot">
        <Button size="sm" variant="ghost" onclick={() => onack?.(null)}>{$t('histviz.guards.ackAll')}</Button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .gwin {
    position: fixed;
    top: 72px;
    right: var(--space-6);
    z-index: 10000;
    width: min(380px, calc(100vw - 2 * var(--space-4)));
    display: flex;
    flex-direction: column;
    border: 1px solid var(--amber);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.35);
  }
  .ghead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    color: var(--amber);
    font-weight: var(--fw-medium);
    cursor: move;
    user-select: none;
    touch-action: none;
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: 50vh;
    overflow: auto;
    margin: 0;
    padding: var(--space-3);
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .muted {
    color: var(--muted);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    padding: 0 var(--space-3) var(--space-3);
  }
</style>
