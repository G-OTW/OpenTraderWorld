// Default MSW handlers: the happy path of every endpoint the frontend tests touch.
//
// A test that needs another answer (an error, an empty list, a slow reply) overrides one
// route with `server.use(...)`; the override is dropped after the test by the reset in
// src/test/setup-unit.js, so tests never leak state into one another.
import { http, HttpResponse } from 'msw';
import * as fx from './fixtures.js';

/** A JSON error the way the backend's `ApiError` serializes it. */
export const apiError = (status, error, code) =>
  HttpResponse.json(code ? { error, code } : { error }, { status });

export const handlers = [
  http.get('*/api/journal/breakdown', () => HttpResponse.json({ breakdown: fx.breakdown() })),
  http.get('*/api/journal/categories', () => HttpResponse.json({ categories: fx.categories })),
  http.get('*/api/journal/templates', () => HttpResponse.json({ templates: fx.templates })),
  http.get('*/api/journal/strategies', () => HttpResponse.json({ strategies: [] })),
  http.get('*/api/journal/fee-schedules', () => HttpResponse.json({ fee_schedules: [] })),
  http.get('*/api/journal/fx/rates/:date', () => HttpResponse.json({ rates: fx.fxRows })),
  http.get('*/api/health', () =>
    HttpResponse.json({ status: 'ok', services: { core: 'ok', postgres: 'ok' } })
  )
];
