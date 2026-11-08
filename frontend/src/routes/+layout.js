// Pure SPA: the core is a separate Rust service; the frontend is static assets
// served by Caddy and talks to the API over HTTP (spec §6). No SSR.
import { localeReady } from '$lib/i18n';

export const ssr = false;
export const prerender = false;

// Hold first render until the stored locale's pack is in (only `en` is bundled).
export async function load() {
  await localeReady;
  return {};
}
