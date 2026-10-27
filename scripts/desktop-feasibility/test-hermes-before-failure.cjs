'use strict';

// Controlled preparation only: no browser, provider or native input is used.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const { prepare } = require('./hermes-before-failure.cjs');

async function trial(scenario, deadline = 800, tick = false) {
  let time = 0, lastNow = 0, samples = 0, clicks = 0, fills = 0, submits = 0;
  let document = {}, url = 'file:///synthetic/dist/index.html', visible = scenario !== 'absent-document';
  let composer = {}, view = {}, originalView = view, clickAt = null;
  const originalDocument = document;
  const root = { ownerDocument: document, isConnected: true,
    classList: { contains: name => name === 'z-(--z-onboarding)' } };
  const element = { ownerDocument: document, isConnected: true,
    closest: selector => selector === '[data-glass-opaque]' ? root : null };
  const context = vm.createContext({ document });
  const unwrap = value => value && Object.hasOwn(value, 'held') ? value.held : value;
  const evaluate = (callback, ...args) => {
    context.document = document;
    context.args = args.map(unwrap);
    return vm.runInContext(`(${callback.toString()})(...args)`, context);
  };
  const now = () => { lastNow = time; if (tick) time++; return lastNow; };
  const sample = () => ({ candidate: { x: 10, y: 10 } });
  const button = {
    async evaluate(callback) {
      if (callback === sample) {
        samples++;
        if (samples === 3 && scenario === 'document-before-action') document = {};
        if (samples === 3 && scenario === 'url-before-action') url = 'file:///foreign/dist/index.html';
        return sample();
      }
      return evaluate(callback, element);
    },
    async click(options) {
      clicks++; clickAt = lastNow;
      assert(options.timeout > 0 && options.timeout <= deadline);
      if (scenario === 'uncertain') throw Error('PRIVATE synthetic action uncertainty');
      visible = false;
      if (scenario === 'view-remount') { view = {}; composer = {}; }
      if (scenario === 'document-after-action') document = {};
    },
  };
  const choice = { count: async () => 1, isEnabled: async () => true,
    elementHandle: async () => button, evaluate: async (callback, held) => evaluate(callback, element, held) };
  // Browser ElementHandles carry object identity, never selector reacquisition.
  button.held = element;
  const cover = {
    async count() {
      if (scenario === 'absent-document') document = {};
      return visible ? 1 : 0;
    },
    isVisible: async () => visible,
    getByRole(role, options) {
      assert.equal(role, 'button'); assert.equal(options.name, "I'll choose a provider later");
      return choice;
    },
  };
  const composerLocator = { count: async () => 1,
    elementHandle: async () => ({ held: composer }),
    evaluate: async (callback, held) => evaluate(callback, composer, held) };
  const page = {
    url: () => url,
    evaluateHandle: async callback => ({ held: evaluate(callback) }),
    evaluate: async (callback, held) => evaluate(callback, held),
    locator(selector) {
      return selector.startsWith('[data-glass-opaque]') ? cover : composerLocator;
    },
  };
  const result = await prepare({ page, deadline, sample, now,
    owned: () => !(scenario === 'owner-before-action' && samples >= 3),
    stable: (first, second) => first.candidate && second.candidate ? second.candidate : null,
    pause: async ms => { time += ms; } });
  if (result.ready) { fills++; submits++; }
  return { result, clicks, clickAt, fills, submits, remounted: view !== originalView,
    documentRetained: document === originalDocument };
}

(async () => {
  const remounted = await trial('view-remount');
  assert.deepEqual(remounted.result, { ready: true, stage: 'dismissed' });
  assert.equal(remounted.clicks, 1); assert(remounted.remounted && remounted.documentRetained);
  assert.equal(remounted.fills, 1); assert.equal(remounted.submits, 1);
  for (const scenario of ['owner-before-action', 'document-before-action', 'url-before-action', 'absent-document']) {
    const result = await trial(scenario);
    assert.deepEqual(result.result, { ready: false, stage: 'blocked' }, scenario);
    assert.equal(result.clicks, 0, scenario); assert.equal(result.fills, 0); assert.equal(result.submits, 0);
  }
  for (const scenario of ['uncertain', 'document-after-action']) {
    const result = await trial(scenario);
    assert.deepEqual(result.result, { ready: false, stage: 'uncertain' }, scenario);
    assert.equal(result.clicks, 1, scenario); assert.equal(result.fills, 0); assert.equal(result.submits, 0);
  }
  // Sweep the original budget across dispatch rather than freezing one incidental
  // sequence of clock reads. Expiry must prevent construction of the click Promise.
  let before = false, after = false;
  for (let deadline = 120; deadline <= 230; deadline++) {
    const result = await trial('deadline', deadline, true);
    if (result.clicks) { after = true; assert(result.clickAt < deadline, 'no click may start at/after the cutoff'); }
    else { before = true; assert.equal(result.fills, 0); assert.equal(result.submits, 0); }
    assert(result.clicks <= 1);
  }
  assert(before && after, 'the controlled clock must straddle the action boundary');
  console.log('PASS: one pre-failure dismissal, same-document view remount, held ownership and deadline-safe dispatch');
})().catch(error => { console.error(error); process.exitCode = 1; });
