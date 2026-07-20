/**
 * Theme colors for canvas-drawn charts (uPlot and ECharts alike).
 *
 * SVG charts can write `fill="var(--chart-1)"` and follow the theme for free.
 * Canvas ones need concrete hex strings, so they read the custom properties off
 * `<html>` with getComputedStyle — and a value read once at build time never
 * changes when the user flips the theme. The chart keeps drawing in the old
 * palette until something forces a rebuild.
 *
 * The subtle part: a bare `getComputedStyle` helper is not a rune, so an
 * `$effect` that calls it records no dependency on the theme and never re-runs.
 * `chartColors()` touches the theme store, which is what makes the effect
 * re-fire — read it *inside* the effect (or via `$derived`), not once at init:
 *
 *   const c = $derived(chartColors());
 *   $effect(() => { c; rebuildPlot(); });   // repaints on theme flip
 *
 * Series colors are the categorical ramp, assigned in fixed order and never
 * cycled: a 9th series folds into "Other" rather than reusing --chart-1.
 * `diverge` is the signed-midpoint scale (correlation): two hues + a neutral
 * middle, never a hue at zero.
 */

import { theme } from './store.svelte.js';

const SERIES_COUNT = 8;

function read(name, fallback) {
  if (typeof window === 'undefined') return fallback;
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

/**
 * The current theme's chart palette. Reading `theme.choice` (a rune) is what
 * makes this reactive — do not "optimize" it away.
 *
 * @returns {{series: string[], diverge: string[], accent: string, accentContrast: string,
 *            muted: string, dim: string, faint: string, border: string,
 *            borderControl: string, gridLine: string,
 *            surface: string, surface2: string, bg: string, mono: string,
 *            text: string, green: string, red: string, amber: string}}
 */
export function chartColors() {
  // Touch the store so any $derived/$effect reading us re-runs on a theme change.
  // `resolved` also covers 'system' flipping under us.
  void theme.choice;
  void theme.resolved;

  const series = Array.from({ length: SERIES_COUNT }, (_, i) =>
    read(`--chart-${i + 1}`, '#9a7b34')
  );

  return {
    series,
    // Ordered low → mid → high, ready to hand straight to a visualMap `inRange`.
    diverge: [
      read('--diverge-neg', '#b05548'),
      read('--diverge-mid', '#ecebe6'),
      read('--diverge-pos', '#4f8a68')
    ],
    accent: read('--accent', '#9a7b34'),
    accentContrast: read('--accent-contrast', '#ffffff'),
    muted: read('--muted', '#5c5f66'),
    dim: read('--dim', '#7a7d84'),
    faint: read('--faint', '#a8aab0'),
    border: read('--border', '#e2e1db'),
    borderControl: read('--border-control', '#d4d3cc'),
    gridLine: read('--grid-line', '#ececea'),
    surface: read('--surface', '#ffffff'),
    surface2: read('--surface-2', '#f4f3ef'),
    bg: read('--bg', '#ffffff'),
    mono: read('--mono', "ui-monospace, 'SF Mono', Menlo, monospace"),
    text: read('--text', '#1a1b1e'),
    green: read('--green', '#4f8a68'),
    red: read('--red', '#b05548'),
    amber: read('--amber', '#9a7b34')
  };
}

/**
 * Institutional equity-curve line style, ready to spread into an ECharts line
 * series (or read piecemeal for a uPlot/SVG chart). Gold 1.5px stroke, hollow
 * data points (bg-filled, 1px gold ring, r=2), NO area fill, NO gradient.
 *
 *   const c = $derived(chartColors());
 *   series: [{ type: 'line', data, ...equityLineStyle(c) }]
 */
export function equityLineStyle(c) {
  return {
    showSymbol: true,
    symbol: 'circle',
    symbolSize: 4, // r=2
    lineStyle: { color: c.accent, width: 1.5 },
    itemStyle: { color: c.bg, borderColor: c.accent, borderWidth: 1 },
    areaStyle: null
  };
}

/**
 * Shared axis/grid style for institutional charts: hairline gridlines in
 * --grid-line, axis labels in monospace 10px --dim. No axis tick marks.
 * Per the spec, gridlines are HORIZONTAL only — apply this to the value (y) axis;
 * the category (x) axis must set `splitLine: { show: false }` (see xAxisStyle).
 */
export function axisStyle(c) {
  return {
    splitLine: { show: true, lineStyle: { color: c.gridLine, width: 0.5 } },
    axisLine: { show: false },
    axisTick: { show: false },
    axisLabel: { color: c.dim, fontFamily: c.mono, fontSize: 10 }
  };
}

/**
 * Category (x) axis: labels only, NO vertical gridlines and NO axis line —
 * the spec forbids vertical rules and axis lines entirely.
 */
export function xAxisStyle(c) {
  return {
    splitLine: { show: false },
    axisLine: { show: false },
    axisTick: { show: false },
    axisLabel: { color: c.dim, fontFamily: c.mono, fontSize: 10 }
  };
}

/**
 * Institutional tooltip: --surface-2 fill, hairline --border-control filet,
 * radius 0, no shadow, monospace figures in --text.
 */
export function tooltipStyle(c) {
  return {
    backgroundColor: c.surface2,
    borderColor: c.borderControl,
    borderWidth: 0.5,
    padding: [6, 10],
    textStyle: { color: c.text, fontFamily: c.mono, fontSize: 11 },
    extraCssText: 'border-radius:0;box-shadow:none;'
  };
}

/**
 * A translucent version of a chart color, for area fills under a line.
 * `alpha` is 0–1; the result is an 8-digit hex uPlot accepts.
 */
export function withAlpha(hex, alpha) {
  const a = Math.round(Math.max(0, Math.min(1, alpha)) * 255)
    .toString(16)
    .padStart(2, '0');
  return `${hex}${a}`;
}
