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
 *            muted: string, border: string, surface: string, surface2: string,
 *            text: string, green: string, red: string, amber: string}}
 */
export function chartColors() {
  // Touch the store so any $derived/$effect reading us re-runs on a theme change.
  // `resolved` also covers 'system' flipping under us.
  void theme.choice;
  void theme.resolved;

  const series = Array.from({ length: SERIES_COUNT }, (_, i) =>
    read(`--chart-${i + 1}`, '#4f46e5')
  );

  return {
    series,
    // Ordered low → mid → high, ready to hand straight to a visualMap `inRange`.
    diverge: [
      read('--diverge-neg', '#2563eb'),
      read('--diverge-mid', '#e8eaee'),
      read('--diverge-pos', '#dc2626')
    ],
    accent: read('--accent', '#4f46e5'),
    // The only safe ink to draw ON --accent or a saturated mark; plain #fff drops
    // to 2.9:1 on the dark theme's lavender accent.
    accentContrast: read('--accent-contrast', '#ffffff'),
    muted: read('--muted', '#667085'),
    border: read('--border', '#e4e7f0'),
    surface: read('--surface', '#ffffff'),
    surface2: read('--surface-2', '#f1f3f9'),
    text: read('--text', '#171a21'),
    green: read('--green', '#0f9d58'),
    red: read('--red', '#dc2626'),
    amber: read('--amber', '#d97706')
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
