import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    // Build to static assets served by Caddy (M6: dashboard is the client surface).
    adapter: adapter({
      pages: 'build',
      assets: 'build',
      fallback: 'index.html',
      precompress: false,
      strict: true
    }),

    // Content-Security-Policy, emitted as a <meta> tag in the built index.html.
    //
    // It lives here rather than only in the Caddyfile because SvelteKit injects one inline
    // <script> of its own, whose content changes with every build. In `hash` mode SvelteKit
    // computes that hash at build time and writes it into the policy; a hand-written header
    // could not keep up. Our own pre-paint script was moved to static/prepaint.js for the
    // same reason ('self' covers it, no hash to maintain).
    //
    // Almost everything the app needs is same-origin: fonts are bundled under /fonts and
    // thumbnails are proxied through /api/files/{id}. The only exception is the TradingView
    // embed used by the economic calendar, whose loader and iframe are listed below. Before
    // adding a host here, check whether the asset can be served locally instead.
    csp: {
      mode: 'hash',
      directives: {
        'default-src': ['self'],
        // TradingView's embed loader (economic calendar) is served from their CDN and
        // cannot be self-hosted: the script builds a signed widget URL at runtime.
        'script-src': ['self', 'https://s3.tradingview.com'],
        // Svelte components carry inline style attributes; style-src-attr cannot be
        // hashed. Inline *styles* are not an XSS primitive the way inline scripts are:
        // script-src above is what stops injected code from running.
        'style-src': ['self', 'unsafe-inline'],
        // data: for inline SVG/PNG payloads, blob: for canvases the chart exports.
        'img-src': ['self', 'data:', 'blob:'],
        'font-src': ['self'],
        // XHR, SSE and WebSocket all go to this origin only.
        'connect-src': ['self'],
        // The mail reader renders a message in a sandboxed srcdoc iframe, which inherits
        // this policy. The TradingView economic-calendar widget injects an iframe from
        // their widget origin.
        'frame-src': ['self', 'https://www.tradingview-widget.com', 'https://s.tradingview.com'],
        'media-src': ['self', 'blob:'],
        'object-src': ['none'],
        'base-uri': ['none'],
        'form-action': ['self'],
        'frame-ancestors': ['none']
      }
    }
  }
};

export default config;
