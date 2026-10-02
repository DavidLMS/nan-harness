// Read-only public startup headings reduce to closed enums, without a browser.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-renderer.cjs`, 'utf8');
const timingStart = source.indexOf('function onboardingTrial(');
const timingEnd = source.indexOf('async function run()', timingStart);
const timing = vm.runInNewContext(`(() => { ${source.slice(timingStart, timingEnd)}
  return {onboardingTrial, onboardingDeadline}; })()`);
const hosted = {GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted',
  RUNNER_OS: 'Windows', NANH_CODEX_PUBLIC_ONBOARDING: 'engineering'};
assert.equal(timing.onboardingTrial('chatgpt-desktop', 'win32', hosted), true);
for (const [app, platform, env] of [
  ['claude-desktop', 'win32', hosted], ['chatgpt-desktop', 'darwin', hosted],
  ['chatgpt-desktop', 'win32', {...hosted, RUNNER_ENVIRONMENT: 'self-hosted'}],
  ['chatgpt-desktop', 'win32', {...hosted, NANH_CODEX_PUBLIC_ONBOARDING: 'unknown'}],
]) assert.equal(timing.onboardingTrial(app, platform, env), false);
// An exhausted startup clock cannot consume the public action budget, while
// the original total observation deadline still limits late attachment.
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 36000), 60000);
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 10000), 35000);
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 61000), 60000);
assert.equal(timing.onboardingDeadline(false, 25000, 25000, 26000), 25000);
const start = source.indexOf('const counts = await page.evaluate(') + 'const counts = await page.evaluate('.length;
const end = source.indexOf('}, app);', start) + 1;
function trial(headings, app = 'pen-desktop', bodyText = '', roleCount = 0) {
  const nodes = headings.map(textContent => ({textContent, isConnected: true,
    getBoundingClientRect: () => ({width: 10, height: 10})}));
  const read = vm.runInNewContext(`(${source.slice(start, end)})`, {
    document: {readyState: 'complete', body: {innerText: bodyText},
      querySelectorAll: selector => selector === 'h1' || selector === '*' ? nodes
        : selector === 'input[type="radio"][name="conversational-onboarding-inline-role"]'
          ? Array.from({length: roleCount}, () => ({textContent: 'PRIVATE_ROLE', isConnected: true,
              getBoundingClientRect: () => ({width: 1, height: 1})})) : []},
    location: {href: 'pen:synthetic', protocol: 'pen:'},
    getComputedStyle: () => ({visibility: 'visible'}),
  });
  const facts = read(app);
  const output = JSON.stringify(facts);
  for (const heading of headings) assert(!output.includes(heading));
  if (bodyText) assert(!output.includes(bodyText));
  assert(!output.includes('synthetic'));
  assert(!output.includes('PRIVATE_ROLE'));
  assert.equal(facts.onboardingCounts.roleRadios, roleCount);
  return facts.startupScreen;
}
assert.equal(trial(['Hardware acceleration unavailable']), 'gpu-unavailable');
assert.equal(trial(['Failed to start pen.dev']), 'startup-failed');
assert.equal(trial(['PRIVATE_UNKNOWN_HEADING']), 'other');
assert.equal(trial(['Hardware acceleration unavailable', 'PRIVATE']), 'other');
assert.equal(trial(['Hardware acceleration unavailable'], 'chatgpt-desktop'), 'unmeasured');
const connectionError = 'Something went wrong connecting to the Codex CLI. Try restarting';
assert.equal(trial([], 'chatgpt-desktop', connectionError), 'cli-connection-failed');
assert.equal(trial([], 'chatgpt-desktop', 'PRIVATE_UNKNOWN_ERROR'), 'unmeasured');
assert.equal(trial([], 'claude-desktop', connectionError), 'unmeasured');
assert.equal(trial([], 'chatgpt-desktop', '', 11), 'unmeasured');
console.log('Renderer inventory: closed startup headings passed');
