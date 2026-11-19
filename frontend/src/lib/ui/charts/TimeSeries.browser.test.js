// The shared time-series chart (uPlot) in a real browser: an empty series, a single
// point, a gap in the data, and a container that changes width. Values are read through
// the onHover callback and the tooltip, the way a reader reaches them.
import { describe, expect, test, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page, userEvent } from 'vitest/browser';
import SizedBox from '../../../test/harness/SizedBox.svelte';
import TimeSeries from './TimeSeries.svelte';

const box = (props, width = 600) =>
  render(SizedBox, { props: { component: TimeSeries, inner: { sensitive: false, ...props }, width } });

const canvasOf = (container) => container.querySelector('canvas');
const overOf = (container) => container.querySelector('.u-over');

/** Hover the plot at a horizontal fraction of its width. */
async function hoverAt(container, fx) {
  const over = overOf(container);
  const { width, height } = over.getBoundingClientRect();
  await userEvent.hover(page.elementLocator(over), {
    position: { x: Math.max(1, Math.min(width - 1, width * fx)), y: height / 2 }
  });
}

describe('TimeSeries', () => {
  test('an empty series keeps the chart height and draws no plot', async () => {
    const { container } = await box({ series: [], ariaLabel: 'Net worth' });
    const chart = page.getByRole('img', { name: 'Net worth' });
    await expect.element(chart).toBeVisible();
    expect(chart.element().getBoundingClientRect().height).toBe(240);
    expect(canvasOf(container)).toBeNull();
  });

  test('a series with no point is the same empty chart', async () => {
    const { container } = await box({ series: [{ label: 'Net worth', points: [] }], height: 180 });
    const chart = page.getByRole('img', { name: 'Net worth' });
    await expect.element(chart).toBeVisible();
    expect(chart.element().getBoundingClientRect().height).toBe(180);
    expect(canvasOf(container)).toBeNull();
  });

  test('a single point is drawn and its value is read under the cursor', async () => {
    const onHover = vi.fn();
    const { container } = await box({
      series: [{ label: 'Net worth', points: [['2026-03-14', 1200]] }],
      onHover
    });
    await expect.poll(() => canvasOf(container)).not.toBeNull();
    await hoverAt(container, 0.5);
    await expect.poll(() => onHover.mock.calls.at(-1)?.[0]?.values).toEqual([
      { key: 'Net worth', label: 'Net worth', value: 1200 }
    ]);
    // One point spans no time, so the tooltip names the instant.
    await expect.element(page.getByText('Mar 14, 2026, 12:00 AM')).toBeVisible();
    await expect.element(page.getByText('1,200', { exact: true })).toBeVisible();
  });

  test('a null value is a gap: under the cursor that series reports nothing', async () => {
    const onHover = vi.fn();
    const { container } = await box({
      series: [
        { label: 'Fund', points: [['2026-03-02', 10], ['2026-03-03', null], ['2026-03-04', 30]] },
        { label: 'Index', points: [['2026-03-02', 1], ['2026-03-03', 2], ['2026-03-04', 3]] }
      ],
      change: 'none',
      onHover
    });
    await expect.poll(() => canvasOf(container)).not.toBeNull();
    // Three evenly spaced days: the middle of the plot is the 3rd, the gap.
    await hoverAt(container, 0.5);
    await expect.poll(() => onHover.mock.calls.at(-1)?.[0]).toMatchObject({
      index: 1,
      values: [{ key: 'Index', label: 'Index', value: 2 }]
    });
    // Either side of the gap both series answer.
    await hoverAt(container, 0.98);
    await expect.poll(() => onHover.mock.calls.at(-1)?.[0]?.values.map((v) => v.value)).toEqual([30, 3]);
    // Two series: the legend names both.
    await expect.element(page.getByRole('button', { name: 'Fund' })).toBeVisible();
    await expect.element(page.getByRole('button', { name: 'Index' })).toBeVisible();
  });

  test('the plot follows its container when it is resized', async () => {
    const series = [{ label: 'Net worth', points: [['2026-03-02', 10], ['2026-03-04', 30]] }];
    const screen = await box({ series }, 600);
    await expect.poll(() => canvasOf(screen.container)?.getBoundingClientRect().width).toBe(600);
    await screen.rerender({ width: 320 });
    await expect.poll(() => canvasOf(screen.container)?.getBoundingClientRect().width).toBe(320);
    await screen.rerender({ width: 900 });
    await expect.poll(() => canvasOf(screen.container)?.getBoundingClientRect().width).toBe(900);
  });
});
