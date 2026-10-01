// Exercise closed failures without launching a browser or desktop application.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-hermes.cjs`, 'utf8').replace('await driveDom(); process.exit(', 'await driveDom(); return process.exit(');
async function trial(overrides, connectionOverrides = {}, scenario = null, qualify = false) {
  const output = new Map();
  const request = { connectionPath: '/connection', ownerPid: 20, prompt: 'Check this connection',
    expectedMarker: 'NAN CHECK RESPONSE ' + 'APPLE '.repeat(32), timeoutMs: 1, ...overrides };
  const connection = { schemaVersion: 1, port: 43210, launcherPid: 30, ...connectionOverrides };
  let attaches = 0;
  let submits = 0;
  let fills = 0;
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
    async evaluate(callback) {
      if (callback.toString().includes('document.activeElement')) return scenario !== 'focus-failed';
      return scenario === 'blocked-modal' ? 'modal' : scenario === 'blocked-menu' ? 'menu'
        : scenario === 'inert' ? 'inert' : null;
    },
    async scrollIntoViewIfNeeded() {},
    async focus() {},
    async click() { throw new Error('Semantic driver must never click'); },
    async press(key) {
      assert.equal(key, 'Enter');
      if (scenario === 'click-timeout') throw new Error('Timeout 100ms exceeded PRIVATE_SYNTHETIC_VALUE');
      submits++; value = '';
    },
  };
  const errorCards = {
    async count() { return scenario === 'failure-appears' && submits === 0 ? 0 : scenario === 'missing-error' ? 0 : scenario === 'duplicate-error' ? 2 : 1; },
    getByRole(role, options) { assert.equal(role, 'button'); assert.equal(options.name, 'Retry'); return { ...send, async count() { return scenario === 'missing-retry' ? 0 : 1; } }; },
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
  const page = {
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
      if (!args) return true;
      assert.equal(args.prompt, request.prompt);
      assert.equal(args.marker, request.expectedMarker);
      observations++;
      if (scenario === 'transient-context' && observations === 1) throw new Error('Execution context was destroyed');
      return { inputCleared: scenario !== 'missing-editor', userTurnObserved: submits === 1,
        assistantTurnCount: submits, responseVerified: submits === 1 };
    },
    locator(selector) {
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
      if (path === '/proc/net/tcp') return scenario
        ? `header\n0: 0100007F:${connection.port.toString(16).toUpperCase()} 00000000:0000 0A 0 0 0 0 0 777\n`
        : 'header\n';
      throw new Error('synthetic missing process');
    },
    readdirSync(path) { return path === '/proc' ? ['30'] : ['5']; },
    readlinkSync() { return 'socket:[777]'; },
    writeFileSync(path, value) { output.set(path, value); },
    renameSync(from, to) { output.set(to, output.get(from)); },
  };
  await new Promise((resolve, reject) => {
    vm.runInNewContext(source, {
      require(name) {
        if (name === 'node:fs') return mockFs;
        if (name.endsWith('/package.json')) return { version: '1.61.1' };
        return { chromium: { async connectOverCDP() { attaches++;
          if (!scenario) throw new Error('must not attach'); return browser; } } };
      },
      URL, setTimeout,
      process: { argv: ['node', 'helper', qualify ? '--qualify' : '--drive', '/request', '/output'], exit: resolve },
    });
    setTimeout(() => reject(new Error('bounded helper fixture timed out')), 1000).unref();
  });
  assert.equal(attaches, scenario ? 1 : 0);
  const facts = JSON.parse(output.get('/output'));
  if (qualify) {
    assert(!output.get('/output').includes(request.expectedMarker));
    return { facts, submits, fills };
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
  for (const scenario of ['missing-error', 'duplicate-error', 'missing-retry', 'foreign-user', 'blocked-modal', 'focus-failed', 'stale', 'duplicate']) {
    const rejected = await trial(retryRequest, {}, scenario, true);
    assert.equal(rejected.submits, 0, scenario);
    assert.equal(rejected.fills, 0, scenario);
  }
  const invalid = await trial({ ...phase, action: 'retry' }, {}, null, true);
  assert.equal(invalid.facts.errorCategory, 'invalid-request');
  console.log('Hermes DOM feasibility and qualification guards: 32 synthetic cases passed');
})().catch(error => { console.error(error.message); process.exitCode = 1; });
