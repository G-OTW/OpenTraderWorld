/** Block key → panel component.
 *
 * The frontend mirror of `ANALYSES` in `otw-core/src/portfolios/analytics/mod.rs`. Adding an
 * analysis is one Rust file, one line there, one Svelte file, and one line here. Nothing
 * else: the view loops over what the server returned and renders whatever it has a panel
 * for, so a block from a newer backend degrades to "not shown" instead of a broken page.
 */
import BookPanel from './BookPanel.svelte';
import CostsPanel from './CostsPanel.svelte';
import PerformancePanel from './PerformancePanel.svelte';
import RiskPanel from './RiskPanel.svelte';
import BenchmarkPanel from './BenchmarkPanel.svelte';
import DriftPanel from './DriftPanel.svelte';
import StressPanel from './StressPanel.svelte';
import HarvestPanel from './HarvestPanel.svelte';
import ComparePanel from './ComparePanel.svelte';
import ProjectionPanel from './ProjectionPanel.svelte';

export const PANELS = [
  { key: 'book', component: BookPanel },
  { key: 'performance', component: PerformancePanel },
  { key: 'risk', component: RiskPanel },
  { key: 'benchmark', component: BenchmarkPanel },
  { key: 'costs', component: CostsPanel },
  { key: 'drift', component: DriftPanel },
  { key: 'stress', component: StressPanel },
  // `self`: the panel reads its own endpoint (harvest, compare, projection are not
  // analytics blocks), so the view hands it the portfolio and fetches nothing.
  { key: 'harvest', component: HarvestPanel, self: true },
  { key: 'compare', component: ComparePanel, self: true },
  { key: 'projection', component: ProjectionPanel, self: true }
];

export const isSelf = (key) => PANELS.find((p) => p.key === key)?.self === true;

export const panelFor = (key) => PANELS.find((p) => p.key === key)?.component ?? null;

/** Named windows, shortest first. Mirrors `analytics::WINDOWS`. */
export const WINDOWS = ['1m', '3m', '6m', 'ytd', '1y', '3y', '5y', 'inception'];
