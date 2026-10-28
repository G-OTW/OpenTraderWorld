/** TaxCalculator API client.
 *
 * Trading/investing tax estimation only (no personal tax). A reusable Profile (country + person
 * type + rules) drives a per-year Scenario; the engine computes a breakdown. Templates are the
 * read-only country rule library. Estimates only — not tax advice. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

export const taxcalcApi = {
  /** Country rule templates (the regime library). */
  templates: () => req('/taxcalc/templates').then((r) => r.templates),

  profiles: () => req('/taxcalc/profiles').then((r) => r.profiles),
  profile: (id) => req(`/taxcalc/profiles/${id}`).then((r) => r.profile),
  createProfile: (p) =>
    req('/taxcalc/profiles', { method: 'POST', body: JSON.stringify(p) }).then((r) => r.id),
  updateProfile: (id, p) =>
    req(`/taxcalc/profiles/${id}`, { method: 'PUT', body: JSON.stringify(p) }),
  deleteProfile: (id) => req(`/taxcalc/profiles/${id}`, { method: 'DELETE' }),

  scenarios: () => req('/taxcalc/scenarios').then((r) => r.scenarios),
  scenario: (id) => req(`/taxcalc/scenarios/${id}`).then((r) => r.scenario),
  createScenario: (s) =>
    req('/taxcalc/scenarios', { method: 'POST', body: JSON.stringify(s) }).then((r) => r.id),
  updateScenario: (id, s) =>
    req(`/taxcalc/scenarios/${id}`, { method: 'PUT', body: JSON.stringify(s) }),
  deleteScenario: (id) => req(`/taxcalc/scenarios/${id}`, { method: 'DELETE' }),
  /** What a broker account realized inside a tax year, per line of the form. Reads only:
   * nothing is stored, the answer fills the form. */
  brokerPreview: (body) =>
    req('/taxcalc/broker/preview', { method: 'POST', body: JSON.stringify(body) }),
  /** The same read, run on the server: resolves to `{ task }`. See `$lib/tasks`. */
  brokerPreviewBg: (body) =>
    req('/taxcalc/broker/preview?background=true', {
      method: 'POST',
      body: JSON.stringify(body)
    }),

  /** Run the engine WITHOUT persisting — the ephemeral "Calculate" path. */
  computePreview: (s) =>
    req('/taxcalc/compute', { method: 'POST', body: JSON.stringify(s) }).then((r) => r.result),
  /** The loss registry of a profile: its pools, the per-year rows, and what each pool
   * still carries into `year`. */
  losses: (profileId, year) =>
    req(`/taxcalc/profiles/${profileId}/losses?year=${encodeURIComponent(year)}`),
  /** A year's own net result in one pool (negative = a loss). */
  putLoss: (profileId, year, pool, net, note = '') =>
    req(`/taxcalc/profiles/${profileId}/losses/${year}/${encodeURIComponent(pool)}`, {
      method: 'PUT',
      body: JSON.stringify({ net, note })
    }),
  deleteLoss: (profileId, year, pool) =>
    req(`/taxcalc/profiles/${profileId}/losses/${year}/${encodeURIComponent(pool)}`, {
      method: 'DELETE'
    }),

  /** Run the engine and cache the breakdown on an already-saved scenario. */
  compute: (id) =>
    req(`/taxcalc/scenarios/${id}/compute`, { method: 'POST' }).then((r) => r.result)
};

/** Build a profile payload from a template (the "start from country" path). A file-backed
 * regime carries its own rates, so only the custom one copies rates into the profile. */
export function profileFromTemplate(t, name) {
  return {
    name: name || t.label,
    country: t.country || '',
    currency: t.default_currency,
    person_type: t.person_type,
    regime: t.regime,
    flat_rate: t.flat_rate ?? null,
    marginal_income_rate: t.marginal_income_rate ?? null,
    social_charges_rate: t.social_charges_rate ?? null,
    allowances: {
      capital_gains: { annual_free: t.cg_allowance ?? 0 },
      dividends: { annual_free: t.div_allowance ?? 0 }
    },
    loss_carry: {},
    holding_period_rules: (t.holding_relief || []).map(([min_days, rate]) => ({ min_days, rate })),
    wealth_tax: null,
    notes: t.custom ? t.source_note : '',
    is_custom: !!t.custom,
    cost_method: null
  };
}

/** The first and last day of tax year `year` under a regime's calendar, as ISO days. */
export function yearSpan(t, year) {
  const cal = t?.calendar;
  if (!cal || cal.year_start === '01-01') return null;
  const [m, d] = cal.year_start.split('-').map(Number);
  const startYear = cal.label === 'end' ? year - 1 : year;
  const start = new Date(Date.UTC(startYear, m - 1, d));
  const end = new Date(Date.UTC(startYear + 1, m - 1, d - 1));
  return { start: start.toISOString().slice(0, 10), end: end.toISOString().slice(0, 10) };
}

// Formatting lives in $lib/format.js. Tax rates are already percentages (25 = 25%).
export { fmtMoney, fmtPct } from '$lib/format.js';
