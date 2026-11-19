// Desktop no-regression check for the mobile work: every page route, rendered with the sweep's
// fixture at 1440x900 and 1100x800 in light and dark, must produce byte-identical full-page PNGs
// on the baseline server (clean HEAD) and on the working tree.
// Run: node tests/mobile-desktop-baseline-browser.mjs [working tree URL] [--baseline=URL] [--browser=webkit]
//      [--only=/route,/other] [--resamples=N]
//
// Determinism, applied the same way on both servers so only real rendering changes remain:
// - fixed clock (context.clock.setFixedTime), seeded Math.random, counter-based crypto ids;
// - reducedMotion 'reduce', screenshots with animations disabled and the caret hidden;
// - every request outside the server origin is aborted (third-party widgets, fonts, avatars);
// - each capture starts from a fresh page, the theme comes from the persisted `otw-theme` choice
//   before first paint, and the capture waits for fonts and two animation frames.
// Remaining noise, measured with the baseline against itself: in the dark theme Chromium
// (headless shell and full Chrome, with or without --disable-gpu, partial raster or threaded
// compositing off) rasterizes a few anti-aliased icon and text edges differently from one load
// to the next (1 to 17 levels on a few dozen pixels) while the DOM, every computed style and
// every box are identical. A load renders one of a small set of variants, so a pair that
// differs is captured again up to --resamples times (default 6) per side; it counts as
// identical only when both sides produced the exact same PNG at least once. A real change has
// no common variant and stays reported.
import { createHash } from 'node:crypto';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { determinismInitScript, installApiFixture, launchBrowser, parseArgs, routePaths, routeSlug, tcpRelay } from './mobile-helpers.mjs';

const { base: targetUrl, flags, browserName } = parseArgs();
const referenceUrl = flags.baseline ?? 'http://127.0.0.1:4190';
// WebKit cannot open port 4190: both sides go through a local relay so they stay symmetric.
const relays = browserName === 'webkit' ? await Promise.all([tcpRelay(referenceUrl), tcpRelay(targetUrl)]) : [];
const reference = relays[0]?.url ?? referenceUrl;
const target = relays[1]?.url ?? targetUrl;
const artifacts = fileURLToPath(new URL(`../node_modules/.cache/mobile-desktop-baseline/${browserName === 'chromium' ? '' : `${browserName}/`}`, import.meta.url));
await rm(artifacts, { recursive: true, force: true });
await mkdir(artifacts, { recursive: true });

const only = typeof flags.only === 'string' ? flags.only.split(',') : null;
const paths = (await routePaths()).filter((p) => !only || only.includes(p));
const viewports = [{ width: 1440, height: 900 }, { width: 1100, height: 800 }];
const themes = ['light', 'dark'];
const fixedTime = new Date('2026-10-05T12:00:00Z');
const resamples = Number(flags.resamples ?? 6);
const hash = (png) => createHash('sha1').update(png).digest('hex');

// Pixel statistics for the report, using the PNG codec bundled with Playwright when it loads.
let PNG = null;
try { PNG = createRequire(import.meta.url)('playwright-core/lib/utilsBundle').PNG ?? null; } catch { /* stats skipped */ }
function pixelStats(a, b) {
  if (!PNG) return null;
  const A = PNG.sync.read(a);
  const B = PNG.sync.read(b);
  if (A.width !== B.width || A.height !== B.height) return { sizeChanged: true };
  let pixels = 0, maxDelta = 0, x0 = Infinity, y0 = Infinity, x1 = -1, y1 = -1;
  for (let i = 0; i < A.data.length; i += 4) {
    const d = Math.max(Math.abs(A.data[i] - B.data[i]), Math.abs(A.data[i + 1] - B.data[i + 1]), Math.abs(A.data[i + 2] - B.data[i + 2]), Math.abs(A.data[i + 3] - B.data[i + 3]));
    if (!d) continue;
    const p = i / 4, x = p % A.width, y = (p - x) / A.width;
    pixels++; maxDelta = Math.max(maxDelta, d);
    x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
  }
  return { pixels, maxDelta, box: [x0, y0, x1, y1] };
}

const browser = await launchBrowser(browserName, artifacts);

async function makeContext(server, theme) {
  const origin = new URL(server).origin;
  const context = await browser.newContext({ viewport: viewports[0], locale: 'en-US', timezoneId: 'UTC', colorScheme: theme, reducedMotion: 'reduce' });
  await context.clock.setFixedTime(fixedTime);
  await context.addInitScript(determinismInitScript);
  await context.addInitScript((choice) => { try { localStorage.setItem('otw-theme', choice); } catch { /* storage blocked */ } }, theme);
  await context.route((url) => url.origin !== origin, (route) => route.abort());
  await installApiFixture(context);
  return context;
}

async function capture(context, server, path, viewport) {
  for (let attempt = 0; ; attempt++) {
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    try {
      await page.setViewportSize(viewport);
      await page.goto(`${server}${path}`, { waitUntil: 'domcontentloaded', timeout: 20000 });
      await page.waitForFunction(() => document.body.innerText.trim().length > 20, undefined, { timeout: 6000 }).catch(() => {});
      await page.waitForLoadState('networkidle', { timeout: 6000 }).catch(() => {});
      await page.evaluate(() => document.fonts.ready.then(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))));
      await page.waitForTimeout(250);
      const png = await page.screenshot({ fullPage: true, animations: 'disabled', caret: 'hide', timeout: 15000 });
      const theme = await page.evaluate(() => document.documentElement.getAttribute('data-theme'));
      await page.close();
      return { png, errors, theme };
    } catch (error) {
      await page.close().catch(() => {});
      // The first navigation after a cold dependency optimization can reload the page: retry once.
      if (attempt >= 1) return { error: error.message, errors };
    }
  }
}

const pngSize = (buf) => ({ width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) });

const contexts = {};
for (const theme of themes) contexts[theme] = { ref: await makeContext(reference, theme), work: await makeContext(target, theme) };

const pairs = [];
for (const path of paths) {
  const line = [];
  for (const viewport of viewports) {
    for (const theme of themes) {
      const [ref, work] = await Promise.all([
        capture(contexts[theme].ref, reference, path, viewport),
        capture(contexts[theme].work, target, path, viewport)
      ]);
      const key = `${viewport.width}x${viewport.height}-${theme}`;
      const pair = { route: path, viewport: `${viewport.width}x${viewport.height}`, theme };
      if (ref.error || work.error) {
        pair.status = 'capture-failed';
        pair.error = ref.error ?? work.error;
      } else {
        pair.status = ref.png.equals(work.png) ? 'identical' : 'different';
        pair.reference = pngSize(ref.png);
        pair.target = pngSize(work.png);
        if (pair.status === 'different' && resamples > 0) {
          // Rasterization variance: look for one variant both sides can produce.
          const refHashes = new Set([hash(ref.png)]);
          const workHashes = new Set([hash(work.png)]);
          for (let i = 0; i < resamples && pair.status === 'different'; i++) {
            const [r, w] = await Promise.all([
              capture(contexts[theme].ref, reference, path, viewport),
              capture(contexts[theme].work, target, path, viewport)
            ]);
            if (r.png) refHashes.add(hash(r.png));
            if (w.png) workHashes.add(hash(w.png));
            if ([...refHashes].some((h) => workHashes.has(h))) {
              pair.status = 'identical';
              pair.matchedAfterResamples = i + 1;
            }
          }
          pair.variants = { reference: refHashes.size, target: workHashes.size };
        }
        if (pair.status === 'different') pair.pixels = pixelStats(ref.png, work.png);
        if (ref.theme !== theme || work.theme !== theme) pair.themeApplied = { reference: ref.theme, target: work.theme };
        if (pair.status === 'different') {
          const name = `${routeSlug(path)}-${key}`;
          await writeFile(`${artifacts}/${name}-baseline.png`, ref.png);
          await writeFile(`${artifacts}/${name}-work.png`, work.png);
          pair.files = [`${name}-baseline.png`, `${name}-work.png`];
        }
      }
      pair.pageErrors = { reference: ref.errors, target: work.errors };
      pairs.push(pair);
      line.push(pair.status === 'identical' ? '.' : pair.status === 'different' ? `DIFF ${key}` : `FAIL ${key}`);
    }
  }
  console.log(`${path}: ${line.every((l) => l === '.') ? 'identical' : line.filter((l) => l !== '.').join(', ')}`);
}

await browser.close();
await Promise.all(relays.map((r) => r.close()));
const differing = pairs.filter((p) => p.status === 'different');
const failed = pairs.filter((p) => p.status === 'capture-failed');
const wrongTheme = pairs.filter((p) => p.themeApplied);
const resampled = pairs.filter((p) => p.matchedAfterResamples);
await writeFile(`${artifacts}/report.json`, JSON.stringify({ reference: referenceUrl, target: targetUrl, relayed: relays.length > 0, browser: browserName, fixedTime, pairs }, null, 2));
console.log(`${paths.length} routes, ${pairs.length} pairs (${referenceUrl} vs ${targetUrl}, ${browserName}): ${pairs.length - differing.length - failed.length} identical, ${differing.length} different, ${failed.length} capture failures, ${resampled.length} identical only after resampling render variance${wrongTheme.length ? `, ${wrongTheme.length} with an unexpected data-theme` : ''}. Report and differing PNGs: ${artifacts}`);
if (differing.length || failed.length) process.exitCode = 1;
