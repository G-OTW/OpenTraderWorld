import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { percentChange, statementYoY, statementRows, compatibleDataset, minimumBars, correlationData, correlationPairs, seasonValue, sortedCompanies, heatShade } from '../src/lib/modules/dashboard/widgets/insights.js';

const quarter = (year, n, value) => ({ fiscal_year: year, fiscal_period: `Q${n}`, period: `Q${n} FY${year}`, lines: { revenue: value } });

test('YoY matches fiscal identity despite absent and null quarters', () => {
  const rows = [quarter(2024, 4, 100), quarter(2025, 1, 140), quarter(2025, 3, 190), quarter(2025, 4, 120), quarter(2026, 1, null), quarter(2026, 4, 150)];
  assert.equal(statementYoY(rows, rows.at(-1), 'revenue'), 25);
  assert.equal(statementYoY(rows.slice(0, -1), rows[4], 'revenue'), null);
  assert.equal(statementYoY([rows[0], rows.at(-1)], rows.at(-1), 'revenue'), null);
});

test('YoY also matches annual fiscal years and suppresses non-positive bases', () => {
  const rows = [2023, 2025, 2026].map((year) => ({ fiscal_year: year, fiscal_period: 'FY', lines: { revenue: year === 2025 ? 100 : 150 } }));
  assert.equal(statementYoY(rows, rows.at(-1), 'revenue'), 50);
  assert.equal(statementYoY([rows[0], rows.at(-1)], rows.at(-1), 'revenue'), null);
  rows[1].lines.revenue = -100;
  assert.equal(statementYoY(rows, rows.at(-1), 'revenue'), null);
});

test('TTM requires four consecutive non-missing fiscal quarters across a year boundary', () => {
  const rows = [quarter(2024, 3, 10), quarter(2024, 4, 20), quarter(2025, 1, 30), quarter(2025, 2, 40)];
  const rolling = statementRows(rows, 'revenue', true);
  assert.equal(rolling.at(-1).lines.revenue, 100);
  assert.equal(rolling[2].lines.revenue, null);
  assert.equal(statementRows(rows.filter((r) => r.fiscal_period !== 'Q4'), 'revenue', true).at(-1).lines.revenue, null);
  rows[1].lines.revenue = null;
  assert.equal(statementRows(rows, 'revenue', true).at(-1).lines.revenue, null);
});

test('zero is a valid statement value; input rows remain intact', () => {
  const rows = [1, 2, 3, 4].map((n) => quarter(2025, n, 0));
  assert.equal(statementRows(rows, 'revenue', true).at(-1).lines.revenue, 0);
  assert.equal(rows.at(-1).period, 'Q4 FY2025');
  assert.equal(percentChange(0, 100), -100);
  assert.equal(percentChange(100, 0), null);
});

test('correlation consumes the real nested API response and summarizes distinct pairs', () => {
  const basket = { labels: ['wrong'], correlation: { labels: ['A', 'B', 'C'], matrix: [[1, .2, -.9], [.2, 1, .6], [-.9, .6, 1]] } };
  const { labels, matrix } = correlationData(basket);
  assert.deepEqual(labels, ['A', 'B', 'C']);
  assert.equal(matrix[0][2], -.9);
  assert.deepEqual(correlationPairs(labels, matrix).map((p) => p.value), [-.9, .6, .2]);
  assert.deepEqual(correlationData(null), { labels: [], matrix: [] });
});

test('basket compatibility allows different daily providers but rejects different intraday sessions', () => {
  const ds = (timeframe, provider) => ({ timeframe, provider });
  assert.equal(compatibleDataset(ds('1d', 'A'), ds('1d', 'B')), true);
  assert.equal(compatibleDataset(ds('1M', 'A'), ds('1M', 'B')), true);
  assert.equal(compatibleDataset(ds('1m', 'A'), ds('1m', 'B')), false);
  assert.equal(compatibleDataset(ds('1h', 'A'), ds('1h', 'A')), true);
  assert.equal(compatibleDataset(ds('1d', 'A'), ds('1w', 'A')), false);
  assert.equal(compatibleDataset(ds('1d', 'A'), null), false);
});

test('dataset eligibility accounts for return count when fitting four regimes', () => {
  assert.equal(minimumBars('regime', 4), 201);
  assert.equal(minimumBars('regime', 2), 200);
  assert.equal(minimumBars('volatility'), 60);
  assert.equal(minimumBars('correlation'), 3);
});

test('seasonality volume keeps raw units, percent metrics convert fractions, and zero differs from missing', () => {
  const num = (v) => String(v), pct = (v) => `${v}%`;
  assert.equal(seasonValue(1234, 'volume', num, pct), '1234');
  assert.equal(seasonValue(.025, 'return', num, pct), '2.5%');
  assert.equal(seasonValue(.025, 'volatility', num, pct), '2.5%');
  assert.equal(seasonValue(.025, 'range', num, pct), '2.5%');
  assert.equal(seasonValue(0, 'volume', num, pct), '0');
  assert.equal(seasonValue(null, 'return', num, pct), '—');
  assert.notEqual(heatShade(null), heatShade(0));
});

test('company ranking places unavailable values last in both directions and does not mutate source', () => {
  const rows = [{ ticker: 'C', metrics: { pe: null } }, { ticker: 'A', metrics: { pe: 0 } }, { ticker: 'B', metrics: { pe: 12 } }];
  assert.deepEqual(sortedCompanies(rows, 'pe', 'desc').map((r) => r.ticker), ['B', 'A', 'C']);
  assert.deepEqual(sortedCompanies(rows, 'pe', 'asc').map((r) => r.ticker), ['A', 'B', 'C']);
  assert.deepEqual(rows.map((r) => r.ticker), ['C', 'A', 'B']);
});

test('heatmap foreground meets 4.5:1 for the default palettes throughout the scale', () => {
  const css = fs.readFileSync(new URL('../src/lib/theme/default.css', import.meta.url), 'utf8');
  const rgb = (hex) => hex.match(/[\da-f]{2}/gi).map((v) => parseInt(v, 16));
  const lum = (color) => color.map((v) => v / 255).map((v) => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((a, b, i) => a + b * [.2126, .7152, .0722][i], 0);
  const contrast = (a, b) => (Math.max(lum(a), lum(b)) + .05) / (Math.min(lum(a), lum(b)) + .05);
  for (const theme of ['light', 'dark']) {
    const palette = css.split(`[data-theme='${theme}'] {`)[1].split('}')[0];
    const color = (token) => rgb(palette.match(new RegExp(`--${token}: (#[0-9a-f]{6})`))[1]);
    for (const mode of ['correlation', 'return', 'volatility', 'volume', 'range']) {
      for (const value of [-1, -.5, .001, .5, 1]) {
        const shade = heatShade(value, 1, mode);
        const [, hue, mix] = shade.match(/var\(--([\w-]+)\) ([\d.]+)%/);
        const alpha = Number(mix) / 100;
        const fill = color(hue).map((v, i) => alpha * v + (1 - alpha) * color('surface')[i]);
        assert.ok(contrast(color('text'), fill) >= 4.5, `${theme}/${mode}/${value}`);
      }
    }
  }
});

test('new widget copy is translated and drill-down routes exist', () => {
  const en = JSON.parse(fs.readFileSync(new URL('../src/lib/i18n/en.json', import.meta.url), 'utf8'));
  for (const name of ['FundamentalsWidget.svelte', 'QuantWidget.svelte', 'WidgetConfig.svelte', 'parts/Matrix.svelte']) {
    const source = fs.readFileSync(new URL(`../src/lib/modules/dashboard/widgets/${name}`, import.meta.url), 'utf8');
    for (const [, key] of source.matchAll(/\$t\('(dashboard\.widgets\.insights\.[^']+)'/g)) assert.ok(en[key], key);
    assert.equal(source.includes('/fundamentals/macro'), false);
  }
});
