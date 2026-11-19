// Setup for the `unit` Vitest project (jsdom).
//
// - jest-dom matchers (`toBeInTheDocument`, `toHaveTextContent`, ...).
// - MSW intercepts every fetch. An unmatched request fails the test: no test may reach
//   the network, and a call nobody mocked is a call nobody checked.
// - Overrides added with `server.use` are reset after each test.
// - Numbers format as en-US whatever the host LANG (a test wanting fr-FR asks for it with
//   withNumberLocale); the zone is pinned to UTC in vite.config.js.
import '@testing-library/jest-dom/vitest';
import { afterAll, afterEach, beforeAll } from 'vitest';
import { server } from '../mocks/node.js';
import { pinDefaultNumberLocale } from './locale.js';

pinDefaultNumberLocale('en-US');

// jsdom has no layout: give the few layout calls components make a no-op so they can run.
if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = function () {};

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());
