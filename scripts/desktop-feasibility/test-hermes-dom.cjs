// Exercise closed failures without launching a browser or desktop application.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const ownershipSource = fs.readFileSync(`${__dirname}/endpoint-ownership.cjs`, 'utf8');
const ownershipFunctions = ownershipSource.slice(ownershipSource.indexOf('function saveWindowsProof('), ownershipSource.indexOf('return { ownedEndpoint'));
const source = fs.readFileSync(`${__dirname}/observe-hermes.cjs`, 'utf8').replace("const { ownedEndpoint, descendant, parentPid, windowsProof } = require('./endpoint-ownership.cjs').proof(owner, port);", ownershipFunctions).replace("require('./hermes-response-observation.cjs').observe", '(' + require('./hermes-response-observation.cjs').observe.toString() + ')').replace("require('./hermes-retry-subtree.cjs')", `(() => { const module = { exports: {} }; ${fs.readFileSync(`${__dirname}/hermes-retry-subtree.cjs`, 'utf8')} return module.exports; })()`).replace("require('./hermes-before-failure.cjs')", `(() => { const module = { exports: {} }; ${fs.readFileSync(`${__dirname}/hermes-before-failure.cjs`, 'utf8')} return module.exports; })()`).replace('await driveDom(); process.exit(', 'await driveDom(); return process.exit(');
async function trial(overrides, connectionOverrides = {}, scenario = null, qualify = false) {
  const beforeFailureTrial = scenario?.startsWith('before-failure-') ?? false;
  if (beforeFailureTrial) scenario = scenario.slice('before-failure-'.length);
  const subtreeTrial = scenario?.startsWith('subtree-') ?? false;
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
  let onboardingVisible = (scenario?.startsWith('onboarding-') ?? false) || scenario === 'subtree-after-reveal';
  let samplingChoice = false;
  let commandVisible = scenario?.startsWith('command-') ?? false;
  let focusChecks = 0;
  let focused = false;
  let focuses = 0;
  let settleTimerObserved = false;
  let scopeCaptures = 0, capturingScope = false;
  let scopeChecks = 0, scopeChanged = false, heldUserDetached = false, currentAlert;
  const unwrap = value => value && Object.hasOwn(value, 'scopeValue') ? value.scopeValue : value;
  const scopeHandle = value => ({ scopeValue: value,
    async evaluate(callback, argument) { return callback(value, unwrap(argument)); },
    async getProperty(key) { return { asElement: () => ({
      async evaluate(callback, argument) { sampleCount++; return callback(value[key], unwrap(argument)); },
      async click(options) { return send.click(options); }, async dispose() {},
    }) }; }, async dispose() {} });
  const scopeIncomplete = kind => capturingScope && (scenario === 'initial-scope-' + kind
    || scenario === 'initial-scope-' + kind + '-settles' && scopeCaptures === 1);
  const messageGroup = { isConnected: true };
  const focusButton = { tagName: 'BUTTON', tabIndex: scenario === 'retry-not-focusable' ? -1 : 0,
    isConnected: scenario !== 'retry-disconnected', parentElement: null,
    closest(selector) { return selector === '[inert]' && scenario === 'retry-ancestor-inert' ? {} : null; },
    matches(selector) { assert.equal(selector, ':disabled'); return scenario === 'retry-fieldset-disabled'; } };
  const reclaimedComposer = { tagName: 'DIV', privateText: 'PRIVATE_SYNTHETIC_VALUE',
    closest(selector) { return selector === '[data-slot="composer-root"]' ? {} : null; } };
  const turnPair = { isConnected: true, closest(selector) { return selector === '[data-slot="aui_message-group"]' ? messageGroup : null; },
    querySelectorAll(selector) { assert.equal(selector, '[data-role="user"]');
      if (scopeIncomplete('missing-user')) return [];
      if (capturingScope && scenario === 'initial-scope-duplicate-user') return [boundUser, { ...boundUser }];
      return [scenario === 'sampling-remount-user' && sampleCount >= 2 ? { ...boundUser } : boundUser]; } };
  const replacementPair = { ...turnPair };
  const currentPair = () => scopeChanged && scenario === 'scope-pair-missing' ? null
    : scopeChanged && (subtreeTrial || ['scope-user-reparented', 'scope-whole-tuple-remounted', 'scope-stability-pair-remounted', 'scope-diagnostic-failed'].includes(scenario))
      || scenario === 'initial-scope-valid-pair-changed' && scopeCaptures > 0 && !capturingScope ? replacementPair : turnPair;
  const boundUser = { get isConnected() { return !heldUserDetached && !scopeIncomplete('disconnected-user'); }, closest(selector) { return selector === '[inert]' ? null : currentPair(); }, get innerText() {
    if (capturingScope && scenario === 'initial-scope-foreign-user') return 'Foreign synthetic prompt';
    return (['foreign-error-turn', 'delayed-foreign-error'].includes(scenario)
      || scenario === 'foreign-card-after-settle' && frames === 2
      || scenario === 'foreign-card-during-wait' && sampleCount >= 2
      || scenario === 'onboarding-foreign-pair' && skips > 0
      || scenario === 'onboarding-remount-foreign-pair' && retryHandles >= 3)
      ? 'Older unrelated user turn' : request.prompt;
  } };
  const assistantRoot = { isConnected: true, closest() { return scopeIncomplete('missing-pair') ? null : currentPair(); } };
  const boundAlert = { isConnected: true, get ownerDocument() { return capturingScope && scenario === 'initial-scope-foreign-document' ? {} : fixtureDocument; },
    closest(selector) { return selector === '[inert]' ? null : selector.includes('assistant-message-root')
      ? assistantRoot : scopeIncomplete('missing-pair') ? null
        : scenario === 'sampling-remount-pair' && sampleCount >= 2 ? { ...turnPair } : currentPair(); } };
  currentAlert = boundAlert;
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
    closest(selector) { return selector === '[role="alert"]' ? capturingScope && scenario === 'initial-scope-foreign-retry' ? {} : boundAlert : selector === '[data-slot="aui_turn-pair"]' ? turnPair : selector === '[data-slot="aui_thread-viewport"]' ? {} : null; },
    getBoundingClientRect() { const left = scenario === 'retry-offviewport' ? 200 : scenario === 'retry-unstable' && sampleCount > 1 ? 11 : 10; return { left, right: left + 20, top: 10, bottom: 30, width: 20, height: 20 }; },
    contains(element) { return element === hitChild; },
    scrollIntoView(options) {
      assert.equal(options.block, 'center');
      assert.equal(options.inline, 'nearest');
      assert.equal(options.behavior, 'instant');
      centers++;
    },
  };
  let subtreeReplacement;
  let subtreeButton;
  let subtreeFixtureError;
  const subtreeStates = [];
  if (subtreeTrial) {
    const surface = { isConnected: true, get ownerDocument() { return fixtureDocument; },
      getAttribute(key) { return key === 'data-session-anchor' ? 'workspace' : key === 'data-composer-target' ? 'main' : null; } };
    const parent = { isConnected: true, get ownerDocument() { return fixtureDocument; } };
    const viewport = { isConnected: true, get ownerDocument() { return fixtureDocument; },
      contains(node) { return node === parent; }, closest(selector) { return selector === '[data-chat-surface]' ? surface : null; } };
    Object.assign(messageGroup, { parentElement: parent,
      closest(selector) { return selector === '[data-chat-surface]' ? surface : selector === '[data-slot="aui_thread-viewport"]' ? viewport : null; },
      querySelectorAll(selector) { return selector === '[data-slot="aui_turn-pair"]' ? [currentPair()] : [currentAlert]; } });
    Object.defineProperty(messageGroup, 'ownerDocument', { get: () => fixtureDocument });
    const visible = node => { node.getBoundingClientRect ??= () => ({ width: 20, height: 20 });
      Object.defineProperty(node, 'ownerDocument', { configurable: true, get: () => fixtureDocument }); return node; };
    for (const node of [turnPair, boundUser, boundAlert, replacementPair]) visible(node);
    subtreeReplacement = () => {
      scopeChanged = true; turnPair.isConnected = false; heldUserDetached = true;
      boundAlert.isConnected = false; hitButton.isConnected = false;
      const user = visible({ ...boundUser, isConnected: true });
      currentAlert = visible({ ...boundAlert, isConnected: true });
      const button = visible({ ...hitButton, isConnected: true, innerText: 'Retry',
        closest(selector) { return selector === '[role="alert"]' ? currentAlert
          : selector === '[data-slot="aui_turn-pair"]' ? replacementPair : null; } });
      currentAlert.querySelectorAll = () => [button];
      replacementPair.querySelectorAll = () => [user];
      subtreeButton = button;
    };
  }
  let pageEnumerations = 0;
  let observations = 0;
  let value = '';
  const rendererUrl = 'file:///synthetic/resources/app.asar.unpacked/dist/index.html';
  const composerElement = { get value() { return value; } };
  const composer = {
    async elementHandle() { return { scopeValue: composerElement, async dispose() {} }; },
    async count() { return scenario === 'duplicate' ? 2 : 1; },
    async isEditable() { return true; },
    async fill(prompt) { fills++; value = scenario === 'mismatch' ? 'different synthetic input' : prompt; },
    async evaluate(callback, prompt) {
      if (beforeFailureTrial && callback.toString().includes('element === original')) return callback(composerElement, unwrap(prompt));
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
    async elementHandle() { retryHandles++; latestHandleSamples=0; const generation = retryHandles; let handleSamples = 0; hitButton.ownerDocument = fixtureDocument; return {
      scopeValue: hitButton, async dispose() {},
      async evaluate(callback, argument) {
        if (callback.toString().includes('old.ownerDocument')) return scenario !== 'onboarding-remount-document';
        if (callback.toString().includes('held.document')) {
          hitButton.ownerDocument = fixtureDocument;
          if (scenario === 'sampling-remount-document') hitButton.ownerDocument = {};
          return callback(hitButton, unwrap(argument));
        }
        handleSamples++; latestHandleSamples=handleSamples; samplingChoice = false; sampleCount++; hitButton.isConnected = scenario !== 'retry-detached' && !(scenario === 'command-detached' && escapes > 0); if (scenario?.startsWith('onboarding-remount') && skips > 0 && generation >= 2 && handleSamples >= 3
          && (generation === 2 || scenario === 'onboarding-remount-again')) hitButton.isConnected = false;
        if (scenario?.startsWith('sampling-remount') && handleSamples >= 2
            && (generation === 1 || scenario === 'sampling-remount-again')) hitButton.isConnected = false;
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
      if (scenario === 'subtree-native-owner-rejected') return;
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
    async evaluateHandle(callback, argument) {
      scopeCaptures++; capturingScope = true;
      try { return scopeHandle(callback(boundAlert, { ...argument,
        button: unwrap(argument.button), originalDocument: unwrap(argument.originalDocument) })); }
      finally { capturingScope = false; }
    },
    async evaluate(callback, argument) {
      if (callback.toString().includes('let reason = null')) {
        scopeChecks++;
        if (scenario?.startsWith('scope-') && scopeChecks === (scenario === 'scope-stability-pair-remounted' ? 2 : 1)) {
          scopeChanged = true;
          if (['scope-pair-detached', 'scope-user-reparented', 'scope-whole-tuple-remounted', 'scope-stability-pair-remounted', 'scope-diagnostic-failed'].includes(scenario)) turnPair.isConnected = false;
          if (scenario === 'scope-pair-detached') {
            turnPair.closest = () => null;
            heldUserDetached = true; boundAlert.isConnected = false; hitButton.isConnected = false;
          }
          if (['scope-whole-tuple-remounted', 'scope-stability-pair-remounted'].includes(scenario)) {
            heldUserDetached = true; boundAlert.isConnected = false; hitButton.isConnected = false;
            replacementPair.querySelectorAll = () => [{ ...boundUser, isConnected: true }];
            currentAlert = { ...boundAlert, isConnected: true };
          }
          if (scenario === 'scope-alert-changed') { boundAlert.isConnected = false; currentAlert = { ...boundAlert, isConnected: true }; }
          if (scenario === 'scope-document-changed') currentAlert = { ...boundAlert, ownerDocument: {} };
          if (scenario === 'scope-user-changed') {
            heldUserDetached = true; turnPair.querySelectorAll = () => [{ ...boundUser, isConnected: true }];
          }
          if (scenario === 'scope-user-multiple') turnPair.querySelectorAll = () => [boundUser, { ...boundUser }, { ...boundUser }];
          if (scenario === 'scope-group-changed') turnPair.closest = () => ({ isConnected: true });
          if (scenario === 'scope-diagnostic-failed') Object.defineProperty(messageGroup, 'isConnected', {
            get() { throw Error('PRIVATE_SYNTHETIC_VALUE'); },
          });
        }
      }
      if (scenario === 'sampling-remount-alert' && sampleCount >= 2) return callback({ ...boundAlert }, unwrap(argument));
      return callback(currentAlert, unwrap(argument));
    },
    async count() { return (beforeFailureTrial || ['failure-appears', 'delayed-foreign-error'].includes(scenario)) && submits === 0 ? 0 : scenario === 'missing-error' ? 0 : scenario === 'duplicate-error' ? 2 : 1; },
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
    async evaluate(callback) { samplingChoice = true; choiceButton.ownerDocument = fixtureDocument;
      onboardingRoot.ownerDocument = fixtureDocument; onboardingRoot.isConnected = true;
      return callback(choiceButton); },
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
    async evaluateHandle(callback, argument) {
      try {
        if (subtreeTrial && callback.name === 'inspectSubtree' && !scopeChanged
            && (scenario !== 'subtree-after-reveal' || sampleCount >= 2)) {
          assert.equal(callback(unwrap(argument)).state, 'unchanged', 'original source anchors');
          subtreeReplacement();
        }
        const value = callback(unwrap(argument));
        if (subtreeTrial && callback.name === 'inspectSubtree') subtreeStates.push(value.state);
        return scopeHandle(value);
      } catch (error) { subtreeFixtureError = error; throw error; }
    },
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
      if (_callback.name === 'sameDocument') return _callback(unwrap(args));
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
        if (name === 'node:child_process' && beforeFailureTrial) return { execFileSync(command) {
          if (command === '/bin/ps') return '20';
          assert.equal(command, '/usr/sbin/lsof'); return 'p30\nf5\nn127.0.0.1:43210\n';
        } };
        if (name === 'node:child_process' && subtreeTrial) return { execFileSync(command, args) {
          assert.equal(command, 'pwsh');
          assert.ok(args.includes('endpoint') || args.includes('descendant'));
          return 'true';
        } };
        if (name === './owned-renderer-connection.cjs') return require(name);
        if (name.endsWith('/package.json')) return { version: '1.61.1' };
        return { chromium: { async connectOverCDP() { attaches++;
          if (!scenario) throw new Error('must not attach'); return browser; } } };
      },
      __dirname, URL, Buffer, clearTimeout, setTimeout: (callback, delayMs) => { if (delayMs === 0) settleTimerObserved = true; return setTimeout(callback, delayMs); }, requestAnimationFrame: callback => {
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
          || (scenario === 'retry-intercepted' && clicks > 0) ? overlay : scenario === 'retry-fading-overlay' && sampleCount <= 2 ? foreignHit : ['retry-ancestor-cover', 'retry-composer-cover'].includes(scenario) ? ancestorFront : (['retry-all-covered', 'aux-write-failed'].includes(scenario) || scenario === 'retry-covered-at-final' && sampleCount >= 3) ? foreignHit : (sourceRegionSelectors[scenario] || ['gateway-connecting', 'gateway-forged'].includes(scenario)) ? sourceRegionHit : scenario === 'retry-frame-settle' && frames < 2 ? threadBackground : scenario === 'retry-source-clipped' ? threadBackground : scenario === 'retry-offviewport' ? null : scenario === 'retry-private-hit' ? foreignHit : scenario === 'retry-body-hit' ? bodyHit : scenario === 'retry-dock-strip' ? dockStrip : scenario === 'retry-child-hit' ? hitChild : subtreeButton ?? hitButton },
      location: { href: rendererUrl + '#/PRIVATE-owned-session', hash: '#/PRIVATE-owned-session' },
      process: { env: subtreeTrial ? { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', RUNNER_OS: 'Windows',
        FEASIBILITY_HERMES_RETRY_SUBTREE_SOURCE: '0.17.6:f97608f178d1ffeca59860195ab7da295f7c8e5f' } : {},
        platform: beforeFailureTrial ? 'darwin' : subtreeTrial ? 'win32' : 'linux', argv: ['node', 'helper', qualify ? '--qualify' : '--drive', '/request', '/output'], exit: resolve },
    });
    setTimeout(() => reject(new Error('bounded helper fixture timed out')), 1000).unref();
  });
  if (subtreeFixtureError) throw subtreeFixtureError;
  assert.equal(attaches, scenario ? 1 : 0);
  const facts = JSON.parse(output.get('/output'));
  if (qualify) {
    assert(!output.get('/output').includes(request.expectedMarker));
    return { facts, submits, fills, clicks, keys, centers, frames, focuses, escapes, skips, retryHandles, subtreeStates, auxiliary: output.get('/output.front.json'), backend: output.get('/output.backend.json') };
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
  const early = await trial({ ...phase, timeoutMs: 800 }, {}, 'before-failure-onboarding-normal', true);
  assert.equal(early.facts.firstRunPreparation, 'dismissed');
  assert.equal(early.skips, 1);
  assert.equal(early.submits, 1);
  assert.equal(early.fills, 1);
  assert.equal(early.retryHandles, 0, 'no failed-turn identity exists before preparation');
  assert.equal(early.clicks, 0, 'preparation never dispatches Retry');
  const preparationFixtures = [early.facts];
  for (const scenario of ['onboarding-remaining', 'onboarding-uncertain', 'onboarding-duplicate',
      'onboarding-disabled', 'onboarding-missing', 'onboarding-forged']) {
    const refused = await trial({ ...phase, timeoutMs: 350 }, {}, 'before-failure-' + scenario, true);
    assert.equal(refused.submits, 0, scenario);
    assert.equal(refused.fills, 0, scenario);
    assert(refused.skips <= 1, scenario);
    assert.equal(refused.clicks, 0, scenario);
    assert.equal(refused.facts.inputSubmitted, false, scenario);
    assert(['blocked', 'uncertain'].includes(refused.facts.firstRunPreparation), scenario);
    preparationFixtures.push(refused.facts);
  }
  if (process.argv[4]) fs.writeFileSync(process.argv[4], JSON.stringify(preparationFixtures) + '\n', { mode: 0o600 });
  assert.deepEqual(preparationFixtures,
    JSON.parse(fs.readFileSync(`${__dirname}/../../canary/tests/fixtures/hermes-first-run-preparation.json`, 'utf8')),
    'actual first-run emitter matches the shared Rust/Python fixtures');
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
  assert.equal(retry.facts.retrySubtreeSettlement, undefined, 'legacy policy does not attempt row settlement');
  const settledSubtree = await trial({ ...retryRequest, timeoutMs: 900 }, {}, 'subtree-whole-remount', true);
  assert.equal(settledSubtree.facts.retrySubtreeSettlement, 'completed', JSON.stringify({
    errorCategory: settledSubtree.facts.errorCategory, scopeFailure: settledSubtree.facts.retryScopeFailure,
    scopeObservation: settledSubtree.facts.retryScopeObservation, states: settledSubtree.subtreeStates }));
  assert.equal(settledSubtree.facts.retryScopeFailure, null);
  assert.equal(settledSubtree.facts.retryScopeObservation, undefined);
  assert.equal(settledSubtree.facts.responseVerified, true);
  assert.equal(settledSubtree.clicks, 1);
  for (const key of ['fills', 'keys', 'skips', 'escapes']) assert.equal(settledSubtree[key], 0);
  assert.ok(!JSON.stringify(settledSubtree.facts).includes('PRIVATE'));
  const nativeOwnerRejected = await trial({ ...retryRequest, timeoutMs: 600 }, {}, 'subtree-native-owner-rejected', true);
  assert.equal(nativeOwnerRejected.facts.retrySubtreeSettlement, 'completed');
  assert.equal(nativeOwnerRejected.clicks, 1);
  assert.equal(nativeOwnerRejected.submits, 0, 'controller never replaces a refused native session RPC');
  assert.equal(nativeOwnerRejected.facts.responseVerified, false);
  assert.equal(nativeOwnerRejected.facts.errorCategory, 'response-timeout');
  assert.equal(nativeOwnerRejected.fills, 0);
  assert.equal(nativeOwnerRejected.keys, 0);
  const revealRejected = await trial({ ...retryRequest, timeoutMs: 900 }, {}, 'subtree-after-reveal', true);
  assert.equal(revealRejected.facts.retrySubtreeSettlement, 'rejected');
  assert.equal(revealRejected.facts.retryScopeFailure, 'pair-changed');
  for (const key of ['fills', 'clicks', 'keys', 'skips', 'escapes']) assert.equal(revealRejected[key], 0);
  assert.equal(revealRejected.facts.inputSubmitted, false);
  if (process.argv[3]) fs.writeFileSync(process.argv[3], JSON.stringify(settledSubtree.facts) + '\n', { mode: 0o600 });
  for (const scenario of ['initial-scope-missing-pair-settles', 'initial-scope-missing-user-settles', 'initial-scope-disconnected-user-settles']) {
    const settled = await trial({ ...retryRequest, timeoutMs: 700 }, {}, scenario, true);
    assert.equal(settled.clicks, 1, scenario);
    assert.equal(settled.keys, 0, scenario);
    assert.equal(settled.fills, 0, scenario);
    assert.equal(settled.facts.responseVerified, true, scenario);
  }
  for (const scenario of ['initial-scope-missing-pair', 'initial-scope-missing-user', 'initial-scope-disconnected-user']) {
    const incomplete = await trial({ ...retryRequest, timeoutMs: 150 }, {}, scenario, true);
    assert.equal(incomplete.clicks, 0, scenario);
    assert.equal(incomplete.keys, 0, scenario);
    assert.equal(incomplete.facts.retryScopeFailure, 'initial-scope-incomplete', scenario);
  }
  for (const scenario of ['initial-scope-duplicate-user', 'initial-scope-foreign-user', 'initial-scope-foreign-retry', 'initial-scope-foreign-document']) {
    const invalid = await trial(retryRequest, {}, scenario, true);
    assert.equal(invalid.clicks, 0, scenario);
    assert.equal(invalid.keys, 0, scenario);
    assert.equal(invalid.facts.retryScopeFailure, 'initial-scope-invalid', scenario);
  }
  const changedCandidate = await trial(retryRequest, {}, 'initial-scope-valid-pair-changed', true);
  assert.equal(changedCandidate.clicks, 0);
  assert.equal(changedCandidate.keys, 0);
  assert.equal(changedCandidate.facts.retryScopeFailure, 'pair-changed');
  assert.equal(changedCandidate.facts.retryScopeObservation.phase, 'initial-reproof');
  const scopeFixtures = [];
  const scopeBooleans = ['documentSame', 'currentPairPresent', 'currentPairSame', 'currentPairConnected',
    'heldPairConnected', 'currentGroupSame', 'heldGroupConnected', 'currentUserSame',
    'heldUserConnected', 'currentAlertSame', 'heldAlertConnected', 'heldRetryConnected'];
  for (const [scenario, reason, changes] of [
    ['scope-pair-missing', 'pair-changed', { currentPairPresent: false, currentPairSame: false,
      currentPairConnected: false, currentGroupSame: false, currentUserSame: false, currentUserCount: 0 }],
    ['scope-pair-detached', 'pair-changed', { currentPairConnected: false, heldPairConnected: false,
      currentGroupSame: false, heldUserConnected: false, heldAlertConnected: false, heldRetryConnected: false }],
    ['scope-user-reparented', 'pair-changed', { currentPairSame: false, heldPairConnected: false }],
    ['scope-whole-tuple-remounted', 'pair-changed', { currentPairSame: false, heldPairConnected: false,
      currentUserSame: false, heldUserConnected: false, currentAlertSame: false,
      heldAlertConnected: false, heldRetryConnected: false }],
    ['scope-stability-pair-remounted', 'pair-changed', { currentPairSame: false, heldPairConnected: false,
      currentUserSame: false, heldUserConnected: false, currentAlertSame: false,
      heldAlertConnected: false, heldRetryConnected: false, phase: 'initial-stability' }],
    ['scope-user-changed', 'user-changed', { currentUserSame: false, heldUserConnected: false }],
    ['scope-user-multiple', 'user-changed', { currentUserSame: false, currentUserCount: 2 }],
    ['scope-alert-changed', 'alert-changed', { currentAlertSame: false, heldAlertConnected: false }],
    ['scope-document-changed', 'document-changed', { documentSame: false, currentAlertSame: false }],
  ]) {
    const refused = await trial(retryRequest, {}, scenario, true);
    assert.equal(refused.facts.retryScopeFailure, reason, scenario);
    assert.deepEqual(refused.facts.retryScopeObservation, {
      phase: 'initial-reproof', ...Object.fromEntries(scopeBooleans.map(key => [key, true])),
      currentUserCount: 1, ...changes,
    }, scenario);
    assert.equal(refused.facts.errorCategory, 'send-unavailable', scenario);
    for (const key of ['submits', 'fills', 'clicks', 'keys', 'skips', 'escapes']) assert.equal(refused[key], 0, scenario);
    assert.equal(refused.facts.inputSubmitted, false, scenario);
    assert.ok(!JSON.stringify(refused.facts).includes('PRIVATE'), scenario);
    if (scenario === 'scope-whole-tuple-remounted') assert.deepEqual(refused.facts,
      JSON.parse(fs.readFileSync(`${__dirname}/../../canary/tests/fixtures/hermes-retry-scope.json`, 'utf8')),
      'actual emitter matches the full shared Rust/Python receipt');
    scopeFixtures.push({ scenario, facts: refused.facts });
  }
  const unavailableScope = await trial(retryRequest, {}, 'scope-diagnostic-failed', true);
  assert.equal(unavailableScope.facts.retryScopeFailure, 'pair-changed');
  assert.equal(unavailableScope.facts.errorCategory, 'send-unavailable');
  assert.equal(unavailableScope.facts.retryScopeObservation, undefined);
  assert.equal(unavailableScope.clicks, 0);
  assert.equal(unavailableScope.keys, 0);
  assert.equal(retry.facts.retryScopeObservation, undefined, 'success has no identity refusal');
  const diagnosticGroup = await trial(retryRequest, {}, 'scope-group-changed', true);
  assert.equal(diagnosticGroup.facts.responseVerified, true, 'group witness adds no acceptance guard');
  assert.equal(diagnosticGroup.clicks, 1);
  assert.equal(diagnosticGroup.facts.retryScopeObservation, undefined);
  if (process.argv[2]) fs.writeFileSync(process.argv[2], JSON.stringify(scopeFixtures) + '\n', { mode: 0o600 });
  const remount = await trial({ ...retryRequest, timeoutMs: 900 }, {}, 'sampling-remount', true);
  assert.equal(remount.clicks, 1);
  assert.equal(remount.retryHandles, 2);
  assert.equal(remount.keys, 0);
  assert.equal(remount.fills, 0);
  const scopeReasons = { 'sampling-remount-alert': 'alert-changed', 'sampling-remount-pair': 'pair-changed', 'sampling-remount-user': 'user-changed' };
  for (const scenario of ['sampling-remount-again', 'sampling-remount-alert', 'sampling-remount-document', 'sampling-remount-pair', 'sampling-remount-user']) {
    const rejected = await trial({ ...retryRequest, timeoutMs: 900 }, {}, scenario, true);
    assert.equal(rejected.clicks, 0, scenario);
    assert.equal(rejected.keys, 0, scenario);
    if (scopeReasons[scenario]) assert.equal(rejected.facts.retryScopeFailure, scopeReasons[scenario], scenario);
    if (scopeReasons[scenario]) assert.equal(rejected.facts.retryScopeObservation.phase, 'sampling-remount', scenario);
  }
  const expiredRemount = await trial({ ...retryRequest, timeoutMs: 150 }, {}, 'sampling-remount', true);
  assert.equal(expiredRemount.clicks, 0);
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
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
