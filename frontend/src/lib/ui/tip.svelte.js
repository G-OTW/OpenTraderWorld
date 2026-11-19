// App-wide hover tooltip for charts. One host at the root (`TipHost.svelte`) renders
// whatever the last hovered mark pushed here, so a chart inside a widget card can show a
// readout without fighting the card's `overflow: hidden`.
//
//   onpointermove={(e) => tip.show(e, { title: 'Mon 12', rows: [{ label: 'PnL', value: '+120' }] })}
//   onpointerleave={() => tip.hide()}
//
// A row may carry `color` (a legend dot) and `tone` ('pos' | 'neg' | 'warn').
//
// `text` is a plain hint (a button's description, see `quickTitles.js`): wrapped, in body ink.
class Tip {
  open = $state(false);
  x = $state(0);
  y = $state(0);
  title = $state('');
  rows = $state([]);
  text = $state('');

  show(event, content) {
    this.x = event.clientX;
    this.y = event.clientY;
    this.title = content?.title ?? '';
    this.rows = content?.rows ?? [];
    this.text = content?.text ?? '';
    this.open = true;
  }

  move(event) {
    if (!this.open) return;
    this.x = event.clientX;
    this.y = event.clientY;
  }

  hide() {
    this.open = false;
  }
}

export const tip = new Tip();
