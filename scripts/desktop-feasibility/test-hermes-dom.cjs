// Exercise closed failures without launching a browser or desktop application.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-hermes.cjs`, 'utf8').replace('await driveDom(); process.exit(', 'await driveDom(); return process.exit(');
async function trial(overrides, connectionOverrides = {}, scenario = null) {
  const output = new Map();
  const request = { connectionPath: '/connection', ownerPid: 20, prompt: 'Check this connection',
    expectedMarker: 'NAN CHECK RESPONSE ' + 'APPLE '.repeat(32), timeoutMs: 1, ...overrides };
  const connection = { schemaVersion: 1, port: 43210, launcherPid: 30, ...connectionOverrides };
  let attaches = 0;
  let submits = 0;
  let fills = 0;
  let pageEnumerations = 0;
  let value = '';
  const rendererUrl = 'file:///synthetic/resources/app.asar.unpacked/dist/index.html';
  const composer = {
    async count() { return scenario === 'duplicate' ? 2 : 1; },
    async isEditable() { return true; },
    async fill(prompt) { fills++; value = scenario === 'mismatch' ? 'different synthetic input' : prompt; },
    async evaluate(callback, prompt) { return callback({ value }, prompt); },
    async press(key) { assert.equal(key, 'Enter'); submits++; },
  };
  const assistant = {
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
    context() { return { async newCDPSession() { return { async send(method) {
      assert.equal(method, 'Target.getTargetInfo');
      return { targetInfo: { type: 'page', url: rendererUrl } };
    } }; } }; },
    async evaluate() { return true; },
    locator(selector) {
      if (selector === '[data-role="assistant"]:visible') return assistant;
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
      process: { argv: ['node', 'helper', '--drive', '/request', '/output'], exit: resolve },
    });
    setTimeout(() => reject(new Error('bounded helper fixture timed out')), 1000).unref();
  });
  assert.equal(attaches, scenario ? 1 : 0);
  const facts = JSON.parse(output.get('/output'));
  assert.equal(facts.inputSubmitted, ['happy', 'delayed'].includes(scenario));
  assert.equal(facts.responseVerified, ['happy', 'delayed'].includes(scenario));
  assert.equal(facts.attached, Boolean(scenario));
  assert.equal(submits, ['happy', 'delayed'].includes(scenario) ? 1 : 0);
  if (scenario === 'stale' || scenario === 'duplicate') assert.equal(fills, 0);
  if (scenario === 'mismatch' || ['happy', 'delayed'].includes(scenario)) assert.equal(fills, 1);
  if (scenario) {
    assert.equal(facts.endpointOwned, true);
    assert.equal(facts.targetVerified, true);
    assert.equal(facts.inputReadback, ['happy', 'delayed'].includes(scenario));
  }
  assert(!output.get('/output').includes(request.expectedMarker));
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
  const delayed = await trial({ timeoutMs: 500 }, {}, 'delayed');
  assert.equal(delayed.responseVerified, true);
  console.log('Hermes DOM guards and submission: 9 synthetic cases passed');
})().catch(error => { console.error(error.message); process.exitCode = 1; });
