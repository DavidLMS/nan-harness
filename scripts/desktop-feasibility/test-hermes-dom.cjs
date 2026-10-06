// Exercise closed failures without launching a browser or desktop application.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const ownershipSource = fs.readFileSync(`${__dirname}/endpoint-ownership.cjs`, 'utf8');
const ownershipFunctions = ownershipSource.slice(ownershipSource.indexOf('function saveWindowsProof('), ownershipSource.indexOf('return { ownedEndpoint'));
const source = fs.readFileSync(`${__dirname}/observe-hermes.cjs`, 'utf8').replace("const { ownedEndpoint, descendant, parentPid, windowsProof } = require('./endpoint-ownership.cjs').proof(owner, port);", ownershipFunctions).replace("require('./hermes-response-observation.cjs').observe", '(' + require('./hermes-response-observation.cjs').observe.toString() + ')').replace('await driveDom(); process.exit(', 'await driveDom(); return process.exit(');
async function trial(overrides, connectionOverrides = {}, scenario = null, qualify = false) {
  const output = new Map();
  const request = { connectionPath: '/connection', ownerPid: 20, prompt: 'Check this connection',
    expectedMarker: 'NAN CHECK RESPONSE ' + 'APPLE '.repeat(32), timeoutMs: 1, ...overrides };
  const connection = { schemaVersion: 1, port: 43210, launcherPid: 30, ...connectionOverrides };
  let attaches = 0;
  let submits = 0;
  let fills = 0;
  let clicks = 0;
  let keys = 0;
  let centers = 0;
  let frames = 0;
  let fixtureDocument;
  let sampleCount = 0;
  let retryHandles = 0, latestHandleSamples = 0;
  let escapes = 0;
  let skips = 0;
  let onboardingVisible = scenario?.startsWith('onboarding-') ?? false;
  let samplingChoice = false;
  let commandVisible = scenario?.startsWith('command-') ?? false;
  let focusChecks = 0;
  let focused = false;
  let focuses = 0;
  let settleTimerObserved = false;
  const focusButton = { tagName: 'BUTTON', tabIndex: scenario === 'retry-not-focusable' ? -1 : 0,
    isConnected: scenario !== 'retry-disconnected', parentElement: null,
    closest(selector) { return selector === '[inert]' && scenario === 'retry-ancestor-inert' ? {} : null; },
    matches(selector) { assert.equal(selector, ':disabled'); return scenario === 'retry-fieldset-disabled'; } };
  const reclaimedComposer = { tagName: 'DIV', privateText: 'PRIVATE_SYNTHETIC_VALUE',
    closest(selector) { return selector === '[data-slot="composer-root"]' ? {} : null; } };
  const turnPair = {};
  const ancestorFront = { tagName: 'DIV', privateValue: 'PRIVATE_SYNTHETIC_VALUE',
    contains(element) { return element === hitButton || scenario === 'retry-composer-cover'; },
    closest(selector) { return selector === '[data-slot="aui_turn-pair"]' ? turnPair : null; } };
  const overlay = { tagName: 'DIV', contains() { return false; }, closest(selector) { return ['[data-slot="composer-root"]', '[data-slot="composer-dock"]'].includes(selector) ? {} : null; } };
  const dockStrip = { tagName: 'DIV', privateClass: 'PRIVATE_SYNTHETIC_VALUE', closest(selector) { return selector === '[data-slot="composer-dock"]' ? {} : null; } };
  const foreignHit = { tagName: 'PRIVATE_SYNTHETIC_VALUE', contains() { return false; }, closest() { return null; } };
  const sourceRegionSelectors = { 'composer-bounds': '[data-slot="composer-bounds"]',
    'composer-portal': '[data-composer-owner]', 'particle-field': '.particle-field',
    'chat-drop-overlay': '[data-slot="chat-drop-overlay"]',
    'pane-overlay': '[data-pane-overlay]', 'pane-host': '[data-pane-host]', 'narrow-overlay': '[data-narrow-overlay]',
    'floating-pane': '[data-floating-pane]', 'tree-group': '[data-tree-group]', 'panel-header': '[data-panel-header]',
    'panel-page-header': '[data-panel-page-header]', 'zone-tabstrip': '[data-zone-tabstrip]',
    'window-drag-handle': '[data-window-drag-handle]', 'dialog-overlay': '[data-slot="dialog-overlay"]' };
  const sourceRegionHit = { tagName: 'DIV', contains: () => false, privateValue: 'PRIVATE_SYNTHETIC_VALUE',
    classList: { contains: name => scenario === 'gateway-connecting' && name === 'z-(--z-connecting)' },
    closest(selector) { return ['gateway-connecting', 'gateway-forged'].includes(scenario) && selector === '[data-glass-opaque]' ? this : selector === sourceRegionSelectors[scenario] ? {} : null; } };
  const bodyHit = { tagName: 'BODY', closest() { return null; } };
  const clippedParent = { matches() { return false; }, parentElement: null, getBoundingClientRect() { return { left: 0, top: 0, width: 5, height: 5 }; } };
  const sourceClipParent = { matches(selector) { return selector === '[data-sticky-prompt-clip]'; },
    parentElement: null, getBoundingClientRect() { return { left: 0, top: 0, width: 100, height: 100 }; } };
  const threadBackground = { tagName: 'DIV', closest(selector) { return selector === '[data-slot="aui_thread-viewport"]' ? {} : null; } };
  const hitChild = { tagName: 'SVG', privateLabel: 'PRIVATE_SYNTHETIC_VALUE', closest() { return null; } };
  const hitButton = {
    tagName: 'BUTTON', type: 'button', isConnected: true, disabled: false, offsetWidth: 20, offsetHeight: 20, clientWidth: 20, clientHeight: 20, clientLeft: 0, clientTop: 0,
    getAttribute() { return null; }, matches() { return false; }, checkVisibility(options) { assert.equal(options.contentVisibilityAuto, true); return scenario !== 'retry-hidden-render'; }, parentElement: scenario === 'retry-clipped' ? clippedParent : ['retry-source-clipped', 'retry-frame-settle'].includes(scenario) ? sourceClipParent : null,
    closest(selector) { return selector === '[data-slot="aui_turn-pair"]' ? turnPair : selector === '[data-slot="aui_thread-viewport"]' ? {} : null; },
    getBoundingClientRect() { const left = scenario === 'retry-offviewport' ? 200 : scenario === 'retry-unstable' && sampleCount > 1 ? 11 : 10; return { left, right: left + 20, top: 10, bottom: 30, width: 20, height: 20 }; },
    contains(element) { return element === hitChild; },
    scrollIntoView(options) {
      assert.equal(options.block, 'center');
      assert.equal(options.inline, 'nearest');
      assert.equal(options.behavior, 'instant');
      centers++;
    },
  };
  let pageEnumerations = 0;
  let observations = 0;
  let value = '';
  const rendererUrl = 'file:///synthetic/resources/app.asar.unpacked/dist/index.html';
  const composer = {
    async count() { return scenario === 'duplicate' ? 2 : 1; },
    async isEditable() { return true; },
    async fill(prompt) { fills++; value = scenario === 'mismatch' ? 'different synthetic input' : prompt; },
    async evaluate(callback, prompt) {
      if (submits > 0) throw new Error('Editable locator is disabled or detached after submission');
      return callback({ value }, prompt);
    },
    async press() { throw new Error('Do not submit through composer keyboard'); },
  };
  const send = {
    async count() { return scenario === 'send-duplicate' ? 2 : 1; },
    async isEnabled() { return scenario !== 'send-disabled'; },
    async evaluate(callback, pointerRetry) {
      if (callback.toString().includes('button === sampled')) return scenario !== 'retry-remounted' && !(scenario?.startsWith('onboarding-remount') && retryHandles===2 && latestHandleSamples>=2);
      if (callback.toString().includes('document.elementFromPoint')) return callback(hitButton);
      if (callback.toString().includes('button.scrollIntoView')) return callback(hitButton);
      if (scenario === 'retry-not-focusable' && !callback.toString().includes('document.activeElement')) {
        assert.equal(pointerRetry, true);
        return callback({ tabIndex: -1, disabled: false, getAttribute: () => null, closest: () => null }, pointerRetry);
      }
      if (callback.toString().includes('document.activeElement')) { focusChecks++; return callback(focusButton); }
      return scenario === 'blocked-modal' ? 'modal' : scenario === 'blocked-menu' ? 'menu'
        : scenario === 'inert' ? 'inert' : null;
    },
    async elementHandle() { retryHandles++; latestHandleSamples=0; const generation = retryHandles; let handleSamples = 0; return {
      async evaluate(callback) {
        if (callback.toString().includes('old.ownerDocument')) return scenario !== 'onboarding-remount-document';
        handleSamples++; latestHandleSamples=handleSamples; samplingChoice = false; sampleCount++; hitButton.isConnected = scenario !== 'retry-detached' && !(scenario === 'command-detached' && escapes > 0); if (scenario?.startsWith('onboarding-remount') && skips > 0 && generation >= 2 && handleSamples >= 3
          && (generation === 2 || scenario === 'onboarding-remount-again')) hitButton.isConnected = false;
        hitButton.disabled = scenario === 'retry-disabled'; hitButton.offsetWidth = scenario === 'retry-transformed' ? 40 : 20; hitButton.type = scenario === 'retry-wrong-type' ? 'submit' : 'button'; hitButton.ownerDocument = scenario === 'retry-foreign-document' ? {} : fixtureDocument; return callback(hitButton); },
      async click(options) { return send.click(options); },
    }; },
    async scrollIntoViewIfNeeded() {},
    async focus() {
      assert(!(qualify && request.action === 'retry'));
      focuses++; focused = true;
    },
    async click(options) {
      assert(qualify && request.action === 'retry');
      assert.equal(options.force, undefined);
      assert(options.timeout > 0);
      assert(options.position.x > 0 && options.position.x < 20);
      assert(options.position.y > 0 && options.position.y < 20);
      clicks++;
      if (scenario === 'retry-intercepted') throw new Error('subtree intercepts pointer events PRIVATE_SYNTHETIC_VALUE');
      submits++; value = '';
    },
    async press(key) {
      assert.equal(key, 'Enter');
      keys++;
      assert(focused);
      if (scenario === 'click-timeout') throw new Error('Timeout 100ms exceeded PRIVATE_SYNTHETIC_VALUE');
      submits++; value = '';
    },
  };
  const errorCards = {
    async evaluate(callback, prompt) {
      const user = { innerText: (['foreign-error-turn', 'delayed-foreign-error'].includes(scenario) || scenario === 'foreign-card-after-settle' && frames === 2 || scenario === 'foreign-card-during-wait' && sampleCount >= 2 || scenario === 'onboarding-foreign-pair' && skips > 0 || scenario === 'onboarding-remount-foreign-pair' && retryHandles >= 3) ? 'Older unrelated user turn' : request.prompt,
        closest() { return pair; } };
      const pair = { querySelectorAll(selector) { assert.equal(selector, '[data-role="user"]'); return [user]; },
        closest() { return {}; } };
      const assistantRoot = { closest() { return pair; } };
      return callback({ closest(selector) { return selector.includes('assistant-message-root') ? assistantRoot : pair; } }, prompt);
    },
    async count() { return ['failure-appears', 'delayed-foreign-error'].includes(scenario) && submits === 0 ? 0 : scenario === 'missing-error' ? 0 : scenario === 'duplicate-error' ? 2 : 1; },
    getByRole(role, options) { assert.equal(role, 'button'); assert.equal(options.name, 'Retry'); return { ...send, async count() { return scenario === 'missing-retry' ? 0 : scenario === 'duplicate-retry' ? 2 : 1; } }; },
  };
  const users = { filter({ hasText }) { assert.equal(hasText, request.prompt); return {
    async count() { return scenario === 'foreign-user' ? 0 : 1; },
    async evaluate(callback, prompt) { return callback({ innerText: request.prompt }, prompt); },
  }; } };
  const assistant = {
    async count() { return submits === 1 ? 1 : 0; },
    filter({ hasText }) {
      assert.equal(hasText, request.expectedMarker);
      return {
        async count() { return scenario === 'stale' || submits === 1 ? 1 : 0; },
        async evaluate(callback, marker) { return callback({ innerText: request.expectedMarker }, marker); },
      };
    },
  };
  const rect = () => ({ width: 20, height: 20 });
  const responseUser = { innerText: request.prompt, getBoundingClientRect: rect, closest() { return responsePair; } };
  const foreignResponseUser = { ...responseUser, innerText: 'Older unrelated prompt' };
  const responsePair = { closest() { return {}; }, querySelectorAll() {
    return [scenario === 'foreign-response-turn' ? foreignResponseUser : responseUser];
  } };
  const responseAssistant = { innerText: request.expectedMarker, getBoundingClientRect: rect, closest() { return responsePair; } };
  const onboardingRoot = { tagName: 'DIV', contains: () => false, parentElement: null,
    classList: { contains: name => name === 'z-(--z-onboarding)' && scenario !== 'onboarding-forged' },
    closest(selector) { return selector === '[data-glass-opaque]' ? this : null; }, getAttribute: () => null };
  const choiceButton = { ...hitButton, parentElement: null, contains: () => false,
    closest(selector) { return selector === '[data-glass-opaque]' ? onboardingRoot : null; } };
  const choiceHandle = {
    async evaluate(callback) { samplingChoice = true; choiceButton.ownerDocument = fixtureDocument; return callback(choiceButton); },
    async click(options) { assert.equal(options.force, undefined); assert(options.position.x > 0); skips++;
      if (scenario === 'onboarding-uncertain') throw new Error('PRIVATE_SYNTHETIC_VALUE');
      if (scenario !== 'onboarding-remaining') onboardingVisible = false;
    },
  };
  const choice = { async count() { return scenario === 'onboarding-missing' ? 0 : scenario === 'onboarding-duplicate' ? 2 : 1; },
    async isEnabled() { return scenario !== 'onboarding-disabled'; }, async elementHandle() { return choiceHandle; },
    async evaluate(callback) { return callback(choiceButton, choiceButton); } };
  const onboardingCover = { async count() { return onboardingVisible ? 1 : 0; }, async isVisible() { return onboardingVisible; },
    getByRole(role, options) { assert.equal(role, 'button'); assert.equal(options.name, "I'll choose a provider later"); return choice; } };
  const commandContent = { matches: selector => selector.includes('[role="dialog"]'), querySelector: () => scenario === 'command-missing-content' ? null : ({}),
    getBoundingClientRect: rect };
  const commandBackdrop = { tagName: 'DIV', closest: () => null, contains: () => false,
    classList: { contains: name => name === 'z-(--z-over-modal)' && scenario !== 'command-forged-class' }, getAttribute: () => 'open', parentElement: null };
  commandBackdrop.parentElement = { children: [commandBackdrop, commandContent], parentElement: null,
    classList: { contains: () => false }, getAttribute: () => null };
  for (const element of [hitButton, overlay, foreignHit, ancestorFront, bodyHit, hitChild, dockStrip, sourceRegionHit, threadBackground]) {
    element.classList ??= Object.assign(['z-(--z-over-modal)', 'z-[var(--z-layer)]', 'z-[123]', 'z-[url(private)]', 'PRIVATE_SYNTHETIC_VALUE', ...Array.from({length:80}, (_,i)=>'static_token_'+i), '[bad:token]'], { contains: () => false }); element.getAttribute ??= () => null;
  }
  if (scenario === 'retry-all-covered') {
    let parent = foreignHit;
    for (let depth = 0; depth < 6; depth++) {
      parent.parentElement = { tagName: 'DIV', parentElement: null, getAttribute: () => null,
        classList: Object.assign(Array.from({ length: 40 }, (_, i) => 'ancestor_token_' + depth + '_' + i), { contains: () => false }) };
      parent = parent.parentElement;
    }
  }
  const page = {
    keyboard: { async press(key) { assert.equal(key, 'Escape'); escapes++;
      if (scenario === 'command-escape-failed') throw new Error('PRIVATE_SYNTHETIC_VALUE');
      if (scenario !== 'command-remaining') commandVisible = false;
    } },
    url() { return rendererUrl; },
    on(event, callback) {
      assert(['requestfailed', 'response'].includes(event));
      if (event === 'response') {
        callback({ status: () => 401, url: () => 'https://foreign.invalid/api/private' });
        callback({ status: () => 500, url: () => 'http://127.0.0.1:7777/not-api/private' });
        callback({ status: () => 200, url: () => 'http://localhost:7777/api/synthetic' });
        callback({ status: () => 503, url: () => 'http://127.0.0.1:7777/api/synthetic' });
      }
    },
    context() { return { async newCDPSession() { return { async send(method) {
      assert.equal(method, 'Target.getTargetInfo');
      return { targetInfo: { type: 'page', url: rendererUrl } };
    } }; } }; },
    async evaluate(_callback, args) {
      if (_callback.toString().includes('requestAnimationFrame')) return _callback();
      if (!args) return _callback.toString().includes('classList.contains') || _callback.toString().includes('openCommands') ? _callback() : true;
      assert.equal(args.prompt, request.prompt);
      assert.equal(args.marker, request.expectedMarker);
      observations++;
      if (scenario === 'transient-context' && observations === 1) throw new Error('Execution context was destroyed');
      if (qualify) return _callback(args);
      return { inputCleared: scenario !== 'missing-editor', userTurnObserved: submits === 1,
        assistantTurnCount: submits, responseVerified: submits === 1 };
    },
    locator(selector) {
      if (selector === '[data-glass-opaque][class~="z-(--z-onboarding)"]') return onboardingCover;
      if (selector === '[data-role="assistant"][data-slot="aui_assistant-message-root"] [role="alert"]:visible') return errorCards;
      if (selector === '[data-slot="composer-root"] [role="textbox"]:visible') return composer;
      if (selector === '[data-role="assistant"]:visible') return assistant;
      if (selector === '[data-role="user"]:visible') return users;
      if (selector === '[data-slot="composer-root"] button[type="submit"][aria-label="Send"]:visible') return send;
      assert.equal(selector, '[data-slot="composer-root"] [role="textbox"][contenteditable="true"]:visible:not([aria-disabled="true"])');
      return composer;
    },
  };
  const browser = { version() { return '140.0.7339.80'; },
    contexts() { return [{ pages() { pageEnumerations++;
      return scenario === 'delayed' && pageEnumerations === 1 ? [] : [page]; } }]; } };
  const mockFs = {
    readFileSync(path) {
      if (path === '/request') return JSON.stringify(request);
      if (path === '/connection') return JSON.stringify(connection);
      if (path === '/proc/30/stat') return '30 (synthetic) S 20';
      if (path === '/proc/net/tcp' && scenario === 'owner-after-settle' && frames === 2) return 'header\n';
      if (path === '/proc/net/tcp') return scenario
        ? `header\n0: 0100007F:${connection.port.toString(16).toUpperCase()} 00000000:0000 0A 0 0 0 0 0 777\n`
        : 'header\n';
      throw new Error('synthetic missing process');
    },
    readdirSync(path) { return path === '/proc' ? ['30'] : ['5']; },
    readlinkSync() { return 'socket:[777]'; },
    writeFileSync(path, value) { if (scenario === 'aux-write-failed' && path.includes('.front.json')) throw new Error('PRIVATE_SYNTHETIC_VALUE'); output.set(path, value); },
    renameSync(from, to) { output.set(to, output.get(from)); },
  };
  await new Promise((resolve, reject) => {
    vm.runInNewContext(source, {
      require(name) {
        if (name === 'node:fs') return mockFs;
        if (name === 'node:crypto') return require('node:crypto');
        if (name.endsWith('/package.json')) return { version: '1.61.1' };
        return { chromium: { async connectOverCDP() { attaches++;
          if (!scenario) throw new Error('must not attach'); return browser; } } };
      },
      URL, Buffer, setTimeout: (callback, delayMs) => { if (delayMs === 0) settleTimerObserved = true; return setTimeout(callback, delayMs); }, requestAnimationFrame: callback => {
        if (scenario !== 'retry-no-frames') setTimeout(() => { frames++; callback(); }, 0);
      }, window: { innerWidth: 100, innerHeight: 100 }, innerWidth: 100, innerHeight: 100,
      getComputedStyle: element => ({ display: scenario === 'retry-ancestor-hidden' ? 'none' : 'block', visibility: 'visible', contentVisibility: 'visible', clipPath: scenario === 'retry-source-clipped' ? 'inset(25px 0px 0px)' : 'inset(0px 0px 0px 0px)', maskImage: 'none', pointerEvents: scenario === 'retry-pointer-none' && element === hitButton ? 'none' : 'auto',
        overflowX: element === clippedParent ? 'hidden' : 'visible', overflowY: 'visible', contain: '',
        getPropertyValue: property => property === '--sticky-prompt-clip' && element === sourceClipParent ? scenario === 'retry-frame-settle' && frames >= 2 ? '0px' : '25px'
          : property === '-webkit-app-region' && scenario === 'retry-native-drag' ? 'drag' : '' }),
      document: fixtureDocument = { hasFocus() { return true; }, get activeElement() { if (scenario === 'retry-focus-reclaimed') return reclaimedComposer; return focused && scenario !== 'focus-failed' && !(scenario === 'retry-focus-lost' && focusChecks > 1) ? focusButton : null; }, querySelectorAll: selector => {
        if (selector === '[data-state]') return commandVisible ? [commandBackdrop] : [];
        if (selector === '[role="dialog"][data-state="open"]') return commandVisible ? [commandContent] : [];
        if (!submits) return selector.includes('composer-root') ? [{ value: '', getBoundingClientRect: rect }] : [];
        if (selector === '[data-role="user"]') return [responseUser];
        if (selector === '[data-role="assistant"]') return [responseAssistant];
        if (selector.includes('composer-root')) return scenario === 'missing-editor' ? [] : [{ value: '', getBoundingClientRect: rect }];
        return [];
      }, elementFromPoint: (x, y) =>
        samplingChoice ? choiceButton : onboardingVisible ? onboardingRoot : commandVisible ? commandBackdrop : scenario === 'retry-center-covered' && x === 20 && y === 20 || scenario === 'retry-overlay' || (scenario === 'retry-centering' && centers === 0)
          || (scenario === 'retry-intercepted' && clicks > 0) ? overlay : scenario === 'retry-fading-overlay' && sampleCount <= 2 ? foreignHit : ['retry-ancestor-cover', 'retry-composer-cover'].includes(scenario) ? ancestorFront : (['retry-all-covered', 'aux-write-failed'].includes(scenario) || scenario === 'retry-covered-at-final' && sampleCount >= 3) ? foreignHit : (sourceRegionSelectors[scenario] || ['gateway-connecting', 'gateway-forged'].includes(scenario)) ? sourceRegionHit : scenario === 'retry-frame-settle' && frames < 2 ? threadBackground : scenario === 'retry-source-clipped' ? threadBackground : scenario === 'retry-offviewport' ? null : scenario === 'retry-private-hit' ? foreignHit : scenario === 'retry-body-hit' ? bodyHit : scenario === 'retry-dock-strip' ? dockStrip : scenario === 'retry-child-hit' ? hitChild : hitButton },
      process: { argv: ['node', 'helper', qualify ? '--qualify' : '--drive', '/request', '/output'], exit: resolve },
    });
    setTimeout(() => reject(new Error('bounded helper fixture timed out')), 1000).unref();
  });
  assert.equal(attaches, scenario ? 1 : 0);
  const facts = JSON.parse(output.get('/output'));
  if (qualify) {
    assert(!output.get('/output').includes(request.expectedMarker));
    return { facts, submits, fills, clicks, keys, centers, frames, focuses, escapes, skips, retryHandles, auxiliary: output.get('/output.front.json'), backend: output.get('/output.backend.json') };
  }
  assert.equal(facts.inputSubmitted, ['happy', 'delayed', 'missing-editor', 'transient-context', 'keyboard'].includes(scenario));
  assert.equal(facts.responseVerified, ['happy', 'delayed', 'missing-editor', 'transient-context', 'keyboard'].includes(scenario));
  assert.equal(facts.attached, Boolean(scenario));
  assert.equal(submits, ['happy', 'delayed', 'missing-editor', 'transient-context', 'keyboard'].includes(scenario) ? 1 : 0);
  if (scenario === 'stale' || scenario === 'duplicate') assert.equal(fills, 0);
  if (['mismatch', 'happy', 'delayed', 'missing-editor', 'transient-context', 'keyboard', 'click-timeout', 'blocked-modal', 'blocked-menu', 'inert', 'focus-failed', 'send-disabled', 'send-duplicate'].includes(scenario)) assert.equal(fills, 1);
  if (scenario) {
    assert.equal(facts.endpointOwned, true);
    assert.equal(facts.targetVerified, true);
    assert.equal(facts.inputReadback, ['happy', 'delayed', 'missing-editor', 'transient-context', 'keyboard', 'click-timeout', 'blocked-modal', 'blocked-menu', 'inert', 'focus-failed', 'send-disabled', 'send-duplicate'].includes(scenario));
  }
  assert(!output.get('/output').includes(request.expectedMarker));
  assert(!output.get('/output').includes('PRIVATE_SYNTHETIC_VALUE'));
  return facts;
}
(async () => {
  assert.equal((await trial({ prompt: 'PRIVATE_SYNTHETIC_PROMPT' })).errorCategory, 'invalid-request');
  assert.equal((await trial({ ownerPid: 99 })).errorCategory, 'launcher-unowned');
  assert.equal((await trial({})).errorCategory, 'endpoint-unowned');
  assert.equal((await trial({}, { port: 0 })).errorCategory, 'invalid-request');
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'mismatch')).errorCategory, 'input-mismatch');
  const readiness = { ownerPid: 99, action: 'ready', purpose: 'response' };
  assert.equal((await trial({ ...readiness, timeoutMs: 45000 }, {}, null, true)).facts.errorCategory, 'launcher-unowned');
  assert.equal((await trial({ ...readiness, timeoutMs: 45001 }, {}, null, true)).facts.errorCategory, 'invalid-request');
  assert.equal((await trial({ ownerPid: 99, timeoutMs: 30001 })).errorCategory, 'invalid-request');
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'stale')).errorCategory, 'stale-response');
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'duplicate')).errorCategory, 'composer-ambiguous');
  const happy = await trial({ timeoutMs: 500 }, {}, 'happy');
  assert.equal(happy.errorCategory, null);
  assert.equal(happy.uniqueComposer, true);
  assert.equal(happy.syntheticTextPresent, true);
  assert.equal(happy.uniqueSendControl, true);
  assert.equal(happy.canSend, true);
  assert.equal(happy.inputCleared, true);
  assert.equal(happy.userTurnObserved, true);
  assert.equal(happy.assistantTurnCount, 1);
  assert.equal(happy.apiErrorResponseCount, 1);
  assert.equal(happy.apiErrorStatus, 503);
  assert.equal(happy.requestFailedCount, 0);
  assert.equal((await trial({ timeoutMs: 100 }, {}, 'send-disabled')).errorCategory, 'send-unavailable');
  assert.equal((await trial({ timeoutMs: 100 }, {}, 'send-duplicate')).errorCategory, 'send-unavailable');
  const delayed = await trial({ timeoutMs: 500 }, {}, 'delayed');
  assert.equal(delayed.responseVerified, true);
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'click-timeout')).errorCategory, 'submit-action-timeout');
  const missing = await trial({ timeoutMs: 500 }, {}, 'missing-editor');
  assert.equal(missing.inputCleared, false);
  assert.equal(missing.responseVerified, true);
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'transient-context')).responseVerified, true);
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'keyboard')).responseVerified, true);
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'blocked-modal')).sendBlocker, 'modal');
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'focus-failed')).sendBlocker, 'focus');
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'blocked-menu')).sendBlocker, 'menu');
  assert.equal((await trial({ timeoutMs: 500 }, {}, 'inert')).sendBlocker, 'inert');
  assert.equal(happy.sendMechanism, 'semantic-keyboard');
  const phase = { timeoutMs: 500, action: 'submit', purpose: 'failure',
    prompt: 'Check the expected provider failure', expectedMarker: 'NAN_CHECK_EXPECTED_FAILURE' };
  const failure = await trial(phase, {}, 'failure-appears', true);
  assert.equal(failure.facts.errorObserved, true);
  assert.equal(failure.facts.retryControl, true);
  assert.equal(failure.facts.responseVerified, false);
  assert.equal(failure.submits, 1);
  const staleFailure = await trial(phase, {}, 'happy', true);
  assert.equal(staleFailure.facts.errorCategory, 'stale-response');
  assert.equal(staleFailure.submits, 0);
  const tool = await trial({ timeoutMs: 500, action: 'submit', purpose: 'response', prompt: 'Read read-target.txt using your file tool.' }, {}, 'happy', true);
  assert.equal(tool.facts.responseVerified, true);
  assert.equal(tool.submits, 1);
  const retryRequest = { timeoutMs: 500, action: 'retry', purpose: 'response', prompt: phase.prompt };
  const retry = await trial(retryRequest, {}, 'happy', true);
  assert.equal(retry.facts.responseVerified, true);
  assert.equal(retry.facts.uniqueComposer, true);
  assert.equal(retry.submits, 1);
  assert.equal(retry.fills, 0);
  assert.equal(retry.clicks, 1);
  assert.equal(retry.keys, 0);
  assert.equal(retry.skips, 0);
  assert.equal(retry.focuses, 0);
  assert.equal(retry.facts.sendMechanism, 'pointer');
  assert.equal(retry.facts.retryPointStable, true);
  assert.equal(retry.facts.retryHitOwnedPoints, 9);
  assert.equal(retry.facts.retryRectInViewport, true);
  const edge = await trial(retryRequest, {}, 'retry-center-covered', true);
  assert.equal(edge.clicks, 1);
  assert.equal(edge.keys, 0);
  assert.equal(edge.facts.retryHitOwnedPoints, 8);
  assert.equal(edge.facts.retrySampleStatus, 'owned');
  assert.equal(edge.facts.retryHitTag, 'button');
  const statuses = { 'retry-all-covered': 'no-owned-point', 'retry-source-clipped': 'clipped',
    'retry-foreign-document': 'foreign-document', 'retry-wrong-type': 'native-control-invalid',
    'retry-hidden-render': 'hidden', 'retry-detached': 'detached', 'retry-disabled': 'disabled',
    'retry-transformed': 'transformed' };
  for (const [scenario, status] of Object.entries(statuses)) {
    const rejected = await trial(retryRequest, {}, scenario, true);
    assert.equal(rejected.facts.retrySampleStatus, status, scenario);
    assert.equal(rejected.clicks, 0);
    assert.equal(rejected.keys, 0);
  }
  for (const scenario of ['retry-all-covered', 'retry-unstable', 'retry-remounted', 'retry-source-clipped', 'retry-foreign-document', 'retry-wrong-type', 'retry-covered-at-final', 'retry-hidden-render', 'retry-detached', 'retry-disabled', 'retry-transformed']) {
    const blocked = await trial(retryRequest, {}, scenario, true);
    assert.equal(blocked.clicks, 0, scenario);
    assert.equal(blocked.keys, 0, scenario);
  }
  for (const scenario of ['retry-ancestor-cover', 'retry-composer-cover']) {
    const front = await trial(retryRequest, {}, scenario, true);
    assert.equal(front.facts.retrySampleStatus, 'no-owned-point');
    assert.equal(front.facts.retryHitTag, 'div');
    assert.equal(front.facts.retryHitAncestor, true);
    assert.equal(front.facts.retryHitSharesTurnPair, true);
    assert.equal(front.facts.retryHitContainsComposer, scenario === 'retry-composer-cover');
    assert.equal(front.clicks, 0);
  }
  const dismissed = await trial({ ...retryRequest, timeoutMs: 700 }, {}, 'command-dismissed', true);
  assert.equal(dismissed.escapes, 1);
  assert.equal(dismissed.clicks, 1);
  assert.equal(dismissed.keys, 0);
  assert.equal(dismissed.facts.retryReveal, 'command-dismissed');
  for (const scenario of ['command-remaining', 'command-escape-failed']) {
    const blocked = await trial(retryRequest, {}, scenario, true);
    assert.equal(blocked.escapes, 1);
    assert.equal(blocked.clicks, 0);
    assert.equal(blocked.facts.retryReveal, 'none');
  }
  const detachedAfterDismiss = await trial(retryRequest, {}, 'command-detached', true);
  assert.equal(detachedAfterDismiss.escapes, 1);
  assert.equal(detachedAfterDismiss.clicks, 0);
  for (const scenario of ['command-forged-class', 'command-missing-content']) {
    const forged = await trial({ ...retryRequest, timeoutMs: 150 }, {}, scenario, true);
    assert.equal(forged.escapes, 0);
    assert.equal(forged.clicks, 0);
  }
  for (const region of ['pane-overlay', 'pane-host', 'narrow-overlay', 'floating-pane', 'tree-group', 'panel-header',
    'panel-page-header', 'zone-tabstrip', 'window-drag-handle', 'dialog-overlay']) {
    const classified = await trial({ ...retryRequest, timeoutMs: 150 }, {}, region, true);
    assert.equal(classified.facts.retryHitRegion, region);
    assert.equal(classified.clicks, 0);
    assert.equal(classified.escapes, 0);
  }
  const fade = await trial(retryRequest, {}, 'retry-fading-overlay', true);
  assert.equal(fade.clicks, 1);
  assert.equal(fade.escapes, 0);
  assert.equal(fade.facts.retryReveal, 'none');
  const changedDuringWait = await trial(retryRequest, {}, 'foreign-card-during-wait', true);
  assert.equal(changedDuringWait.clicks, 0);
  assert.equal(changedDuringWait.escapes, 0);
  for (const scenario of ['gateway-connecting', 'gateway-forged']) {
    const gateway = await trial({ ...retryRequest, timeoutMs: 150 }, {}, scenario, true);
    assert.equal(gateway.facts.retryHitRegion, scenario === 'gateway-connecting' ? scenario : 'other');
    assert.equal(gateway.clicks, 0);
    assert.equal(gateway.escapes, 0);
  }
  const fingerprint = await trial({ ...retryRequest, timeoutMs: 150 }, {}, 'retry-all-covered', true);
  assert.equal(fingerprint.clicks, 0);
  const auxiliary = JSON.parse(fingerprint.auxiliary);
  assert.equal(auxiliary.mechanism, 'hermes-front-source');
  assert.equal(auxiliary.schemaVersion, 1);
  assert.equal(auxiliary.diagnosticsOnly, true);
  const hashToken = token => require('node:crypto').createHash('sha256').update(token).digest('hex');
  for (const token of ['z-(--z-over-modal)', 'z-[var(--z-layer)]', 'z-[123]'])
    assert(auxiliary.levels[0].tokenHashes.includes(hashToken(token)));
  assert(!auxiliary.levels[0].tokenHashes.includes(hashToken('z-[url(private)]')));
  assert.equal(auxiliary.levels.length, 4);
  assert.equal(auxiliary.levels.reduce((sum, level) => sum + level.tokenCount, 0), 48);
  for (const level of auxiliary.levels) {
    assert(level.tokenCount <= 24);
    assert.equal(level.tokenHashes.length, level.tokenCount);
    assert(level.tokenHashes.every(hash => /^[a-f0-9]{64}$/.test(hash)));
  }
  assert(!fingerprint.auxiliary.includes('PRIVATE_SYNTHETIC_VALUE'));
  assert(!fingerprint.auxiliary.includes('static_token_'));
  assert(Buffer.byteLength(fingerprint.auxiliary) <= 8192);
  const failedAux = await trial({ ...retryRequest, timeoutMs: 150 }, {}, 'aux-write-failed', true);
  assert.equal(failedAux.auxiliary, undefined);
  assert.equal(failedAux.clicks, 0);
  assert.equal(failedAux.keys, 0);
  assert.equal(failedAux.facts.inputSubmitted, false);
  const skipped = await trial({ ...retryRequest, timeoutMs: 700 }, {}, 'onboarding-normal', true);
  assert.equal(skipped.skips, 1);
  assert.equal(skipped.retryHandles, 2);
  assert.equal(skipped.clicks, 1);
  assert.equal(skipped.escapes, 0);
  assert.equal(skipped.facts.retryReveal, 'onboarding-skipped');
  const remounted = await trial({ ...retryRequest, timeoutMs: 1000 }, {}, 'onboarding-remount', true);
  assert.equal(remounted.skips, 1);
  assert.equal(remounted.retryHandles, 3);
  assert.equal(remounted.clicks, 1);
  assert.equal(remounted.facts.inputSubmitted, true);
  for (const scenario of ['onboarding-remount-document', 'onboarding-remount-again', 'onboarding-remount-foreign-pair']) {
    const rejected = await trial({ ...retryRequest, timeoutMs: 1000 }, {}, scenario, true);
    assert.equal(rejected.skips, 1, scenario);
    assert.equal(rejected.clicks, 0, scenario);
    assert.equal(rejected.facts.inputSubmitted, false, scenario);
  }
  for (const scenario of ['onboarding-remaining', 'onboarding-uncertain', 'onboarding-foreign-pair', 'onboarding-duplicate', 'onboarding-disabled', 'onboarding-missing', 'onboarding-forged']) {
    const blocked = await trial({ ...retryRequest, timeoutMs: 350 }, {}, scenario, true);
    assert.equal(blocked.clicks, 0, scenario);
    assert.equal(blocked.escapes, 0, scenario);
    assert.equal(blocked.facts.inputSubmitted, false, scenario);
  }
  const uncertain = await trial(retryRequest, {}, 'retry-intercepted', true);
  assert.equal(uncertain.clicks, 1);
  assert.equal(uncertain.keys, 0);
  assert.equal(uncertain.submits, 0);
  assert.equal(uncertain.facts.inputSubmitted, false);
  for (const scenario of ['missing-error', 'duplicate-error', 'missing-retry', 'foreign-error-turn', 'foreign-user', 'blocked-modal', 'blocked-menu', 'inert', 'stale', 'duplicate', 'duplicate-retry', 'send-disabled']) {
    const rejected = await trial(retryRequest, {}, scenario, true);
    assert.equal(rejected.submits, 0, scenario);
    assert.equal(rejected.fills, 0, scenario);
  }
  const delayedForeign = await trial({ ...phase, timeoutMs: 10 }, {}, 'delayed-foreign-error', true);
  assert.equal(delayedForeign.submits, 1);
  assert.equal(delayedForeign.facts.errorObserved, false);
  assert.equal(delayedForeign.facts.retryControl, false);
  assert.equal(delayedForeign.facts.errorCategory, 'response-timeout');
  const foreignResponse = await trial({ ...retryRequest, timeoutMs: 250 }, {}, 'foreign-response-turn', true);
  assert.equal(foreignResponse.submits, 1);
  assert.equal(foreignResponse.facts.userTurnObserved, true);
  assert.equal(foreignResponse.facts.responseVerified, false);
  assert.deepEqual(JSON.parse(foreignResponse.backend).responseShape,
    { exactUserCount: 1, markerAssistantCount: 1, boundMarkerAssistantCount: 0 });
  assert.equal(foreignResponse.facts.errorCategory, 'response-timeout');
  const invalid = await trial({ ...phase, action: 'retry' }, {}, null, true);
  assert.equal(invalid.facts.errorCategory, 'invalid-request');
  console.log('Hermes DOM feasibility and qualification guards: synthetic cases passed');
})().catch(error => { console.error(error.message); process.exitCode = 1; });
