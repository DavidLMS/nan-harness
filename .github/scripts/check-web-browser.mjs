// See ../web-check/README.md for pinned provisioning. An absent dependency fails;
// this check never downloads a module or browser implicitly.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { prepareWeb } from './prepare-web.mjs';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE
  || '../web-check/node_modules/playwright/index.mjs');
const staging = fs.mkdtempSync(path.join(os.tmpdir(), 'nanh-web-browser-'));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png' };
const server = http.createServer((request, response) => {
  const file = path.join(staging, new URL(request.url, 'http://localhost').pathname);
  const target = file === `${staging}/` ? path.join(staging, 'index.html') : file;
  if (!target.startsWith(`${staging}/`) || !fs.existsSync(target) || !fs.statSync(target).isFile()) {
    response.writeHead(404).end();
    return;
  }
  response.setHeader('Content-Type', types[path.extname(target)] || 'text/plain');
  response.end(fs.readFileSync(target));
});
let browser;
try {
  prepareWeb(staging);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ headless: true });
  if (process.env.WEB_SCREENSHOT_DIR) fs.mkdirSync(process.env.WEB_SCREENSHOT_DIR, { recursive: true });
  console.log(`Browser: ${browser.version()} (${process.platform}/${process.arch})`);
  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    for (const javaScriptEnabled of [false, true]) {
      await checkPages(origin, viewport, javaScriptEnabled);
    }
    await checkFailedScript(origin, viewport, 'app.js');
    await checkFailedScript(origin, viewport, 'interactions.js');
    await checkLocaleAndTools(origin, viewport);
  }
  await checkCachedHtml(origin);
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
  fs.rmSync(staging, { recursive: true, force: true });
}

async function isolatedContext(origin, options) {
  const context = await browser.newContext({ reducedMotion: 'reduce', ...options });
  // Never contact provider services or use the user's browser profile.
  await context.route('**/*', (route) => new URL(route.request().url()).origin === origin
    ? route.continue() : route.abort());
  return context;
}

async function checkStructure(page) {
  assert.equal(await page.locator('main').count(), 1);
  assert.equal(await page.locator('h1').count(), 1);
  assert.ok(await page.locator('h1').isVisible());
  assert.deepEqual(await page.locator('[id]').evaluateAll((nodes) => {
    const ids = nodes.map((node) => node.id);
    return ids.filter((id, index) => ids.indexOf(id) !== index);
  }), []);
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'No page-wide horizontal overflow');
}

async function checkPages(origin, viewport, javaScriptEnabled) {
  const context = await isolatedContext(origin, { viewport, javaScriptEnabled, locale: 'en-US' });
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  try {
    for (const file of ['index.html', 'docs.html', 'logos.html']) {
      await page.goto(`${origin}/${file}`);
      await checkStructure(page);
      if (process.env.WEB_SCREENSHOT_DIR) {
        await page.screenshot({ path: path.join(process.env.WEB_SCREENSHOT_DIR,
          `${file}-${viewport.width}-js-${javaScriptEnabled}.png`), fullPage: true });
      }
      assert.equal(await page.locator('html').getAttribute('lang'), 'en');
      assert.equal(await page.locator('.language-selector').isVisible(), javaScriptEnabled);
      if (file === 'index.html') {
        if (javaScriptEnabled) {
          await page.waitForFunction(() => {
            const track = document.querySelector('[data-picker-track]').getBoundingClientRect();
            const item = document.querySelector('[data-picker-item].is-active').getBoundingClientRect();
            return track.height > 0 && Math.abs(item.top + item.height / 2 - track.top - track.height / 2) < 2;
          });
          assert.equal(await page.locator('[data-picker-control]').getAttribute('aria-activedescendant'), 'picker-option-claude');
          await page.locator('[data-install-tab="windows"]').click();
          assert.match(await page.locator('[data-install-code]').innerText(), /PS>.*install\.ps1/);
          await page.locator('[data-install-tab="windows"]').press('ArrowLeft');
          assert.equal(await page.locator('[data-install-tab="unix"]').getAttribute('aria-selected'), 'true');
          const copy = page.locator('[data-install-command] [data-copy]');
          await copy.click();
          await page.waitForFunction(() => document.querySelector('[data-install-command] [data-copy]').dataset.state === 'copied');
          assert.equal(await page.evaluate(() => navigator.clipboard.readText()), await copy.getAttribute('data-copy'));
          await page.locator('[data-picker-control]').press('End');
          assert.equal(await page.locator('[data-picker-command-text]').innerText(), 'nanh fx');
          assert.equal(await page.locator('[data-picker-option][aria-selected="true"]').count(), 1);
        } else {
          assert.match(await page.locator('.install-fallback').innerText(), /install\.sh[\s\S]*install\.ps1/);
          assert.equal(await page.locator('.picker-fallback a').getAttribute('href'), 'docs.html#harnesses');
          assert.equal(await page.locator('button:visible, [role="listbox"]:visible').count(), 0);
        }
        await page.locator('.faq-row summary').first().click();
        assert.equal(await page.locator('.faq-row').first().getAttribute('open'), '');
        await page.locator('.hero-actions a[href="docs.html"]').click();
        assert.ok(page.url().endsWith('/docs.html'));
      } else if (file === 'docs.html') {
        await page.locator('.docs-sidebar a[href="#harnesses"]').click();
        assert.ok(page.url().endsWith('#harnesses'));
        assert.ok(await page.locator('#harnesses').isVisible());
        if (!javaScriptEnabled) assert.equal(await page.locator('button:visible').count(), 0);
      } else {
        await page.locator('a[href="logos/licenses/APACHE-2.0.txt"]').click();
        assert.match(await page.locator('body').innerText(), /Apache License/);
      }
    }
    assert.deepEqual(errors, []);
    console.log(`PASS pages/navigation/controls: ${viewport.width}px JS ${javaScriptEnabled ? 'on' : 'off'}`);
  } finally {
    await context.close();
  }
}

async function checkFailedScript(origin, viewport, script) {
  const context = await isolatedContext(origin, { viewport });
  await context.route(`**/${script}?*`, (route) => route.abort());
  try {
    const page = await context.newPage();
    await page.goto(origin);
    await checkStructure(page);
    assert.match(await page.locator('.install-fallback').innerText(), /install\.sh[\s\S]*install\.ps1/);
    assert.equal(await page.locator('button:visible, [role="listbox"]:visible').count(), 0);
    assert.equal(await page.locator('body').getAttribute('data-enhanced'), null);
    console.log(`PASS failed ${script}: ${viewport.width}px`);
  } finally {
    await context.close();
  }
}

async function checkLocaleAndTools(origin, viewport) {
  const context = await isolatedContext(origin, { viewport, locale: 'es-ES' });
  // Exercise optional API registration in a real DOM, without claiming native
  // WebMCP support in the installed browser.
  await context.addInitScript(() => {
    window.registeredTools = [];
    Object.defineProperty(document, 'modelContext', { value: {
      registerTool(tool) { window.registeredTools.push(tool); },
    } });
  });
  try {
    const page = await context.newPage();
    await page.goto(`${origin}/docs.html`);
    assert.equal(await page.locator('html').getAttribute('lang'), 'es');
    await checkStructure(page);
    assert.deepEqual(await page.evaluate(() => window.registeredTools.map((tool) => tool.name).sort()),
      ['open_documentation', 'search_documentation']);
    assert.ok(await page.evaluate(() => window.registeredTools[0].execute({ query: 'nanh config' }).results.length > 0));
    await Promise.all([page.waitForEvent('load'), page.locator('[data-locale="en"]').click()]);
    assert.equal(await page.locator('html').getAttribute('lang'), 'en');
    await page.reload();
    assert.equal(await page.locator('html').getAttribute('lang'), 'en', 'Saved locale overrides browser preference');
    assert.equal(await page.evaluate(() => window.registeredTools.length), 2);
    await checkStructure(page);
    console.log(`PASS preferred/saved locale and optional WebMCP: ${viewport.width}px`);
  } finally {
    await context.close();
  }
}

async function checkCachedHtml(origin) {
  const context = await isolatedContext(origin, { locale: 'en-US' });
  // Older shells load only the compatibility app bundle, with no locale files.
  const shell = fs.readFileSync('web/index.html', 'utf8')
    .replace(/\s*<script src="content-(?:en|es)\.js[^>]+><\/script>/g, '');
  await context.route(`${origin}/index.html`, (route) => route.fulfill({ contentType: 'text/html', body: shell }));
  try {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`${origin}/index.html`);
    await checkStructure(page);
    assert.equal(await page.locator('.install-fallback').count(), 0);
    await page.locator('[data-install-tab="windows"]').click();
    assert.match(await page.locator('[data-install-code]').innerText(), /install\.ps1/);
    assert.deepEqual(errors, []);
    console.log('PASS cached empty HTML with compatibility bundle');
  } finally {
    await context.close();
  }
}
