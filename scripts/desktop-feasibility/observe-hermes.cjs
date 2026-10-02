// Read-only button sampling. Coordinates remain private and never enter facts.
function sampleRetryInterior(button) {
function sourceFrontRegion(front) {
  if (!front) return 'none';
  const glass = front.closest('[data-glass-opaque]');
  if (glass?.classList.contains('z-(--z-onboarding)')) return 'onboarding';
  if (glass?.classList.contains('z-(--z-connecting)')) return 'gateway-connecting';
  let ancestor = front;
  for (let depth = 0; ancestor && depth < 16; depth++, ancestor = ancestor.parentElement) {
    if (ancestor.classList.contains('z-(--z-over-modal)') && ancestor.getAttribute('data-state') === 'open'
        && [...(ancestor.parentElement?.children ?? [])].some(sibling => sibling !== ancestor
          && sibling.matches('[role="dialog"][data-state="open"]')
          && sibling.querySelector('[data-slot="command"]'))) return 'command-backdrop';
  }
  const fixed = [
    ['[data-slot="dialog-overlay"]', 'dialog-overlay'],
    ['[data-narrow-overlay]', 'narrow-overlay'],
    ['[data-floating-pane]', 'floating-pane'],
    ['[data-pane-overlay]', 'pane-overlay'],
    ['[data-window-drag-handle]', 'window-drag-handle'],
    ['[data-panel-page-header]', 'panel-page-header'],
    ['[data-panel-header]', 'panel-header'],
    ['[data-zone-tabstrip]', 'zone-tabstrip'],
    ['[data-pane-host]', 'pane-host'],
    ['[data-tree-group]', 'tree-group'],
  ];
  return fixed.find(([selector]) => front.closest(selector))?.[1] ?? 'other';
}

  const doc = button.ownerDocument;
  const rect = button.getBoundingClientRect();
  const closed = { buttonTag: button.tagName === 'BUTTON' ? 'button' : 'other',
    ownerDocumentSame: doc === document, hitOwnedPoints: 0, clipped: false,
    rectInViewport: rect.left >= 0 && rect.top >= 0 && rect.right <= innerWidth && rect.bottom <= innerHeight,
    pointerEventsNone: getComputedStyle(button).pointerEvents === 'none' };
  closed.status = 'unmeasured';
  closed.frontTag = 'unmeasured'; closed.frontRegion = 'unmeasured';
  closed.hitAncestor = false; closed.sharesTurnPair = false; closed.containsComposer = false;
  const reject = status => { closed.status = status; return { closed, candidate: null }; };
  if (!button.isConnected) return reject('detached');
  if (doc !== document) return reject('foreign-document');
  if (button.tagName !== 'BUTTON' || button.type !== 'button') return reject('native-control-invalid');
  if (button.disabled || button.matches(':disabled') || button.getAttribute('aria-disabled') === 'true') return reject('disabled');
  if (button.closest('[inert]')) return reject('inert');
  if (rect.width <= 0 || rect.height <= 0 || !closed.rectInViewport) return reject('outside-viewport');
  if (typeof button.checkVisibility !== 'function' || !button.checkVisibility({ contentVisibilityAuto: true,
    opacityProperty: true, visibilityProperty: true })) return reject('hidden');
  let ancestor = button;
  for (let depth = 0; ancestor && depth < 64; depth++, ancestor = ancestor.parentElement) {
    const style = getComputedStyle(ancestor);
    if (style.pointerEvents === 'none') closed.pointerEventsNone = true;
    if (style.display === 'none' || ['hidden', 'collapse'].includes(style.visibility)
        || style.contentVisibility === 'hidden') return reject('hidden');
    if (style.pointerEvents === 'none') return reject('pointer-events-none');
    if ((style.clipPath !== 'none' && !/^inset\(0(?:px)?(?:\s+0(?:px)?){0,3}\)$/.test(style.clipPath)) || style.maskImage !== 'none') {
      closed.clipped = true; return reject('clipped');
    }
    if (ancestor !== button) {
      const a = ancestor.getBoundingClientRect();
      const clips = value => ['hidden', 'clip', 'scroll', 'auto'].includes(value);
      if ((clips(style.overflowX) && (rect.left < a.left || rect.right > a.right))
          || (clips(style.overflowY) && (rect.top < a.top || rect.bottom > a.bottom))) {
        closed.clipped = true; return reject('clipped');
      }
    }
  }
  if (ancestor) return reject('hidden');
  // Position for ordinary Playwright click is relative to the padding box.
  // A scale/rotation would invalidate this simple CSS-coordinate conversion.
  if (Math.abs(rect.width - button.offsetWidth) > 1 || Math.abs(rect.height - button.offsetHeight) > 1)
    return reject('transformed');
  let candidate = null;
  let representative;
  for (const [fx, fy] of [[.5,.5],[.25,.25],[.75,.25],[.25,.75],[.75,.75],[.5,.25],[.5,.75],[.25,.5],[.75,.5]]) {
    const x = button.clientWidth * fx;
    const y = button.clientHeight * fy;
    const hit = doc.elementFromPoint(rect.left + button.clientLeft + x, rect.top + button.clientTop + y);
    if (representative === undefined) representative = hit;
    if (hit === button || (hit && button.contains(hit))) {
      if (!candidate) representative = hit;
      closed.hitOwnedPoints++;
      candidate ??= { x, y, left: rect.left, top: rect.top, width: rect.width, height: rect.height };
    }
  }
  closed.status = candidate ? 'owned' : 'no-owned-point';
  const front = representative;
  const tag = front?.tagName?.toLowerCase();
  closed.frontTag = !front ? 'none' : ['html', 'body', 'button', 'div', 'span', 'svg'].includes(tag) ? tag : 'other';
  const sourceRegion = sourceFrontRegion(front);
  closed.frontRegion = sourceRegion !== 'other' && sourceRegion !== 'none' ? sourceRegion : !front ? 'none'
    : front.closest('[data-slot="composer-root"]') ? 'composer-root'
    : front.closest('[data-slot="composer-drag-region"]') ? 'composer-drag-region'
    : front.closest('[data-slot="composer-dock"]') ? 'composer-dock'
    : front.closest('[role="dialog"],[role="alertdialog"]') ? 'dialog'
    : front.closest('[data-slot="popover-content"]') ? 'popover'
    : front.closest('[role="tooltip"]') ? 'tooltip'
    : front.closest('[data-slot="aui_thread-viewport"]') ? 'thread-viewport'
    : front.closest('[data-slot="chat-drop-overlay"]') ? 'chat-drop-overlay'
    : front.closest('.particle-field') ? 'particle-field'
    : front.closest('[data-composer-owner]') ? 'composer-portal'
    : front.closest('[data-slot="composer-bounds"]') ? 'composer-bounds' : 'other';
  closed.hitAncestor = Boolean(front && front !== button && front.contains(button));
  const pair = button.closest('[data-slot="aui_turn-pair"]');
  closed.sharesTurnPair = Boolean(pair && front?.closest('[data-slot="aui_turn-pair"]') === pair);
  const composers = [...doc.querySelectorAll('[data-slot="composer-root"] [role="textbox"]')];
  closed.containsComposer = Boolean(front && composers.some(composer => front.contains(composer)));
  return { closed, candidate };
}
function stableCandidate(first, second) {
  if (!first.candidate || !second.candidate) return null;
  return ['x', 'y', 'left', 'top', 'width', 'height'].every(key => first.candidate[key] === second.candidate[key])
    ? { x: second.candidate.x, y: second.candidate.y } : null;
}
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
const { ownedEndpoint, descendant, parentPid, windowsProof } = require('./endpoint-ownership.cjs').proof(owner, port);
function delay(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }
async function driveDom() {
  Object.assign(facts, { mechanism: 'hermes-playwright-dom', targetVerified: false,
    responseVerified: false, inputSubmitted: false, inputCleared: false, userTurnObserved: false,
    assistantTurnCount: 0, uniqueSendControl: false, canSend: false, sendBlocker: 'unmeasured', sendMechanism: 'semantic-keyboard', requestFailedCount: 0,
    requestFailureCategory: null, apiErrorStatus: null, apiErrorResponseCount: 0, errorCategory: 'unclassified',
    playwrightVersion: require('../../.github/web-check/node_modules/playwright/package.json').version,
    observedRuntimeVersion: null });
  if (qualify) Object.assign(facts, { mechanism: 'hermes-renderer-qualification', errorObserved: false, retryControl: false, retryHitOwned: false, retryHitTarget: 'unmeasured', retryRectInViewport: false, retryAncestorClipped: false,
    retryPointerEventsNone: false, retryHitTag: 'unmeasured', retryHitRegion: 'unmeasured',
    retryFocusAfterAcquire: false, retryFocusBeforeAction: false, retryButtonConnected: false,
    retryAncestorHidden: false, retryAncestorInert: false, retryFieldsetDisabled: false,
    retryDocumentFocused: false, retryActiveTag: 'unmeasured', retryActiveRegion: 'unmeasured', retryHitOwnedPoints: 0, retryPointStable: false, retrySampleStatus: 'unmeasured', retryHitAncestor: false, retryHitSharesTurnPair: false, retryHitContainsComposer: false, retryReveal: 'none' });
  saveFacts();
  const exactKeys = (value, keys) => value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
  const requestKeys = ['connectionPath', 'ownerPid', 'prompt', 'expectedMarker', 'timeoutMs'];
  if (qualify) requestKeys.push('action', 'purpose');
  const qualificationRequest = !qualify || (['submit', 'retry', 'ready'].includes(request.action) &&
    ['response', 'failure'].includes(request.purpose) &&
    (request.action !== 'ready' || request.purpose === 'response') &&
    (request.purpose !== 'failure' || (request.action === 'submit' && request.expectedMarker === 'NAN_CHECK_EXPECTED_FAILURE')));
  const readinessBudget = qualify && request.action === 'ready' && process.platform === 'win32'
    && process.env.FEASIBILITY_HERMES_READINESS_POLICY === 'current-catalog' ? 120000 : (qualify ? 45000 : 30000);
  if (!exactKeys(request, requestKeys) || !qualificationRequest ||
      !exactKeys(connection, ['schemaVersion', 'port', 'launcherPid']) || connection.schemaVersion !== 1 ||
      typeof request.connectionPath !== 'string' || request.connectionPath.length > 4096 || !Number.isInteger(connection.port) || connection.port < 1 || connection.port > 65535 ||
      !Number.isInteger(connection.launcherPid) || connection.launcherPid <= 1 ||
      !Number.isInteger(request.ownerPid) || request.ownerPid <= 1 ||
      !(qualify ? ['Check this connection', 'Read read-target.txt using your file tool.', 'Check the expected provider failure'].includes(request.prompt) : request.prompt === 'Check this connection') ||
      typeof request.expectedMarker !== 'string' || request.expectedMarker.length < (qualify && request.purpose === 'failure' ? 1 : 32) ||
      request.expectedMarker.length > 2048 || !Number.isInteger(request.timeoutMs) ||
      request.timeoutMs < 1 || request.timeoutMs > readinessBudget) {
    facts.errorCategory = 'invalid-request'; saveFacts(); return;
  }
  let ancestor = connection.launcherPid;
  let launcherOwned = process.platform === 'win32'
    && windowsProof('descendant', connection.launcherPid, request.ownerPid);
  for (let depth = 0; depth < 32 && ancestor > 1; depth++) {
    if (process.platform === 'win32') break;
    if (ancestor === request.ownerPid) { launcherOwned = true; break; }
    try { ancestor = parentPid(ancestor); } catch { break; }
  }
  if (!launcherOwned) { facts.errorCategory = 'launcher-unowned'; saveFacts(); return; }
  const deadline = Date.now() + request.timeoutMs;
  while (!ownedEndpoint() && Date.now() < deadline) await delay(100);
  if (!ownedEndpoint()) { facts.errorCategory = 'endpoint-unowned'; saveFacts(); return; }
  facts.endpointOwned = true;
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`,
    { timeout: Math.max(1, Math.min(8000, deadline - Date.now())), noDefaults: true });
  facts.attached = true;
  const version = browser.version();
  if (/^(?:Chrome\/)?[0-9]+(?:\.[0-9]+){1,3}$/.test(version)) facts.observedRuntimeVersion = version.replace(/^Chrome\//, '');
  let pages = [];
  while (Date.now() < deadline) {
    pages = browser.contexts().flatMap(context => context.pages())
    .filter(page => { try { const url = new URL(page.url());
      return url.protocol === 'file:' && /\/(?:Contents\/Resources|resources)\/app\.asar(?:\.unpacked)?\/dist\/index\.html$/.test(decodeURIComponent(url.pathname));
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
  if (qualify && request.action === 'ready') {
    if (process.env.FEASIBILITY_HERMES_READINESS_POLICY === 'current-catalog') {
      const readiness = await require('./hermes-windows-ready.cjs').run(page, session, ownedEndpoint,
        deadline, process.env.FEASIBILITY_HERMES_CATALOG_PROFILE);
      require('node:fs').writeFileSync(output + '.ready.json', JSON.stringify(readiness), { mode: 0o600, flag: 'wx' });
      facts.errorCategory = readiness.stage === 'ready' ? null : 'attachment-or-action-failed';
    } else facts.errorCategory = null;
    saveFacts(); return;
  }
  const errorCards = page.locator('[data-role="assistant"][data-slot="aui_assistant-message-root"] [role="alert"]:visible');
  const retryButton = errorCards.getByRole('button', { name: 'Retry', exact: true });
  async function errorProof() {
    facts.errorObserved = await errorCards.count() === 1;
    if (facts.errorObserved && qualify) {
      // Frozen list.tsx groups one user and its following responses in a turn
      // pair. A delayed older error must never activate that older turn's Retry.
      facts.errorObserved = await errorCards.evaluate((alert, prompt) => {
        const assistant = alert.closest('[data-role="assistant"][data-slot="aui_assistant-message-root"]');
        const pair = assistant?.closest('[data-slot="aui_turn-pair"]');
        const group = pair?.closest('[data-slot="aui_message-group"]');
        if (!pair || !group || alert.closest('[data-slot="aui_turn-pair"]') !== pair) return false;
        const users = [...pair.querySelectorAll('[data-role="user"]')];
        return users.length === 1 && users[0].innerText.trim() === prompt
          && users[0].closest('[data-slot="aui_turn-pair"]') === pair;
      }, request.prompt);
    }
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
  if (!ownedEndpoint()) { facts.errorCategory = 'endpoint-unowned'; saveFacts(); return; }
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
  const readiness = () => send.evaluate((button, retryControlAction) => {
    const visible = e => { const r = e.getBoundingClientRect();
      const style = getComputedStyle(e); return r.width > 0 && r.height > 0 &&
        style.visibility !== 'hidden' && style.display !== 'none'; };
    if ([...document.querySelectorAll('[aria-modal="true"],[role="alertdialog"],[role="dialog"]')].some(visible)) return 'modal';
    if ([...document.querySelectorAll('[role="menu"]')].some(visible)) return 'menu';
    if (button.disabled || button.getAttribute('aria-disabled') === 'true') return 'disabled';
    if (button.closest('[inert]')) return 'inert';
    if (!retryControlAction && button.tabIndex < 0) return 'focus';
    return null;
  }, retryAction, { timeout: Math.max(1, Math.min(1000, deadline - Date.now())) });
  facts.sendBlocker = await readiness();
  if (facts.sendBlocker !== null && !(retryAction && facts.sendBlocker === 'modal')) {
    facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
  }
  await send.scrollIntoViewIfNeeded({ timeout: Math.max(1, deadline - Date.now()) });
  async function recordUnknownFront(handle) {
    try {
      const privateLevels = await handle.evaluate(button => {
        const rect = button.getBoundingClientRect();
        let front = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
        const levels = [];
        let remaining = 48;
        for (let level = 0; front && level < 4; level++, front = front.parentElement) {
          const tokens = [];
          for (const token of front.classList) {
            if ((/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(token)
                || /^z-(?:\(--[a-z0-9-]+\)|\[var\(--[a-z0-9-]+\)\]|\[[0-9]{1,5}\])$/.test(token)) && !tokens.includes(token)) {
              if (tokens.length >= 24 || remaining === 0) break;
              tokens.push(token); remaining--;
            }
          }
          levels.push({ level, tokens });
        }
        return levels;
      });
      const { createHash } = require('node:crypto');
      const levels = privateLevels.map(({ level, tokens }) => ({ level, tokenCount: tokens.length,
        tokenHashes: tokens.map(token => createHash('sha256').update(token, 'utf8').digest('hex')) }));
      privateLevels.length = 0;
      const payload = JSON.stringify({ schemaVersion: 1, mechanism: 'hermes-front-source', diagnosticsOnly: true, levels }) + '\n';
      if (Buffer.byteLength(payload, 'utf8') > 8192) return;
      const diagnostic = `${output}.front.json`;
      fs.writeFileSync(`${diagnostic}.tmp`, payload, { mode: 0o600 });
      fs.renameSync(`${diagnostic}.tmp`, diagnostic);
    } catch { /* Auxiliary observation never grants permission to submit. */ }
  }
  let retryHandle;
  let retryPosition;
  let secondCandidate;
  if (retryAction) {
    retryHandle = await send.elementHandle({ timeout: Math.max(1, deadline - Date.now()) });
    if (!retryHandle) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
    let first;
    let second;
    let revealAttempted = false;
    const settleDeadline = Math.min(deadline, Date.now() + 5000);
    const reprove = async () => ownedEndpoint() && await send.count() === 1 && await send.isEnabled()
      && await retryUser.count() === 1
      && await retryUser.evaluate((e, prompt) => e.innerText.trim() === prompt, request.prompt)
      && await errorProof() && await send.evaluate((button, sampled) => button === sampled, retryHandle);
    while (Date.now() < settleDeadline) {
      if (!await reprove()) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
      first = await retryHandle.evaluate(sampleRetryInterior);
      await delay(Math.min(100, Math.max(0, settleDeadline - Date.now())));
      second = await retryHandle.evaluate(sampleRetryInterior);
      retryPosition = stableCandidate(first, second);
      secondCandidate = second.candidate;
      if (retryPosition || second.closed.status !== 'no-owned-point') break;
      if (second.closed.frontRegion === 'onboarding' && !revealAttempted) {
        revealAttempted = true;
        const cover = page.locator('[data-glass-opaque][class~="z-(--z-onboarding)"]');
        const choice = cover.getByRole('button', { name: "I'll choose a provider later", exact: true });
        while (Date.now() < settleDeadline && await choice.count() === 0) {
          if (!await reprove()) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
          await delay(100);
        }
        if (!await reprove() || await cover.count() !== 1 || !await cover.isVisible()
            || await choice.count() !== 1 || !await choice.isEnabled()) {
          facts.errorCategory = 'send-unavailable'; saveFacts(); return;
        }
        const choiceHandle = await choice.elementHandle();
        const choiceFirst = await choiceHandle.evaluate(sampleRetryInterior);
        await delay(Math.min(100, Math.max(0, settleDeadline - Date.now())));
        const choiceSecond = await choiceHandle.evaluate(sampleRetryInterior);
        const choicePosition = stableCandidate(choiceFirst, choiceSecond);
        if (!choicePosition || !await reprove() || await cover.count() !== 1 || await choice.count() !== 1
            || !await choice.isEnabled() || !await choice.evaluate((button, sampled) => button === sampled, choiceHandle)
            || await readiness() !== null) {
          facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
        }
        const finalChoice = await choiceHandle.evaluate(sampleRetryInterior);
        if (!stableCandidate(choiceSecond, finalChoice) || !ownedEndpoint()) {
          facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
        }
        // Frozen ChooseLaterLink's ordinary click only dismisses first-run UI.
        try { await choiceHandle.click({ position: choicePosition, timeout: Math.max(1, settleDeadline - Date.now()) }); }
        catch { facts.errorCategory = 'submit-action-failed'; saveFacts(); return; }
        while (Date.now() < settleDeadline && await cover.count() !== 0) {
          if (!ownedEndpoint()) break;
          await delay(20);
        }
        if (await cover.count() !== 0 || !ownedEndpoint() || !await errorProof()
            || await retryUser.count() !== 1 || !await retryUser.evaluate((e, prompt) => e.innerText.trim() === prompt, request.prompt)
            || await send.count() !== 1 || !await send.isEnabled()) {
          facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
        }
        facts.retryReveal = 'onboarding-skipped';
        retryHandle = await send.elementHandle();
        if (!retryHandle) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
        continue;
      }
      if (second.closed.frontRegion === 'command-backdrop' && !revealAttempted) {
        revealAttempted = true;
        if (!await reprove()) { facts.errorCategory = 'send-unavailable'; saveFacts(); return; }
        // Exact source CommandPalette backdrop only; one ordinary dismissal.
        // Rejection or uncertain input stops here, never another action.
        const liveFront = await retryHandle.evaluate(sampleRetryInterior);
        if (liveFront.closed.status !== 'no-owned-point' || liveFront.closed.frontRegion !== 'command-backdrop') {
          facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
        }
        const dismissible = await page.evaluate(() => {
          const openCommands = [...document.querySelectorAll('[role="dialog"][data-state="open"]')]
            .filter(dialog => dialog.querySelector('[data-slot="command"]'));
          const visible = element => { const rect = element.getBoundingClientRect(); const style = getComputedStyle(element);
            return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden'; };
          return openCommands.length === 1 && visible(openCommands[0]) && ![...document.querySelectorAll('[aria-modal="true"],[role="alertdialog"],[role="dialog"],[role="menu"]')]
            .some(element => visible(element) && element !== openCommands[0]);
        });
        if (!dismissible) { facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return; }
        try { await page.keyboard.press('Escape'); }
        catch { facts.errorCategory = 'submit-action-failed'; saveFacts(); return; }
        let absent = false;
        while (Date.now() < Math.min(deadline, settleDeadline)) {
          absent = await page.evaluate(() => ![...document.querySelectorAll('[data-state]')].some(element =>
            element.classList.contains('z-(--z-over-modal)') && [...(element.parentElement?.children ?? [])]
              .some(sibling => sibling !== element && sibling.matches('[role="dialog"]') && sibling.querySelector('[data-slot="command"]'))));
          if (absent) break;
          await delay(20);
        }
        if (!absent || !await reprove()) { facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return; }
        facts.retryReveal = 'command-dismissed';
      }
    }
    if (!second) { facts.errorCategory = 'submit-action-timeout'; saveFacts(); return; }
    facts.retryHitOwnedPoints = second.closed.hitOwnedPoints;
    facts.retryPointStable = Boolean(retryPosition);
    facts.retryHitOwned = Boolean(retryPosition);
    facts.retryHitTarget = retryPosition ? 'self' : 'other';
    facts.retryHitTag = second.closed.frontTag;
    facts.retryHitRegion = second.closed.frontRegion;
    facts.retrySampleStatus = second.closed.status;
    facts.retryHitAncestor = second.closed.hitAncestor;
    facts.retryHitSharesTurnPair = second.closed.sharesTurnPair;
    facts.retryHitContainsComposer = second.closed.containsComposer;
    facts.retryAncestorClipped = second.closed.clipped;
    facts.retryRectInViewport = second.closed.rectInViewport;
    facts.retryPointerEventsNone = second.closed.pointerEventsNone;
    if (!retryPosition && second.closed.status === 'no-owned-point' && second.closed.frontRegion === 'other'
        && await reprove()) await recordUnknownFront(retryHandle);
    if (!retryPosition || !ownedEndpoint() || Date.now() >= deadline) {
      facts.sendBlocker = 'other'; facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
    }
  } else {
    await send.focus({ timeout: Math.max(1, deadline - Date.now()) });
    facts.sendBlocker = await readiness();
    if (facts.sendBlocker === null && !await send.evaluate(button => document.activeElement === button)) facts.sendBlocker = 'focus';
    if (facts.sendBlocker !== null || !ownedEndpoint()) {
      facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
    }
    facts.inputReadback = await candidates.evaluate((e, prompt) => (e.value ?? e.textContent) === prompt, request.prompt);
    if (!facts.inputReadback) { facts.errorCategory = 'input-mismatch'; saveFacts(); return; }
  }
  if (retryAction) {
    // Bind the final current semantic locator to the SAME sampled DOM element.
    if (!ownedEndpoint() || await send.count() !== 1 || !await send.isEnabled()
        || await retryUser.count() !== 1
        || !await retryUser.evaluate((e, prompt) => e.innerText.trim() === prompt, request.prompt)
        || !await errorProof() || !await send.evaluate((button, sampled) => button === sampled, retryHandle)) {
      facts.errorCategory = 'send-unavailable'; saveFacts(); return;
    }
    facts.sendBlocker = await readiness();
    const finalSample = await retryHandle.evaluate(sampleRetryInterior);
    facts.retrySampleStatus = finalSample.closed.status;
    facts.retryHitTag = finalSample.closed.frontTag; facts.retryHitRegion = finalSample.closed.frontRegion;
    facts.retryHitAncestor = finalSample.closed.hitAncestor; facts.retryHitSharesTurnPair = finalSample.closed.sharesTurnPair;
    facts.retryHitContainsComposer = finalSample.closed.containsComposer;
    facts.retryHitOwnedPoints = finalSample.closed.hitOwnedPoints;
    facts.retryRectInViewport = finalSample.closed.rectInViewport; facts.retryAncestorClipped = finalSample.closed.clipped;
    facts.retryPointerEventsNone = finalSample.closed.pointerEventsNone;
    const stillStable = stableCandidate({ candidate: { ...retryPosition, ...secondCandidate } }, finalSample);
    if (facts.sendBlocker !== null || !stillStable || !ownedEndpoint()) {
      facts.retryPointStable = false; facts.retryHitOwned = false;
      facts.errorCategory = 'submit-action-intercepted'; saveFacts(); return;
    }
  }
  try {
    // Real native Reload button; Playwright enforces ordinary actionability.
    // One stable owned interior position, never force or a keyboard fallback.
    if (retryAction) await retryHandle.click({ position: retryPosition, timeout: Math.max(1, deadline - Date.now()) });
    else await send.press('Enter', { timeout: Math.max(1, deadline - Date.now()) });
  } catch (error) {
    try { facts.sendBlocker = await readiness(); } catch { facts.sendBlocker = 'unmeasured'; }
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
      const observation = await page.evaluate(({ prompt, marker, bindTurn }) => {
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
          backendFailure: (() => {
            const text = assistants.map(e => e.innerText).join('\n');
            const categories = [
              ['python-import-failure', /ModuleNotFoundError|ImportError|No module named/],
              ['provider-unconfigured', /No inference provider configured|no provider configured|missing API key/i],
              ['backend-unavailable', /backend.*(?:unavailable|failed to start|not running)|gateway.*(?:not running|unavailable)/i],
              ['invalid-model', /model.*(?:not found|not configured|invalid)/i],
              ['permission-denied', /PermissionError|permission denied|EACCES/],
              ['connection-failed', /ConnectionError|connection refused|failed to connect/i],
            ].filter(([, pattern]) => pattern.test(text)).map(([category]) => category);
            return categories.length === 1 ? categories[0] : categories.length > 1 ? 'multiple' : 'unclassified';
          })(),
          responseVerified: assistants.filter(e => e.innerText.includes(marker) && (!bindTurn || (() => {
            const pair = e.closest('[data-slot="aui_turn-pair"]');
            if (!pair || !pair.closest('[data-slot="aui_message-group"]')
              || users.filter(user => user.innerText.trim() === prompt).length !== 1) return false;
            const pairUsers = [...pair.querySelectorAll('[data-role="user"]')];
            return pairUsers.length === 1 && pairUsers[0].innerText.trim() === prompt
              && pairUsers[0].closest('[data-slot="aui_turn-pair"]') === pair;
          })())).length === 1,
        };
      }, { prompt: request.prompt, marker: request.expectedMarker, bindTurn: qualify });
      facts.inputCleared ||= observation.inputCleared;
      facts.userTurnObserved ||= observation.userTurnObserved;
      facts.assistantTurnCount = observation.assistantTurnCount;
      if (qualify && facts.userTurnObserved && observation.backendFailure) {
        const failure = { schemaVersion: 1, mechanism: 'hermes-backend-failure', diagnosticsOnly: true,
          category: observation.backendFailure, assistantTurnCount: facts.assistantTurnCount };
        fs.writeFileSync(`${output}.backend.tmp`, JSON.stringify(failure) + '\n', { mode: 0o600 });
        fs.renameSync(`${output}.backend.tmp`, `${output}.backend.json`);
      }
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
  if (drive) { await driveDom(); process.exit((qualify && request.action === 'ready' ? facts.endpointOwned && facts.attached && facts.targetVerified : qualify && request.purpose === 'failure' ? facts.errorObserved && facts.retryControl : facts.responseVerified) ? 0 : 1); }
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
