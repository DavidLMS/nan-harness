// Native ownership proof: no application or network listener is launched here.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-hermes.cjs`, 'utf8');
const proof = source.slice(source.indexOf('function descendant('), source.indexOf('function delay('));
function trial(listing, parents, failure = false) {
  let calls = 0;
  const context = { owner: '20', port: '43210', process: { platform: 'darwin' }, require(name) {
    assert.equal(name, 'node:child_process');
    return { execFileSync(command, args, options) {
      calls++;
      assert.equal(options.timeout, 2000);
      assert.equal(options.stdio[2], 'ignore');
      if (failure) throw new Error('private operational error');
      if (command === '/usr/sbin/lsof') {
        assert.deepEqual(Array.from(args), ['-nP', '-a', '-iTCP:43210', '-sTCP:LISTEN', '-Fpn']);
        return listing;
      }
      assert.equal(command, '/bin/ps');
      assert.deepEqual(Array.from(args).slice(0, 3), ['-o', 'ppid=', '-p']);
      return String(parents[args[3]] ?? 0);
    } };
  } };
  vm.runInNewContext(proof, context);
  const owned = context.ownedEndpoint();
  assert(calls <= 33);
  return owned;
}
assert(trial('p40\nn127.0.0.1:43210\n', { 40: 30, 30: 20 }));
assert(!trial('p40\nn127.0.0.1:43210\n', { 40: 1 }));
assert(!trial('p40\nn*:43210\n', { 40: 20 }));
assert(!trial('p40\nn[::1]:43210\n', { 40: 20 }));
assert(!trial('p40\nn127.0.0.1:43211\n', { 40: 20 }));
assert(!trial('p40\nn127.0.0.1:43210\np41\nn127.0.0.1:43210\n', { 40: 20, 41: 20 }));
assert(!trial('p40\nn127.0.0.1:43210\n', { 40: 41, 41: 40 }));
assert(!trial('p40\nn127.0.0.1:43210\n', { 40: 'private-invalid' }));
assert(!trial('p40\nn127.0.0.1:43210\n', { 40: 20 }, true));
assert(!trial('n127.0.0.1:43210\n', {}));
console.log('Native macOS endpoint ownership: guarded cases passed');
