'use strict';
const assert = require('node:assert/strict');
const vm = require('node:vm');
const { observe } = require('./hermes-response-observation.cjs');

// Mirror the frozen ErrorCardHeadline structure. A closed Details error has a
// generic innerText headline and private textContent in its plain DIV leaf.
function trial(options = {}) {
  const prompt = 'PRIVATE_SYNTHETIC_PROMPT';
  const marker = 'PRIVATE_SYNTHETIC_MARKER';
  const privateValue = 'PRIVATE_SYNTHETIC_ERROR';
  const rect = () => ({ width: 100, height: 20 });
  const group = {};
  const user = { innerText: prompt, getBoundingClientRect: rect,
    closest: () => pair };
  const pair = {
    closest: () => options.noGroup ? null : group,
    querySelectorAll(selector) {
      assert.equal(selector, '[data-role="user"]');
      return options.foreignUser ? [{ ...user }] : [user];
    },
  };
  const leaf = { tagName: 'DIV', childElementCount: options.nestedLeaf ? 1 : 0,
    textContent: options.text ?? `ModuleNotFoundError: ${privateValue}` };
  const details = {
    children: [{ tagName: options.badSummary ? 'SPAN' : 'SUMMARY' }, leaf],
    closest: () => options.foreignDetails ? {} : alert,
  };
  const alert = {
    closest: () => options.foreignAlert ? {} : content,
    querySelectorAll(selector) {
      assert.equal(selector, ':scope > div > div > details');
      return options.noDetails ? [] : options.duplicateDetails ? [details, details] : [details];
    },
  };
  const content = {
    closest: () => options.foreignContent ? {} : assistant,
    querySelectorAll(selector) {
      assert.equal(selector, '[role="alert"]');
      return options.noAlert ? [] : options.duplicateAlerts ? [alert, alert] : [alert];
    },
  };
  const assistant = {
    innerText: options.visibleText ?? 'Generic failure. Details', getBoundingClientRect: rect,
    getAttribute(name) {
      assert.equal(name, 'data-slot');
      return options.foreignAssistantSlot ? 'other' : 'aui_assistant-message-root';
    },
    closest: () => options.foreignPair ? {} : pair,
    querySelectorAll(selector) {
      if (options.queryFails) throw Error(privateValue);
      assert.equal(selector, '[data-slot="aui_assistant-message-content"]');
      return options.duplicateContents ? [content, content] : [content];
    },
  };
  const earlier = { ...assistant, innerText: 'ImportError: PRIVATE_OLDER_REPLY',
    closest: () => ({}), querySelectorAll() { throw Error('Earlier turn was inspected'); } };
  const document = {
    querySelectorAll(selector) {
      if (selector === '[data-slot="composer-root"] [role="textbox"]') return [];
      if (selector === '[data-role="user"]') return options.duplicateUsers ? [user, user] : [user];
      assert.equal(selector, '[data-role="assistant"]');
      return [earlier, assistant, ...(options.duplicateAssistants ? [assistant] : [])];
    },
  };
  const result = vm.runInNewContext(`(${observe.toString()})`, {
    document, getComputedStyle: () => ({ visibility: 'visible', display: 'block' }),
  })({ prompt, marker, bindTurn: true });
  const closed = JSON.parse(JSON.stringify(result));
  assert(!JSON.stringify(closed).includes('PRIVATE_'));
  return closed;
}

const expected = trial();
assert.equal(expected.backendFailure, 'python-import-failure');
assert.equal(expected.responseVerified, false);
for (const [text, category] of [
  ['No inference provider configured', 'provider-unconfigured'],
  ['gateway is unavailable', 'backend-unavailable'],
  ['model not configured', 'invalid-model'],
  ['PermissionError: private value', 'permission-denied'],
  ['ConnectionError: private value', 'connection-failed'],
  ['ModuleNotFoundError and ConnectionError', 'multiple'],
  ['PRIVATE_UNCLASSIFIED_ERROR', 'unclassified'],
]) assert.equal(trial({ text }).backendFailure, category);

for (const option of ['noGroup', 'foreignUser', 'foreignAssistantSlot', 'foreignPair',
  'foreignContent', 'foreignAlert', 'foreignDetails', 'duplicateUsers', 'duplicateAssistants',
  'duplicateContents', 'duplicateAlerts', 'duplicateDetails', 'noAlert', 'noDetails',
  'badSummary', 'nestedLeaf', 'queryFails']) {
  const observed = trial({ [option]: true });
  assert.equal(observed.backendFailure, 'unclassified', option);
  delete observed.backendFailure;
  const unchanged = JSON.parse(JSON.stringify(expected)); delete unchanged.backendFailure;
  if (option === 'duplicateUsers') {
    unchanged.responseShape.exactUserCount = 2;
    unchanged.userTurnObserved = false;
  }
  if (option === 'duplicateAssistants') unchanged.assistantTurnCount = 3;
  assert.deepEqual(observed, unchanged, option);
}
for (const text of ['', 'A'.repeat(65537)]) assert.equal(trial({ text }).backendFailure, 'unclassified');
assert.equal(trial({ noDetails: true, visibleText: 'ModuleNotFoundError: ordinary reply' }).backendFailure,
  'unclassified');
assert.equal(trial({ text: 'PRIVATE_CURRENT_UNKNOWN', visibleText: 'ordinary reply' }).backendFailure,
  'unclassified');
console.log('Hermes current-turn hidden error diagnostics passed');
