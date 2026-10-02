// Native ownership proof: no application or network listener is launched here.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/endpoint-ownership.cjs`, 'utf8');
const proof = source.slice(source.indexOf('function saveWindowsProof('), source.indexOf('return { ownedEndpoint'));
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
assert(trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 30, 30: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 1 }));
assert(!trial('p40\nf3\nn*:43210\n', { 40: 20 }));
assert(!trial('p40\nf3\nn[::1]:43210\n', { 40: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43211\n', { 40: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\np41\nf4\nn127.0.0.1:43210\n', { 40: 20, 41: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 41, 41: 40 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 'private-invalid' }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 20 }, true));
assert(!trial('n127.0.0.1:43210\n', {}));
assert(!trial('p40\nn127.0.0.1:43210\n', { 40: 20 }));
assert(!trial('p40\nfPRIVATE\nn127.0.0.1:43210\n', { 40: 20 }));
console.log('Native macOS endpoint ownership: guarded cases passed');

for (const result of ['true', 'false', 'true\n', 'private unexpected value']) {
  let calls = 0;
  const context = { process: { platform: 'win32' }, owner: '20', port: '43210', __dirname: '/owned',
    require(name) {
      assert.equal(name, 'node:child_process');
      return { execFileSync(command, args, options) {
        calls++;
        assert.equal(command, 'pwsh');
        assert.equal(args[2], '-File');
        assert.equal(args[3], '/owned/endpoint-owner.ps1');
        assert.equal(args[4], 'endpoint');
        assert.equal(args[5], '43210');
        assert.equal(args[6], '20');
        assert.equal(options.timeout, 8000);
        return result;
      } };
    } };
  vm.runInNewContext(proof, context);
  assert.equal(context.ownedEndpoint(), result === 'true');
  assert.equal(calls, 1);
  context.port = '43210; arbitrary-private-command';
  assert(!context.ownedEndpoint());
  assert.equal(calls, 1);
}
console.log('Native Windows proof transport: guarded cases passed');

for (const result of ['true', 'true\n', 'query-failed']) {
  const context = { process: { platform: 'win32', env: { GITHUB_ACTIONS: 'true',
    RUNNER_ENVIRONMENT: 'github-hosted', FEASIBILITY_WINDOWS_PROOF_PYTHON: 'C:\\owned\\python.exe' } },
    owner: '20', port: '43210', __dirname: 'C:/owned', require(name) {
      assert.equal(name, 'node:child_process');
      return { execFileSync(command, args, options) {
        assert.equal(command, 'C:\\owned\\python.exe');
        assert.deepEqual(Array.from(args), ['C:/owned/endpoint-owner-windows.py', 'endpoint', '43210', '20']);
        assert.equal(options.timeout, 8000);
        assert.equal(options.stdio[2], 'ignore');
        return result;
      } };
    } };
  vm.runInNewContext(proof, context);
  assert.equal(context.ownedEndpoint(), result === 'true');
}
console.log('Native Windows Win32 transport: guarded cases passed');
