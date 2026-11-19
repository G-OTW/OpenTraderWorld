// MSW server for the jsdom (unit) project. Started once per test file by
// src/test/setup-unit.js; tests add per-test overrides with `server.use(...)`.
import { setupServer } from 'msw/node';
import { handlers } from './handlers.js';

export const server = setupServer(...handlers);
