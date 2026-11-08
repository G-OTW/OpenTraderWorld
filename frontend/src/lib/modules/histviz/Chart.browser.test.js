// The Charts candle surface (ECharts) in a real browser: no bar, one candle, a hole in
// the data the pane could not read back, and a pane that is resized. The legend over the
// candles is read as text, the way the user reads it.
import { describe, expect, test } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import SizedBox from '../../../test/harness/SizedBox.svelte';
import Chart from './Chart.svelte';

const bars = (rows) => ({
  ts: rows.map((r) => r[0]),
  o: rows.map((r) => r[1]),
  h: rows.map((r) => r[2]),
  l: rows.map((r) => r[3]),
  c: rows.map((r) => r[4]),
  v: rows.map((r) => r[5])
});

const TITLE = { ticker: 'TBTEST', timeframe: '1h', provider: 'binance' };

const chart = (props, width = 900, height = 480) =>
  render(SizedBox, { props: { component: Chart, inner: { title: TITLE, ...props }, width, height } });

const head = (container) => container.querySelector('.chart-host .head');
const flat = (el) => el?.textContent.replace(/\s+/g, ' ').trim() ?? '';
const canvasWidth = (container) => container.querySelector('.chart-host canvas')?.getBoundingClientRect().width;

/** n hourly bars from 2026-03-14T00:00Z: bar i opens at 1000 + i (same formula as testbook). */
const hourly = (n, from = 0) =>
  Array.from({ length: n }, (_, k) => {
    const i = from + k;
    const o = 1000 + i;
    return [new Date(Date.UTC(2026, 2, 14, i)).toISOString(), o, o + 5, o - 3, o + 2, 10 + i];
  });

describe('histviz Chart', () => {
  test('no bar: the legend names the instrument and prints no price', async () => {
    const { container } = await chart({ bars: bars([]) });
    await expect.element(page.getByRole('application')).toBeVisible();
    await expect.poll(() => flat(head(container))).toBe('TBTEST · 1h · binance Volume');
  });

  test('bars not loaded yet (null) render the same empty surface', async () => {
    const { container } = await chart({ bars: null });
    await expect.element(page.getByRole('application')).toBeVisible();
    expect(head(container)).toBeNull();
  });

  test('one candle: the legend reads its open, high, low, close and change', async () => {
    const { container } = await chart({
      bars: bars([['2026-03-14T10:00:00Z', 100.25, 105.5, 98.75, 103.1, 1500]])
    });
    // Change = 103.10 - 100.25 = +2.85, i.e. +2.84% of the open (2.85 / 100.25 = 2.843%).
    await expect
      .poll(() => flat(head(container)))
      .toContain('TBTEST · 1h · binance O100.25 H105.50 L98.75 C103.10 +2.85 (+2.84%) Volume 1.5K');
  });

  test('a falling candle prints a negative change', async () => {
    const { container } = await chart({
      bars: bars([['2026-03-14T10:00:00Z', 50, 51, 40, 45, 10]])
    });
    // With no previous close the change is against the open: 45 - 50 = -5, -10%.
    // Whole-number prices print without decimals.
    // Negatives carry the app's U+2212 minus (format.js), never Intl's hyphen.
    await expect.poll(() => flat(head(container))).toContain('O50 H51 L40 C45 −5 (−10.00%)');
  });

  test('the legend shows the newest bar of a series', async () => {
    const { container } = await chart({ bars: bars(hourly(48)) });
    // Bar 47: open 1047, high 1052, low 1044, close 1049, volume 57. The change is against
    // the previous close (bar 46 closed at 1048): +1, i.e. 1 / 1048 = +0.10%.
    await expect
      .poll(() => flat(head(container)))
      .toContain('O1,047 H1,052 L1,044 C1,049 +1 (+0.10%) Volume 57');
  });

  test('a hole the pane could not read back is marked between its two neighbours', async () => {
    const before = hourly(24);
    const after = hourly(24, 30);
    const { container } = await chart({
      bars: bars([...before, ...after]),
      holes: [{ before: before.at(-1)[0], after: after[0][0], message: 'No bars from 00:00 to 06:00' }]
    });
    await expect.poll(() => container.querySelectorAll('line.hole').length).toBe(1);
    expect(container.querySelector('line.hole title').textContent).toBe('No bars from 00:00 to 06:00');
    // The hole sits half a bar right of the last bar before it, inside the plot.
    const hole = container.querySelector('line.hole');
    const x = Number(hole.getAttribute('x1'));
    expect(x).toBeGreaterThan(0);
    expect(x).toBeLessThan(container.querySelector('.chart-host').getBoundingClientRect().width);
  });

  test('the canvas follows the pane when it is resized, and the legend stays', async () => {
    const screen = await chart({ bars: bars(hourly(48)) }, 900);
    await expect.poll(() => canvasWidth(screen.container)).toBe(900);
    await screen.rerender({ width: 420 });
    await expect.poll(() => canvasWidth(screen.container)).toBe(420);
    await screen.rerender({ width: 1200 });
    await expect.poll(() => canvasWidth(screen.container)).toBe(1200);
    expect(flat(head(screen.container))).toContain('C1,049 +1 (+0.10%)');
  });
});
