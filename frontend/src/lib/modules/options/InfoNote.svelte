<script>
  // A small mark for what a number leaves out: its note opens on hover, on keyboard focus and
  // on a click, the only one a touch screen has.
  import Icon from '$lib/ui/Icon.svelte';
  import { tip } from '$lib/ui/tip.svelte.js';

  let { note } = $props();

  const show = (e) => tip.show(e, { text: note });
  function onfocus(e) {
    const r = e.currentTarget.getBoundingClientRect();
    tip.show({ clientX: r.right, clientY: r.bottom }, { text: note });
  }
</script>

<button type="button" class="info" aria-label={note} onpointerenter={show} onpointerleave={() => tip.hide()} onclick={show} {onfocus} onblur={() => tip.hide()}>
  <Icon name="info" size={13} />
</button>

<style>
  .info {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    vertical-align: -2px;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    cursor: help;
  }
  .info:hover,
  .info:focus-visible {
    color: var(--text);
  }
  /* Touch: a 44 px target around the mark, without pushing the line it sits in apart. */
  @media (max-width: 767px), (hover: none) {
    .info {
      width: 44px;
      height: 44px;
      margin: -15px;
    }
  }
</style>
