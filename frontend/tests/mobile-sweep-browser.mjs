// Fixture-backed viewport sweep for every SvelteKit page route.
// Run with the frontend dev server: node tests/mobile-sweep-browser.mjs [base URL] [--browser=webkit].
import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { installApiFixture, launchBrowser, parseArgs, reachabilityAudit, routePaths } from './mobile-helpers.mjs';

const { base, browserName } = parseArgs();
const artifacts = fileURLToPath(new URL(`../node_modules/.cache/mobile-sweep/${browserName === 'chromium' ? '' : `${browserName}/`}`, import.meta.url));
await mkdir(artifacts, { recursive: true });
const paths = await routePaths();

// Phone portrait, phone landscape, small tablet, tablet portrait and landscape.
const viewports = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 844 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 },
  { width: 1024, height: 768 }
];

const browser = await launchBrowser(browserName, artifacts);
const context = await browser.newContext({ viewport: { width: 390, height: 844 }, locale: 'en-US' });
await installApiFixture(context);

const report = [];
for (const path of paths) {
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  const listener = (e) => errors.push(e.stack || e.message);
  const apiPaths = new Set();
  const requestListener = (request) => {
    const pathname = new URL(request.url()).pathname;
    if (pathname.startsWith('/api/')) apiPaths.add(pathname);
  };
  page.on('pageerror', listener);
  page.on('request', requestListener);
  const entry = { route: path, widths: {}, reachability: {}, errors };
  try {
    const response = await page.goto(`${base}${path}`, { waitUntil: 'domcontentloaded', timeout: 15000 });
    entry.status = response?.status();
    try {
      await page.waitForFunction(() => document.body.innerText.trim().length > 20, undefined, { timeout: 6000 });
    } catch {
      entry.notRendered = true;
    }
    entry.url = page.url();
    entry.bodyLength = await page.evaluate(() => document.body.innerText.trim().length);
    for (const { width, height } of viewports) {
      await page.setViewportSize({ width, height });
      await page.waitForTimeout(60);
      const measure = await page.evaluate(() => ({
        viewport: document.documentElement.clientWidth,
        scroll: document.documentElement.scrollWidth,
        overflowers: [...document.querySelectorAll('body *')].filter((el) => {
          const rect = el.getBoundingClientRect();
          const style = getComputedStyle(el);
          return rect.right > document.documentElement.clientWidth + 1 && style.position !== 'fixed' && style.visibility !== 'hidden' && rect.width > 0;
        }).slice(0, 6).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className : ''}`)
      }));
      entry.widths[width] = measure;
      if (width === 320) {
        if (path === '/agent') {
          const headerAndComposer = await page.evaluate(() => {
            const title = document.querySelector('.chat-head h1');
            const textarea = document.querySelector('.composer textarea');
            return {
              titleFits: title && title.scrollWidth <= title.clientWidth + 1 && title.scrollHeight <= title.clientHeight + 1,
              composerFits: textarea && textarea.scrollHeight <= textarea.clientHeight + 1
            };
          });
          assert.equal(headerAndComposer.titleFits, true, 'Agent conversation title must remain readable at 320 px');
          assert.equal(headerAndComposer.composerFits, true, 'Agent composer placeholder must not be cut at 320 px');
        }
        if (path === '/calendar') {
          const actionsFit = await page.evaluate(() => [...document.querySelectorAll('.head button, .cal-wrap .toolbar button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && rect.width >= 44 && rect.height >= 44;
          }));
          assert.equal(actionsFit, true, 'Calendar header and toolbar actions must fit and remain tappable at 320 px');
          const monthFits = await page.locator('.fc-host').evaluate((calendar) => calendar.scrollWidth <= calendar.clientWidth + 1);
          assert.equal(monthFits, true, 'Calendar month grid must show all seven days at 320 px');
        }
        if (path === '/mindset/history') {
          const rangesFit = await page.locator('.limits').evaluate((limits) => [...limits.querySelectorAll('button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && rect.width >= 44 && rect.height >= 44;
          }));
          assert.equal(rangesFit, true, 'All Mindset history ranges must fit at 320 px');
        }
        if (path === '/backtest/paper/fixture-id') {
          const tabsFit = await page.evaluate(() => [...document.querySelectorAll('.view-tabs button, .book .tabs button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && button.scrollWidth <= button.clientWidth + 1;
          }));
          assert.equal(tabsFit, true, 'Paper Backtest tabs must remain fully visible at 320 px');
        }
        if (path === '/backtest/optimize/fixture-id') {
          const tabsFit = await page.locator('.tabs').evaluate((tabs) => [...tabs.querySelectorAll('button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && button.scrollWidth <= button.clientWidth + 1;
          }));
          assert.equal(tabsFit, true, 'Optimizer tabs must remain fully visible at 320 px');
        }
        if (path === '/todos') {
          const summaryReadable = await page.locator('.head .sub').evaluate((summary) => summary.scrollWidth <= summary.clientWidth + 1 && summary.getBoundingClientRect().width >= 100);
          assert.equal(summaryReadable, true, 'ToDo summary must not collapse into a narrow column at 320 px');
        }
        await page.screenshot({ path: `${artifacts}/${path.replaceAll('/', '_') || '_root'}-320.png`, fullPage: true });
      }
      // Diagnostic only: listed in the report, never fails the run.
      entry.reachability[`${width}x${height}`] = await page.evaluate(reachabilityAudit, { scope: '.module-context' });
    }
    await page.setViewportSize({ width: 390, height: 844 });
    // 200% text at 390 px: root font size doubled, so every rem-based size scales like browser text zoom.
    await page.evaluate(() => { document.documentElement.style.fontSize = '200%'; });
    await page.waitForTimeout(120);
    entry.text200 = await page.evaluate(() => {
      const vw = document.documentElement.clientWidth;
      const clipped = [];
      for (const el of document.querySelectorAll('.module-context button, .module-context a[href], .module-context input:not([type="hidden"]), .module-context select, .module-context textarea, .module-context [role="tab"]')) {
        if (el.closest('[inert], [aria-hidden="true"], [hidden]')) continue;
        const rect = el.getBoundingClientRect();
        const style = getComputedStyle(el);
        if (rect.width < 1 || rect.height < 1 || style.visibility === 'hidden') continue;
        const field = /^(INPUT|SELECT|TEXTAREA)$/.test(el.tagName);
        const cutX = !field && style.overflowX !== 'visible' && el.scrollWidth > el.clientWidth + 1;
        const cutY = !field && style.overflowY !== 'visible' && el.scrollHeight > el.clientHeight + 1;
        const offscreen = rect.right > vw + 1 || rect.left < -1;
        if (cutX || cutY || offscreen) {
          const cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(Boolean).slice(0, 2).join('.') : '';
          const name = (el.getAttribute('aria-label') || el.innerText || el.getAttribute('placeholder') || '').trim().replace(/\s+/g, ' ').slice(0, 40);
          clipped.push(`${el.tagName.toLowerCase()}${cls ? `.${cls}` : ''}${name ? ` "${name}"` : ''}: ${[cutX && 'cut-x', cutY && 'cut-y', offscreen && 'offscreen'].filter(Boolean).join('+')}`);
        }
      }
      return { viewport: vw, scroll: document.documentElement.scrollWidth, clipped };
    });
    await page.evaluate(() => { document.documentElement.style.fontSize = ''; });
    await page.waitForTimeout(120);
    await page.screenshot({ path: `${artifacts}/${path.replaceAll('/', '_') || '_root'}.png`, fullPage: true, timeout: 10000 });
    const visibleTypeError = await page.evaluate(() => document.body.innerText.match(/Cannot read properties of (?:undefined|null)|TypeError:|\.map is not a function/i)?.[0] ?? null);
    assert.equal(visibleTypeError, null, `${path} must render without a visible fixture type error`);
    if (path === '/community-docs') {
      await page.locator('.cat-card').first().click();
      await page.locator('.doc-card').first().click();
      await page.locator('.article').waitFor();
      await page.setViewportSize({ width: 320, height: 568 });
      const readerWidth = await page.evaluate(() => ({ viewport: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
      assert.ok(readerWidth.scroll <= readerWidth.viewport + 1, 'Community Docs reader must fit the phone viewport');
      await page.locator('.article .content pre').scrollIntoViewIfNeeded();
      await page.locator('.page').evaluate((element) => { element.scrollTop = element.scrollHeight; });
      const codeReachable = await page.locator('.article .content pre').evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return bounds.top >= 0 && bounds.bottom <= innerHeight - 60 && bounds.width <= document.documentElement.clientWidth;
      });
      assert.equal(codeReachable, true, 'Community Docs reader must scroll to the final content');
      await page.screenshot({ path: `${artifacts}/community-docs-reader-320.png`, fullPage: true });
      await page.setViewportSize({ width: 390, height: 844 });
      entry.interactions = 'category to document reader';
    } else if (path === '/findb') {
      await page.getByRole('button', { name: /Favorites/ }).first().click();
      await page.locator('.mobile-folders select').selectOption('folder-1');
      await page.locator('.fav-detail').getByText('AAPL').waitFor();
      await page.screenshot({ path: `${artifacts}/findb-favorites.png`, fullPage: true });
      entry.interactions = 'folder selector and favorite record';
    } else if (path === '/histviz') {
      const chartButton = page.locator('.mobile-pane-nav button').nth(1);
      await chartButton.click();
      assert.equal(await page.locator('.grid-wrap').isVisible(), true);
      await page.screenshot({ path: `${artifacts}/histviz-chart.png`, fullPage: true });
      entry.interactions = 'instrument list to chart pane';
    } else if (path === '/backtest') {
      await page.locator('.mobile-sections button').nth(1).click();
      assert.equal(await page.locator('.dock').isVisible(), true);
      await page.screenshot({ path: `${artifacts}/backtest-library.png`, fullPage: true });
      await page.locator('.mobile-sections button').first().click();
      assert.equal(await page.locator('.work').isVisible(), true);
      await page.setViewportSize({ width: 320, height: 568 });
      await page.locator('.step-foot .run').scrollIntoViewIfNeeded();
      const runReachable = await page.locator('.step-foot .run').evaluate((button) => {
        const rect = button.getBoundingClientRect();
        const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
        return rect.top >= 0 && rect.bottom <= innerHeight && !!hit?.closest('.run');
      });
      assert.equal(runReachable, true, 'Backtest Run stays reachable above floating shortcuts at 320 px');
      await page.screenshot({ path: `${artifacts}/backtest-run-visible-320.png`, fullPage: true });
      await page.setViewportSize({ width: 390, height: 844 });
      entry.interactions = 'configuration to library and back';
    } else if (path === '/taxcalc') {
      await page.locator('.mobile-new-profile').click();
      await page.locator('[role="dialog"]').waitFor();
      await page.waitForTimeout(250);
      await page.screenshot({ path: `${artifacts}/taxcalc-new-profile.png`, fullPage: true });
      entry.interactions = 'new profile dialog';
    } else if (path === '/fundamentals') {
      const toggle = page.locator('.mobile-catalog-toggle');
      assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
      await toggle.click();
      assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
      assert.equal(await page.locator('#macro-catalog').isVisible(), true);
      await page.screenshot({ path: `${artifacts}/fundamentals-catalog-open.png`, fullPage: true });
      await toggle.click();
      entry.interactions = 'series catalog open and close';
    } else if (path === '/automator/fixture-id') {
      await page.locator('.mobile-workspace button').nth(1).click();
      await page.locator('.palette-slot .block').first().click();
      await page.locator('[role="dialog"]').waitFor();
      await page.keyboard.press('Escape');
      await page.locator('.canvas-slot .task').first().waitFor();
      await page.locator('.mobile-workspace button').nth(1).click();
      await page.locator('.palette-slot .block').nth(1).click();
      await page.locator('[role="dialog"]').waitFor();
      await page.keyboard.press('Escape');
      assert.equal(await page.locator('.canvas-slot .task').count(), 2);
      await page.locator('.mobile-move button').filter({ hasText: 'Move up' }).last().click();
      assert.match(await page.locator('.canvas-slot .task').first().innerText(), /agent1|AI step/i);
      await page.screenshot({ path: `${artifacts}/automator-mobile-canvas.png`, fullPage: true });
      entry.interactions = 'block palette to canvas by tap, then move up';
    }
    if (['/', '/automator/fixture-id', '/backtest', '/findb', '/fundamentals', '/histviz', '/taxcalc'].includes(path)) {
      const name = path.replaceAll('/', '_') || '_root';
      await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
      await page.screenshot({ path: `${artifacts}/${name}-dark-390.png`, fullPage: true });
      await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
      await page.setViewportSize({ width: 1440, height: 900 });
      await page.screenshot({ path: `${artifacts}/${name}-desktop-1440.png`, fullPage: true });
    }
  } catch (error) {
    entry.failure = error.message;
  }
  entry.finalUrl = page.url();
  page.off('pageerror', listener);
  page.off('request', requestListener);
  entry.apiPaths = [...apiPaths].sort();
  report.push(entry);
  await page.close();
  const unreachable = Object.entries(entry.reachability).filter(([, r]) => r.offenders.length).map(([v, r]) => `${v}:${r.offenders.length}`).join(',');
  const text200 = entry.text200 ? `${entry.text200.scroll > entry.text200.viewport + 1 ? ` text200 OVERFLOW ${entry.text200.scroll}` : ''}${entry.text200.clipped.length ? ` text200 clipped ${entry.text200.clipped.length}` : ''}` : '';
  console.log(`${path}: ${Object.entries(entry.widths).filter(([, m]) => m.scroll > m.viewport + 1).map(([w, m]) => `${w}:${m.scroll}`).join(',') || 'fits'}${text200}${unreachable ? ` unreachable ${unreachable}` : ''}${entry.failure ? ` FAIL ${entry.failure.slice(0, 80)}` : ''}`);
}
await writeFile(`${artifacts}/report.json`, JSON.stringify(report, null, 2));
await browser.close();
const text200Overflow = report.filter((r) => r.text200 && r.text200.scroll > r.text200.viewport + 1);
const viewportFailures = report.filter((r) => r.failure || r.notRendered || Object.values(r.widths).some((m) => m.scroll > m.viewport + 1) || text200Overflow.includes(r));
const text200Clipped = report.filter((r) => r.text200?.clipped.length);
const reachOffenders = report.flatMap((r) => Object.entries(r.reachability).flatMap(([v, a]) => a.offenders.map((o) => ({ route: r.route, viewport: v, ...o }))));
const reachOffCanvas = report.flatMap((r) => Object.entries(r.reachability).flatMap(([v, a]) => a.offCanvas.map((o) => ({ route: r.route, viewport: v, ...o }))));
await writeFile(`${artifacts}/reachability.json`, JSON.stringify({ offenders: reachOffenders, offCanvas: reachOffCanvas }, null, 2));
const fixtureErrors = report.filter((r) => r.errors.some((error) => !error.includes('tradingview-widget.com')));
const externalErrors = report.filter((r) => r.errors.some((error) => error.includes('tradingview-widget.com')));
console.log(`${paths.length} routes checked; ${viewportFailures.length} viewport/interaction failures; ${fixtureErrors.length} routes have fixture/runtime errors; ${externalErrors.length} routes have third-party widget errors. Screenshots and report: ${artifacts}`);
console.log(`200% text at 390 px: ${text200Overflow.length} routes overflow (counted above), ${text200Clipped.length} routes have clipped controls.`);
console.log(`Primary action reachability (diagnostic, not a failure): ${reachOffenders.length} offenders on ${new Set(reachOffenders.map((o) => o.route)).size} routes, ${reachOffCanvas.length} off-canvas controls on ${new Set(reachOffCanvas.map((o) => o.route)).size} routes; list in ${artifacts}reachability.json`);
if (viewportFailures.length) process.exitCode = 1;
