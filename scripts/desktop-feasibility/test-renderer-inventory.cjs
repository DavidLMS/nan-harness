// Read-only public startup headings reduce to closed enums, without a browser.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-renderer.cjs`, 'utf8');
const start = source.indexOf('const counts = await page.evaluate(') + 'const counts = await page.evaluate('.length;
const end = source.indexOf('}, app);', start) + 1;
function trial(headings, app = 'pen-desktop') {
  const nodes = headings.map(textContent => ({textContent, isConnected: true,
    getBoundingClientRect: () => ({width: 10, height: 10})}));
  const read = vm.runInNewContext(`(${source.slice(start, end)})`, {
    document: {readyState: 'complete', body: {},
      querySelectorAll: selector => selector === 'h1' || selector === '*' ? nodes : []},
    location: {href: 'pen:synthetic', protocol: 'pen:'},
    getComputedStyle: () => ({visibility: 'visible'}),
  });
  const facts = read(app);
  const output = JSON.stringify(facts);
  for (const heading of headings) assert(!output.includes(heading));
  assert(!output.includes('synthetic'));
  return facts.startupScreen;
}
assert.equal(trial(['Hardware acceleration unavailable']), 'gpu-unavailable');
assert.equal(trial(['Failed to start pen.dev']), 'startup-failed');
assert.equal(trial(['PRIVATE_UNKNOWN_HEADING']), 'other');
assert.equal(trial(['Hardware acceleration unavailable', 'PRIVATE']), 'other');
assert.equal(trial(['Hardware acceleration unavailable'], 'chatgpt-desktop'), 'unmeasured');
console.log('Renderer inventory: closed startup headings passed');
