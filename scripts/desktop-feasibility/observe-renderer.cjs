// Read-only owned renderer inventory. No UI text, HTML, URLs or paths are emitted.
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(path.resolve(__dirname, '../../.github/web-check/node_modules/playwright'));
const request = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const connection = JSON.parse(fs.readFileSync(request.connectionPath, 'utf8'));
const output = process.argv[4];
const app = process.env.NANH_DESKTOP_RENDERER_APP;
const facts = { schemaVersion: 1, mechanism: 'renderer-inventory', diagnosticsOnly: true,
  app, endpointOwned: false, launcherOwned: false, attached: false, pageCount: 0,
  textareaCount: 0, editableCount: 0, sendCount: 0, retryCount: 0,
  newThreadCount: 0, loginCount: 0, dialogCount: 0, documentState: { readyState: 'unobserved', targetKind: 'unobserved', bodyPresent: false, elementCount: 0, visibleElementCount: 0, inputCount: 0, frameCount: 0, pageErrorCount: 0 }, errorCategory: 'unclassified' };
function save() {
  fs.writeFileSync(`${output}.tmp`, JSON.stringify(facts) + '\n', { mode: 0o600 });
  fs.renameSync(`${output}.tmp`, output);
}
async function run() {
  if (!['chatgpt-desktop', 'claude-desktop', 'pen-desktop'].includes(app)
      || !Number.isSafeInteger(request.ownerPid) || request.ownerPid <= 1
      || !Number.isSafeInteger(connection.launcherPid) || connection.launcherPid <= 1
      || !Number.isSafeInteger(connection.port) || connection.port <= 1024 || connection.port > 65535) {
    facts.errorCategory = 'invalid-request'; save(); return;
  }
  const rootProof = require('./endpoint-ownership.cjs').proof(String(request.ownerPid), String(connection.port));
  facts.launcherOwned = rootProof.descendant(connection.launcherPid);
  if (!facts.launcherOwned) { facts.errorCategory = 'launcher-unowned'; save(); return; }
  const ownership = require('./endpoint-ownership.cjs').proof(String(connection.launcherPid), String(connection.port));
  const deadline = Date.now() + 25000;
  while (Date.now() < deadline && !ownership.ownedEndpoint()) await new Promise(r => setTimeout(r, 250));
  facts.endpointOwned = ownership.ownedEndpoint();
  if (!facts.endpointOwned) { facts.errorCategory = 'endpoint-unowned'; save(); return; }
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${connection.port}`, { timeout: 2000, noDefaults: true });
  try {
    facts.attached = true;
    let pages = browser.contexts().flatMap(context => context.pages());
    // The debugger can listen before the application creates its first page.
    // Wait for that page, but never choose among multiple application targets.
    while (pages.length === 0 && Date.now() < deadline && ownership.ownedEndpoint()) {
      await new Promise(r => setTimeout(r, 250));
      pages = browser.contexts().flatMap(context => context.pages());
    }
    facts.pageCount = Math.min(4096, pages.length);
    if (pages.length !== 1) { facts.errorCategory = 'target-ambiguous'; save(); return; }
    const page = pages[0];
    let pageErrorCount = 0;
    page.on('pageerror', () => { pageErrorCount = Math.min(4096, pageErrorCount + 1); });
    const documentDeadline = Math.min(deadline, Date.now() + 10000);
    while (Date.now() < documentDeadline && ownership.ownedEndpoint()) {
      const loaded = await page.evaluate(() => document.readyState === 'complete'
        && document.body !== null && document.querySelectorAll('button,input,textarea,[contenteditable="true"]').length > 0);
      if (loaded) break;
      await new Promise(r => setTimeout(r, 250));
    }
    if (!ownership.ownedEndpoint()) { facts.endpointOwned = false; facts.errorCategory = 'endpoint-unowned'; save(); return; }
    const counts = await page.evaluate(() => {
      const visible = e => e.isConnected && e.getBoundingClientRect().width > 0
        && e.getBoundingClientRect().height > 0 && getComputedStyle(e).visibility === 'visible';
      const count = selector => Math.min(4096, [...document.querySelectorAll(selector)].filter(visible).length);
      const buttons = [...document.querySelectorAll('button,[role="button"]')].filter(visible);
      const named = pattern => Math.min(4096, buttons.filter(e => pattern.test(e.getAttribute('aria-label') || e.innerText || '')).length);
      return { textareaCount: count('textarea'), editableCount: count('[contenteditable="true"]'),
        sendCount: named(/^(send|send message|submit)$/i), retryCount: named(/^(retry|try again)$/i),
        newThreadCount: named(/^(new chat|new thread|new conversation)$/i),
        loginCount: named(/^(log in|sign in|continue with google|continue with apple)$/i),
        dialogCount: count('[role="dialog"],[role="alertdialog"]'),
        documentState: { readyState: document.readyState,
          targetKind: location.href === 'about:blank' ? 'blank'
            : ({ 'file:': 'file', 'http:': 'http', 'https:': 'https',
                 'chrome-error:': 'browser-error', 'app:': 'app' })[location.protocol] || 'other',
          bodyPresent: document.body !== null,
          elementCount: Math.min(4096, document.querySelectorAll('*').length),
          visibleElementCount: count('*'), inputCount: count('input'),
          frameCount: Math.min(4096, document.querySelectorAll('iframe,frame').length), pageErrorCount: 0 } };
    });
    counts.documentState.pageErrorCount = pageErrorCount;
    Object.assign(facts, counts, { errorCategory: null }); save();
  } finally { await browser.close(); }
}
run().catch(() => { facts.errorCategory = 'attachment-or-action-failed'; save(); process.exitCode = 1; });
