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
  /** Run the engine WITHOUT persisting — the ephemeral "Calculate" path. */
  computePreview: (s) =>
    req('/taxcalc/compute', { method: 'POST', body: JSON.stringify(s) }).then((r) => r.result),
  /** Run the engine and cache the breakdown on an already-saved scenario. */
  compute: (id) =>
    req(`/taxcalc/scenarios/${id}/compute`, { method: 'POST' }).then((r) => r.result)
};

/** Build a profile payload from a template (the "start from country" path). */
export function profileFromTemplate(t, name) {
  return {
    name: name || t.label,
    country: t.country || '',
    currency: t.default_currency,
    person_type: t.person_type,
    regime: t.regime,
    marginal_income_rate: t.marginal_income_rate,
    social_charges_rate: t.social_charges_rate,
    allowances: {
      capital_gains: { annual_free: t.cg_allowance },
      dividends: { annual_free: t.div_allowance }
    },
    loss_carry: {},
    holding_period_rules: (t.holding_relief || []).map(([min_days, rate]) => ({ min_days, rate })),
    wealth_tax: null,
    notes: t.source_note,
    is_custom: false
  };
}

// Formatting lives in $lib/format.js. Tax rates are already percentages (25 = 25%).
export { fmtMoney, fmtPct } from '$lib/format.js';
