// Pure SPA: the core is a separate Rust service; the frontend is static assets
// served by Caddy and talks to the API over HTTP (spec §6). No SSR.
export const ssr = false;
export const prerender = false;
