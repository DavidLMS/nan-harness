// Owned renderer observation and opt-in input. Publish only closed facts.
const fs = require('node:fs');
const { chromium } = require('../../.github/web-check/node_modules/playwright');
const qualify = process.argv[2] === '--qualify';
const drive = process.argv[2] === '--drive' || qualify;
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
    assistantTurnCount: 0, uniqueSendControl: false, canSend: false, sendBlocker: 'unmeasured', sendMechanism: 'semantic-keyboard', requestFailedCount: 0,
    requestFailureCategory: null, apiErrorStatus: null, apiErrorResponseCount: 0, errorCategory: 'unclassified',
    playwrightVersion: require('../../.github/web-check/node_modules/playwright/package.json').version,
    observedRuntimeVersion: null });
  if (qualify) Object.assign(facts, { mechanism: 'hermes-renderer-qualification', errorObserved: false, retryControl: false, retryHitOwned: false, retryHitTarget: 'unmeasured', retryRectInViewport: false, retryAncestorClipped: false,
    retryPointerEventsNone: false, retryHitTag: 'unmeasured', retryHitRegion: 'unmeasured' });
  saveFacts();
  const exactKeys = (value, keys) => value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
  const requestKeys = ['connectionPath', 'ownerPid', 'prompt', 'expectedMarker', 'timeoutMs'];
  if (qualify) requestKeys.push('action', 'purpose');
  const qualificationRequest = !qualify || (['submit', 'retry'].includes(request.action) &&
    ['response', 'failure'].includes(request.purpose) &&
    (request.purpose !== 'failure' || (request.action === 'submit' && request.expectedMarker === 'NAN_CHECK_EXPECTED_FAILURE')));
  if (!exactKeys(request, requestKeys) || !qualificationRequest ||
      !exactKeys(connection, ['schemaVersion', 'port', 'launcherPid']) || connection.schemaVersion !== 1 ||
      typeof request.connectionPath !== 'string' || request.connectionPath.length > 4096 || !Number.isInteger(connection.port) || connection.port < 1 || connection.port > 65535 ||
      !Number.isInteger(connection.launcherPid) || connection.launcherPid <= 1 ||
      !Number.isInteger(request.ownerPid) || request.ownerPid <= 1 ||
      !(qualify ? ['Check this connection', 'Read read-target.txt using your file tool.', 'Check the expected provider failure'].includes(request.prompt) : request.prompt === 'Check this connection') ||
      typeof request.expectedMarker !== 'string' || request.expectedMarker.length < (qualify && request.purpose === 'failure' ? 1 : 32) ||
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
  const errorCards = page.locator('[data-role="assistant"][data-slot="aui_assistant-message-root"] [role="alert"]:visible');
  const retryButton = errorCards.getByRole('button', { name: 'Retry', exact: true });
  async function errorProof() {
    facts.errorObserved = await errorCards.count() === 1;
    facts.retryControl = facts.errorObserved && await retryButton.count() === 1 && await retryButton.isEnabled();
    return facts.errorObserved && facts.retryControl;
  }
  // Frozen Hermes assistant-message.tsx MessagePrimitive.Root exposes data-role=assistant.
  const assistant = page.locator('[data-role="assistant"]:visible');
  if (await assistant.filter({ hasText: request.expectedMarker }).count() !== 0) {
    facts.errorCategory = 'stale-response'; saveFacts(); return;
  }
  if (qualify && request.purpose === 'failure' && await errorCards.count() !== 0) {
    facts.errorCategory = 'stale-response'; saveFacts(); return;
  }
  let candidates;
  let send;
  let retryUser;
  if (qualify && request.action === 'retry') {
    const retryComposer = page.locator('[data-slot="composer-root"] [role="textbox"]:visible');
    if (await retryComposer.count() !== 1) {
      facts.errorCategory = 'composer-ambiguous'; saveFacts(); return;
    }
    facts.uniqueComposer = true;
    retryUser = page.locator('[data-role="user"]:visible').filter({ hasText: request.prompt });
    if (await retryUser.count() !== 1 || !await retryUser.evaluate((e, prompt) => e.innerText.trim() === prompt, request.prompt)) {
      facts.errorCategory = 'input-mismatch'; saveFacts(); return;
    }
    if (!await errorProof()) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
    facts.userTurnObserved = true;
    facts.inputReadback = true;
    send = retryButton;
  } else {
  // Select only a single visible editable composer, never set application stores.
  while (Date.now() < deadline && !(await page.evaluate(() => { const fields = [...document.querySelectorAll('[data-slot="composer-root"] [role="textbox"][contenteditable="true"]:not([aria-disabled="true"])')]; return fields.filter(e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && !e.disabled && !e.readOnly; }).length === 1; })))
    await delay(100);
  candidates = page.locator('[data-slot="composer-root"] [role="textbox"][contenteditable="true"]:visible:not([aria-disabled="true"])');
  if (await candidates.count() !== 1 || !await candidates.isEditable()) {
    facts.errorCategory = 'composer-ambiguous'; saveFacts(); return;
  }
  facts.uniqueComposer = true;
  await candidates.fill(request.prompt, { timeout: Math.max(1, deadline - Date.now()) });
  facts.inputReadback = await candidates.evaluate((e, prompt) =>
    (e.value ?? e.textContent) === prompt, request.prompt);
  if (!facts.inputReadback || !ownedEndpoint()) {
    facts.errorCategory = 'input-mismatch'; saveFacts(); return;
  }
  // Frozen controls.tsx uses c.send; frozen English catalog names it Send.
  send = page.locator('[data-slot="composer-root"] button[type="submit"][aria-label="Send"]:visible');
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
  }
  const retryAction = qualify && request.action === 'retry';
  if (retryAction) facts.sendMechanism = 'pointer';
  const readiness = () => send.evaluate((button, pointerRetry) => {
    const visible = e => { const r = e.getBoundingClientRect();
      const style = getComputedStyle(e); return r.width > 0 && r.height > 0 &&
        style.visibility !== 'hidden' && style.display !== 'none'; };
    if ([...document.querySelectorAll('[aria-modal="true"],[role="alertdialog"]')].some(visible)) return 'modal';
    if ([...document.querySelectorAll('[role="menu"]')].some(visible)) return 'menu';
    if (button.disabled || button.getAttribute('aria-disabled') === 'true') return 'disabled';
    if (button.closest('[inert]')) return 'inert';
    if (!pointerRetry && button.tabIndex < 0) return 'focus';
    return null;
  }, retryAction, { timeout: Math.max(1, Math.min(1000, deadline - Date.now())) });
  facts.sendBlocker = await readiness();
  if (facts.sendBlocker !== null) {
    facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
  }
  async function retryHitTest() {
    try {
      const observation = await send.evaluate(button => {
        const rect = button.getBoundingClientRect();
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        const front = document.elementFromPoint(x, y);
        const observation = {
          retryRectInViewport: rect.width > 0 && rect.height > 0 && rect.left >= 0 && rect.top >= 0 &&
            rect.left + rect.width <= window.innerWidth && rect.top + rect.height <= window.innerHeight,
          retryAncestorClipped: false,
          retryPointerEventsNone: getComputedStyle(button).pointerEvents === 'none',
          retryHitTag: front ? ['html', 'body', 'button', 'div', 'span', 'svg'].includes(front.tagName?.toLowerCase())
            ? front.tagName.toLowerCase() : 'other' : 'none',
          retryHitRegion: 'none', retryHitOwned: false, retryHitTarget: 'none',
        };
        for (let parent = button.parentElement; parent; parent = parent.parentElement) {
          const bounds = parent.getBoundingClientRect();
          const style = getComputedStyle(parent);
          const clips = value => ['hidden', 'clip', 'scroll', 'auto'].includes(value);
          if (parent.matches('[data-sticky-prompt-clip]')) {
            const insetText = style.getPropertyValue('--sticky-prompt-clip');
            if (/^\d+(?:\.\d+)?px$/.test(insetText)) {
              const inset = Number.parseFloat(insetText);
              if (Number.isFinite(inset) && y < bounds.top + inset) observation.retryAncestorClipped = true;
            }
          }
          const paintClip = style.contain?.split(/\s+/).some(value => ['paint', 'strict', 'content'].includes(value));
          if (((clips(style.overflowX) || paintClip) && (x < bounds.left || x >= bounds.left + bounds.width)) ||
              ((clips(style.overflowY) || paintClip) && (y < bounds.top || y >= bounds.top + bounds.height)))
            observation.retryAncestorClipped = true;
        }
        if (!front) return observation;
        observation.retryHitRegion = front.closest('[data-slot="composer-drag-region"]') ? 'composer-drag-region'
          : front.closest('[data-slot="composer-dock"]') ? 'composer-dock'
          : front.closest('[role="dialog"],[role="alertdialog"]') ? 'dialog'
          : front.closest('[data-slot="popover-content"]') ? 'popover'
          : front.closest('[role="tooltip"]') ? 'tooltip'
          : getComputedStyle(front).getPropertyValue('-webkit-app-region') === 'drag' ? 'titlebar-drag'
          : front.closest('[data-slot="aui_thread-viewport"]') ? 'thread-viewport' : 'other';
        if (front === button || button.contains(front)) {
          observation.retryHitOwned = true; observation.retryHitTarget = 'self'; return observation;
        }
        observation.retryHitTarget = front.closest('[aria-modal="true"],[role="alertdialog"]') ? 'modal'
          : front.closest('[role="menu"]') ? 'menu'
          : front.closest('[data-slot="composer-root"]') || front.closest('[data-slot="composer-dock"]') ? 'composer'
          : front.closest('[role="alert"]')?.closest('[data-role="assistant"][data-slot="aui_assistant-message-root"]') ? 'error-card' : 'other';
        return observation;
      }, undefined, { timeout: 500 });
      Object.assign(facts, observation);
    } catch {
      facts.retryHitOwned = false; facts.retryHitTarget = 'unmeasured';
      facts.retryRectInViewport = false; facts.retryAncestorClipped = false;
      facts.retryPointerEventsNone = false; facts.retryHitTag = 'unmeasured'; facts.retryHitRegion = 'unmeasured';
    }
  }
  if (retryAction) {
    await send.evaluate(button => button.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' }),
      undefined, { timeout: Math.max(1, deadline - Date.now()) });
    // The frozen sticky-prompt clip reconciles on scroll/IO animation frames.
    // Observe that ordinary layout lifecycle before testing the click point.
    const frameBudget = Math.max(0, Math.min(500, deadline - Date.now()));
    let framesSettled = false;
    if (frameBudget > 0) {
      try {
        framesSettled = await Promise.race([
          page.evaluate(() => new Promise(resolve => {
            requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)));
          })),
          delay(frameBudget).then(() => false),
        ]);
      } catch { /* Context loss is unmeasured, never authorization to click. */ }
    }
    if (!framesSettled || !ownedEndpoint()) {
      facts.sendBlocker = 'unmeasured'; facts.retryHitOwned = false; facts.retryHitTarget = 'unmeasured';
      facts.errorCategory = 'submit-action-timeout'; saveFacts(); return;
    }
    await retryHitTest();
    saveFacts();
    if (!facts.retryHitOwned) {
      facts.sendBlocker = facts.retryHitTarget === 'composer' ? 'composer-drag-region'
        : ['modal', 'menu'].includes(facts.retryHitTarget) ? facts.retryHitTarget : 'other';
      facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
    }
  } else await send.scrollIntoViewIfNeeded({ timeout: Math.max(1, deadline - Date.now()) });
  if (!retryAction) await send.focus({ timeout: Math.max(1, deadline - Date.now()) });
  facts.sendBlocker = await readiness();
  if (!retryAction && facts.sendBlocker === null && !await send.evaluate(button => document.activeElement === button))
    facts.sendBlocker = 'focus';
  saveFacts();
  if (facts.sendBlocker !== null || !ownedEndpoint()) {
    facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
  }
  if (!(qualify && request.action === 'retry')) {
    facts.inputReadback = await candidates.evaluate((e, prompt) =>
      (e.value ?? e.textContent) === prompt, request.prompt);
    if (!facts.inputReadback) { facts.errorCategory = 'input-mismatch'; saveFacts(); return; }
  } else {
    if (await retryUser.count() !== 1 || !await retryUser.evaluate((e, prompt) => e.innerText.trim() === prompt, request.prompt)) {
      facts.errorCategory = 'input-mismatch'; saveFacts(); return;
    }
    if (!await errorProof()) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
  }
  try {
    // Frozen assistant-message.tsx uses a real button with Reload asChild.
    // Its ordinary click activates Reload without requiring retained keyboard
    // focus; Playwright still enforces pointer actionability, never force.
    if (retryAction) await send.click({ timeout: Math.max(1, deadline - Date.now()) });
    else await send.press('Enter', { timeout: Math.max(1, deadline - Date.now()) });
  } catch (error) {
    try { facts.sendBlocker = await readiness(); } catch { facts.sendBlocker = 'unmeasured'; }
    if (retryAction) await retryHitTest();
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
      if (qualify && request.purpose === 'failure') {
        if (await errorProof()) { facts.errorCategory = null; saveFacts(); return; }
        saveFacts(); await delay(100); continue;
      }
      // A submission may disable or replace the editor. Read a single snapshot
      // instead of waiting on a now-missing editable locator after submission.
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
  if (drive) { await driveDom(); process.exit((qualify && request.purpose === 'failure' ? facts.errorObserved && facts.retryControl : facts.responseVerified) ? 0 : 1); }
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
