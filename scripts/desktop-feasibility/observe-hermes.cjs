// Owned renderer observation and opt-in input. Publish only closed facts.
const fs = require('node:fs');
const { chromium } = require('../../.github/web-check/node_modules/playwright');
const drive = process.argv[2] === '--drive';
const request = drive ? JSON.parse(fs.readFileSync(process.argv[3], 'utf8')) : null;
const connection = drive ? JSON.parse(fs.readFileSync(request.connectionPath, 'utf8')) : null;
const [port, owner, output] = drive
  ? [String(connection.port), String(connection.launcherPid), process.argv[4]]
  : process.argv.slice(2);
const facts = { schemaVersion: 1, mechanism: 'hermes-cdp', endpointOwned: false,
  attached: false, uniqueComposer: false, inputReadback: false, syntheticTextPresent: false };
function saveFacts() {
  const temporary = `${output}.tmp`;
  fs.writeFileSync(temporary, JSON.stringify(facts) + '\n', { mode: 0o600 });
  fs.renameSync(temporary, output);
}
saveFacts();
function descendant(pid) {
  for (let depth = 0; depth < 32 && pid > 1; depth++) {
    if (String(pid) === owner) return true;
    const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
    pid = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
  }
  return false;
}
function ownedEndpoint() {
  // Associate the LISTEN socket inode with a child of the nanh launcher.
  const hexPort = Number(port).toString(16).toUpperCase().padStart(4, '0');
  const sockets = fs.readFileSync('/proc/net/tcp', 'utf8').trim().split('\n').slice(1)
    .map(line => line.trim().split(/\s+/))
    .filter(row => row[1] === `0100007F:${hexPort}` && row[3] === '0A');
  if (sockets.length !== 1) return false;
  const inode = `socket:[${sockets[0][9]}]`;
  for (const pid of fs.readdirSync('/proc').filter(x => /^\d+$/.test(x))) {
    try {
      if (!descendant(Number(pid))) continue;
      for (const fd of fs.readdirSync(`/proc/${pid}/fd`)) {
        if (fs.readlinkSync(`/proc/${pid}/fd/${fd}`) === inode) return true;
      }
    } catch { /* Processes can exit between enumeration and inspection. */ }
  }
  return false;
}
function delay(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }
async function driveDom() {
  Object.assign(facts, { mechanism: 'hermes-playwright-dom', targetVerified: false,
    responseVerified: false, inputSubmitted: false, inputCleared: false, userTurnObserved: false,
    assistantTurnCount: 0, uniqueSendControl: false, canSend: false, sendBlocker: 'unmeasured', requestFailedCount: 0,
    requestFailureCategory: null, apiErrorStatus: null, apiErrorResponseCount: 0, errorCategory: 'unclassified',
    playwrightVersion: require('../../.github/web-check/node_modules/playwright/package.json').version,
    observedRuntimeVersion: null });
  saveFacts();
  const exactKeys = (value, keys) => value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
  if (!exactKeys(request, ['connectionPath', 'ownerPid', 'prompt', 'expectedMarker', 'timeoutMs']) ||
      !exactKeys(connection, ['schemaVersion', 'port', 'launcherPid']) || connection.schemaVersion !== 1 ||
      typeof request.connectionPath !== 'string' || request.connectionPath.length > 4096 || !Number.isInteger(connection.port) || connection.port < 1 || connection.port > 65535 ||
      !Number.isInteger(connection.launcherPid) || connection.launcherPid <= 1 ||
      !Number.isInteger(request.ownerPid) || request.ownerPid <= 1 ||
      request.prompt !== 'Check this connection' ||
      typeof request.expectedMarker !== 'string' || request.expectedMarker.length < 32 ||
      request.expectedMarker.length > 2048 || !Number.isInteger(request.timeoutMs) ||
      request.timeoutMs < 1 || request.timeoutMs > 30000) {
    facts.errorCategory = 'invalid-request'; saveFacts(); return;
  }
  let ancestor = connection.launcherPid;
  let launcherOwned = false;
  for (let depth = 0; depth < 32 && ancestor > 1; depth++) {
    if (ancestor === request.ownerPid) { launcherOwned = true; break; }
    try { const stat = fs.readFileSync(`/proc/${ancestor}/stat`, 'utf8');
      ancestor = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
    } catch { break; }
  }
  if (!launcherOwned) { facts.errorCategory = 'launcher-unowned'; saveFacts(); return; }
  const deadline = Date.now() + request.timeoutMs;
  while (!ownedEndpoint() && Date.now() < deadline) await delay(100);
  if (!ownedEndpoint()) { facts.errorCategory = 'endpoint-unowned'; saveFacts(); return; }
  facts.endpointOwned = true;
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`,
    { timeout: Math.min(2000, request.timeoutMs), noDefaults: true });
  facts.attached = true;
  const version = browser.version();
  if (/^(?:Chrome\/)?[0-9]+(?:\.[0-9]+){1,3}$/.test(version)) facts.observedRuntimeVersion = version.replace(/^Chrome\//, '');
  let pages = [];
  while (Date.now() < deadline) {
    pages = browser.contexts().flatMap(context => context.pages())
    .filter(page => { try { const url = new URL(page.url());
      return url.protocol === 'file:' && /\/resources\/app\.asar(?:\.unpacked)?\/dist\/index\.html$/.test(decodeURIComponent(url.pathname));
    } catch { return false; } });
    if (pages.length > 0) break;
    if (!ownedEndpoint()) break;
    await delay(100);
  }
  if (pages.length !== 1 || !ownedEndpoint()) {
    facts.errorCategory = 'target-ambiguous'; saveFacts(); return;
  }
  const page = pages[0];
  page.on('requestfailed', request => {
    facts.requestFailedCount = Math.min(4096, facts.requestFailedCount + 1);
    const error = request.failure()?.errorText ?? '';
    facts.requestFailureCategory = /ABORTED/i.test(error) ? 'aborted'
      : /CERT|SSL|TLS/i.test(error) ? 'tls' : /CONNECTION|NAME_NOT_RESOLVED|INTERNET_DISCONNECTED/i.test(error) ? 'connection' : 'other';
  });
  page.on('response', response => {
    const status = response.status();
    if (!Number.isInteger(status) || status < 400 || status > 599) return;
    try {
      const url = new URL(response.url());
      if (!['http:', 'https:'].includes(url.protocol) ||
          !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) ||
          !url.pathname.startsWith('/api/')) return;
      facts.apiErrorStatus = status;
      facts.apiErrorResponseCount = Math.min(4096, facts.apiErrorResponseCount + 1);
    } catch { /* Invalid response URL never enters closed evidence. */ }
  });
  const session = await page.context().newCDPSession(page);
  const target = (await session.send('Target.getTargetInfo')).targetInfo;
  if (target.type !== 'page' || target.url !== page.url()) {
    facts.errorCategory = 'target-invalid'; saveFacts(); return;
  }
  facts.targetVerified = true;
  // Select only a single visible editable composer, never set application stores.
  while (Date.now() < deadline && !(await page.evaluate(() => { const fields = [...document.querySelectorAll('[data-slot="composer-root"] [role="textbox"][contenteditable="true"]:not([aria-disabled="true"])')]; return fields.filter(e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && !e.disabled && !e.readOnly; }).length === 1; })))
    await delay(100);
  const candidates = page.locator('[data-slot="composer-root"] [role="textbox"][contenteditable="true"]:visible:not([aria-disabled="true"])');
  if (await candidates.count() !== 1 || !await candidates.isEditable()) {
    facts.errorCategory = 'composer-ambiguous'; saveFacts(); return;
  }
  facts.uniqueComposer = true;
  // Frozen Hermes assistant-message.tsx MessagePrimitive.Root exposes data-role=assistant.
  const assistant = page.locator('[data-role="assistant"]:visible');
  if (await assistant.filter({ hasText: request.expectedMarker }).count() !== 0) {
    facts.errorCategory = 'stale-response'; saveFacts(); return;
  }
  await candidates.fill(request.prompt, { timeout: Math.max(1, deadline - Date.now()) });
  facts.inputReadback = await candidates.evaluate((e, prompt) =>
    (e.value ?? e.textContent) === prompt, request.prompt);
  if (!facts.inputReadback || !ownedEndpoint()) {
    facts.errorCategory = 'input-mismatch'; saveFacts(); return;
  }
  // Frozen controls.tsx uses c.send; frozen English catalog names it Send.
  const send = page.locator('[data-slot="composer-root"] button[type="submit"][aria-label="Send"]:visible');
  while (Date.now() < deadline) {
    facts.uniqueSendControl = await send.count() === 1;
    facts.canSend = facts.uniqueSendControl && await send.isEnabled();
    if (facts.canSend) break;
    await delay(100);
  }
  if (!facts.canSend || !ownedEndpoint()) {
    facts.errorCategory = 'send-unavailable'; saveFacts(); return;
  }
  facts.inputReadback = await candidates.evaluate((e, prompt) =>
    (e.value ?? e.textContent) === prompt, request.prompt);
  if (!facts.inputReadback) { facts.errorCategory = 'input-mismatch'; saveFacts(); return; }
  const hitTest = () => send.evaluate(button => {
    const visible = e => { const r = e.getBoundingClientRect();
      const style = getComputedStyle(e); return r.width > 0 && r.height > 0 &&
        style.visibility !== 'hidden' && style.display !== 'none'; };
    if ([...document.querySelectorAll('[aria-modal="true"],[role="alertdialog"]')].some(visible)) return { blocker: 'modal' };
    if ([...document.querySelectorAll('[role="menu"]')].some(visible)) return { blocker: 'menu' };
    const rect = button.getBoundingClientRect();
    const front = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    if (front && button.contains(front)) return { blocker: null };
    let blocker = 'other';
    if (front?.closest('[role="tooltip"]')) blocker = 'tooltip';
    else if (front?.closest('[data-slot="composer-drag-region"]')) blocker = 'composer-drag-region';
    const editor = button.closest('[data-slot="composer-root"]')?.querySelector('[role="textbox"][contenteditable="true"]');
    const editorRect = editor && visible(editor) ? editor.getBoundingClientRect() : null;
    return { blocker, move: blocker === 'tooltip' && editorRect
      ? { x: editorRect.x + editorRect.width / 2, y: editorRect.y + editorRect.height / 2 } : null };
  }, undefined, { timeout: Math.max(1, Math.min(1000, deadline - Date.now())) });
  let clearSamples = 0;
  let movedPointer = false;
  while (Date.now() < deadline && clearSamples < 3) {
    const hit = await hitTest();
    facts.sendBlocker = hit.blocker;
    saveFacts();
    if (hit.blocker === 'modal' || hit.blocker === 'menu') break;
    clearSamples = hit.blocker === null ? clearSamples + 1 : 0;
    if (hit.blocker === 'tooltip' && hit.move && !movedPointer) {
      // Frozen tooltip.tsx labels follow trigger hover; leave normally rather
      // than force a click through an overlay or bypass an intended modal.
      await page.mouse.move(hit.move.x, hit.move.y);
      movedPointer = true;
    }
    if (clearSamples < 3) await delay(100);
  }
  if (clearSamples !== 3 || !ownedEndpoint() || !await send.isEnabled()) {
    facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
  }
  facts.inputReadback = await candidates.evaluate((e, prompt) =>
    (e.value ?? e.textContent) === prompt, request.prompt);
  if (!facts.inputReadback) { facts.errorCategory = 'input-mismatch'; saveFacts(); return; }
  try {
    await send.click({ timeout: Math.max(1, deadline - Date.now()) });
  } catch (error) {
    try { facts.sendBlocker = (await hitTest()).blocker; } catch { facts.sendBlocker = 'unmeasured'; }
    const detail = String(error?.message ?? '');
    facts.errorCategory = /intercepts pointer events|subtree intercepts/i.test(detail) ? 'submit-action-intercepted'
      : /detached|not attached/i.test(detail) ? 'submit-action-detached'
      : /timeout/i.test(detail) ? 'submit-action-timeout' : 'submit-action-failed';
    saveFacts(); return;
  }
  facts.inputSubmitted = true; saveFacts();
  while (Date.now() < deadline) {
    if (!ownedEndpoint()) break;
    try {
      // A submission may disable or replace the editor. Read a single snapshot
      // instead of waiting on a now-missing editable locator after the click.
      const observation = await page.evaluate(({ prompt, marker }) => {
        const visible = e => { const r = e.getBoundingClientRect();
          const style = getComputedStyle(e); return r.width > 0 && r.height > 0 &&
            style.visibility !== 'hidden' && style.display !== 'none'; };
        const editors = [...document.querySelectorAll('[data-slot="composer-root"] [role="textbox"]')].filter(visible);
        const users = [...document.querySelectorAll('[data-role="user"]')].filter(visible);
        const assistants = [...document.querySelectorAll('[data-role="assistant"]')].filter(visible);
        return {
          inputCleared: editors.length === 1 && (editors[0].value ?? editors[0].textContent).trim() === '',
          userTurnObserved: users.filter(e => e.innerText.trim() === prompt).length === 1,
          assistantTurnCount: Math.min(4096, assistants.length),
          responseVerified: assistants.filter(e => e.innerText.includes(marker)).length === 1,
        };
      }, { prompt: request.prompt, marker: request.expectedMarker });
      facts.inputCleared ||= observation.inputCleared;
      facts.userTurnObserved ||= observation.userTurnObserved;
      facts.assistantTurnCount = observation.assistantTurnCount;
      if (observation.responseVerified) {
        facts.responseVerified = true; facts.syntheticTextPresent = true;
        facts.errorCategory = null; saveFacts(); return;
      }
    } catch (error) {
      if (!/execution context was destroyed|cannot find context with specified id/i.test(String(error?.message ?? ''))) {
        facts.errorCategory = 'response-observation-failed'; saveFacts(); return;
      }
      // Context replacement is a read-only retry; never submit the prompt again.
    }
    saveFacts();
    await delay(100);
  }
  facts.errorCategory = 'response-timeout'; saveFacts();
}
(async () => {
  if (drive) { await driveDom(); process.exit(facts.responseVerified ? 0 : 1); }
  const deadline = Date.now() + 120000;
  let browser;
  while (Date.now() < deadline) {
    try {
      if (!browser) {
        if (!ownedEndpoint()) { await delay(250); continue; }
        facts.endpointOwned = true;
        browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 2000, noDefaults: true });
        facts.attached = true;
      }
      for (const context of browser.contexts()) for (const page of context.pages()) {
        const observation = await page.evaluate(() => {
          const visible = e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
          const fields = [...document.querySelectorAll('textarea,[contenteditable="true"],input[type="text"]')].filter(visible);
          return { uniqueComposer: fields.length === 1,
            inputReadback: fields.length === 1 && (fields[0].value ?? fields[0].textContent) === 'Check this connection',
            syntheticTextPresent: [...document.querySelectorAll('[data-role="assistant"]')].some(e =>
              /NAN CHECK RESPONSE(?: (?:APPLE|BREAD|CHAIR|DREAM|EAGLE|FIELD|GREEN|HOUSE|ISLAND|JUICE|KITE|LEMON|MOON|NORTH|OCEAN|PAPER)){32}/.test(e.innerText)) };
        });
        for (const key of Object.keys(observation)) facts[key] ||= observation[key];
      }
      saveFacts(); await delay(100);
    } catch { await delay(250); }
    try { process.kill(Number(owner), 0); } catch { break; }
  }
  saveFacts(); process.exit(0);
})().catch(() => { if (drive) facts.errorCategory = 'attachment-or-action-failed'; saveFacts(); process.exit(1); });
