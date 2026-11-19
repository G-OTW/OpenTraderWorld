// Setup for the `browser` Vitest project (real Chromium).
//
// The app's design tokens live in the theme stylesheets, so chart components resolve their
// colours and spacing the way they do in the app. Any request to /api fails the test: the
// browser tests render components from props only. The clock is frozen (Date only, so
// timers, animation frames and resize observers still run) on a fixed Monday.
import '$lib/theme/default.css';
import '$lib/theme/components.css';
import { afterEach, beforeEach, vi } from 'vitest';

beforeEach(() => vi.useFakeTimers({ now: new Date('2026-03-16T12:00:00Z'), toFake: ['Date'] }));
afterEach(() => vi.useRealTimers());

const realFetch = globalThis.fetch;
beforeEach(() => {
  globalThis.fetch = (input, init) => {
    const url = typeof input === 'string' ? input : input.url;
    if (url.includes('/api/')) throw new Error(`unexpected API call in a browser test: ${url}`);
    return realFetch(input, init);
  };
});
afterEach(() => {
  globalThis.fetch = realFetch;
});
