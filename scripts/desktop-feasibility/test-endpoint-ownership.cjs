// Native ownership proof: no application or network listener is launched here.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/endpoint-ownership.cjs`, 'utf8');
const proof = source.slice(source.indexOf('function saveWindowsProof('), source.indexOf('return { ownedEndpoint'));
function trial(listing, parents, failure = false, details = false) {
  let calls = 0;
  const context = { owner: '20', port: '43210', process: { platform: 'darwin' }, require(name) {
    assert.equal(name, 'node:child_process');
    return { execFileSync(command, args, options) {
      calls++;
      assert.equal(options.timeout, 2000);
      assert.equal(options.stdio[2], 'ignore');
      if(failure&&!(failure==='parent'&&command==='/usr/sbin/lsof'))throw new Error('private operational error');
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
  return details?{owned,reason:context.failure(),calls}:owned;
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

// Exercise the actual module's shared counter and guarded writer without native calls.
{
  const receipts = [], results = ['listener-unavailable', 'true', 'parent-reused', 'true'];
  let nativeCalls = 0;
  const sandbox = {exports: {}, __dirname: '/owned', process: {platform: 'win32', pid: 7,
    env: {GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted',
      NANH_DESKTOP_QUALIFICATION_FACTS: '/owned-facts'}}, require(name) {
    if (name === 'node:fs') return {lstatSync: () => ({isDirectory: () => true}),
      writeFileSync: (_path, bytes, options) => {assert.equal(options.mode, 0o600); receipts.push(JSON.parse(bytes));},
      renameSync: () => {}};
    if (name === 'node:path') return require(name);
    assert.equal(name, 'node:child_process');
    return {execFileSync: () => {nativeCalls++; return results.shift();}};
  }};
  vm.runInNewContext(source, sandbox);
  const rootProof = sandbox.exports.proof('20', 43210);
  const listenerProof = sandbox.exports.proof('30', 43210);
  assert.equal(rootProof.descendant(30), false);
  assert.equal(listenerProof.ownedEndpoint(), true);
  assert.equal(rootProof.descendant(30), false);
  assert.equal(listenerProof.ownedEndpoint(), true);
  assert.equal(nativeCalls, 4); // Counting adds no native measurement.
  const last = receipts.at(-1);
  assert.equal(last.category, 'owned');
  assert.equal(last.categoryCounts.owned, 2);
  assert.equal(last.categoryCounts['listener-unavailable'], 1);
  assert.equal(last.categoryCounts['parent-reused'], 1);
  assert.equal(last.categoryCounts['query-failed'], 0);
  assert(Object.values(last.categoryCounts).every(count => Number.isInteger(count) && count >= 0 && count <= 4096));
  assert(!JSON.stringify(last).includes('owned-facts'));
  assert(!JSON.stringify(last).includes('43210'));
  for (let i = 0; i < 4100; i++) { results.push('true'); assert.equal(listenerProof.ownedEndpoint(), true); }
  assert.equal(receipts.at(-1).categoryCounts.owned, 4096);
  assert.equal(nativeCalls, 4104);
  sandbox.process.env.RUNNER_ENVIRONMENT = 'self-hosted';
  results.push('true');
  assert.equal(listenerProof.ownedEndpoint(), true);
  assert.equal(receipts.length, 4104); // Diagnostic policy cannot modify the proof verdict.
}
console.log('Native Windows proof receipt: earlier failures retained across proof objects');

for(const [listing,parents,failure,reason] of [
  ['p40\nf3\nn127.0.0.1:43210\n',{40:1},false,'listener-unowned'],
  ['p40\nf3\nn127.0.0.1:43210\n',{40:'PRIVATE malformed'},false,'ancestor-query'],
  ['p40\nf3\nn127.0.0.1:43210\n',{40:20},'parent','ancestor-query'],
  ['',{},false,'listener-unavailable'],
  ['p40\nn127.0.0.1:43210\n',{40:20},false,'listener-shape'],
  ['p40\nf3\nn*:43210\n',{40:20},false,'listener-shape'],
  ['',{},true,'listener-query'],
]) {
  const result=trial(listing,parents,failure,true);
  assert.equal(result.owned,false);assert.equal(result.reason,reason);
  assert(!JSON.stringify(result).includes('PRIVATE'));
}
const renderer=fs.readFileSync(`${__dirname}/observe-renderer.cjs`,'utf8');
const guardStart=renderer.indexOf('    const ownerGuard=()=>');
const guardEnd=renderer.indexOf('\n    };',guardStart)+7;
for(const [component,reason,throwing] of [
  ['root','ancestor-query',false],['listener','listener-unavailable',false],
  ['root','ancestor-query',true],['listener','listener-query',true],
  ['listener','PRIVATE unknown',false],
]) {
  const facts={},calls=[];
  const native=part=>({failure:()=>reason,
    descendant(){calls.push(part);if(component===part&&throwing)throw Error('PRIVATE');return component!==part;},
    ownedEndpoint(){calls.push(part);if(component===part&&throwing)throw Error('PRIVATE');return component!==part;}});
  const guard=vm.runInNewContext(`(()=>{${renderer.slice(guardStart,guardEnd)}return ownerGuard;})()`,
    {facts,connection:{launcherPid:40},rootProof:native('root'),ownership:native('listener')});
  if(throwing)assert.throws(guard);else assert.equal(guard(),false);
  assert.equal(calls.length,component==='root'?1:2);
  assert.equal(facts.nativeOwnershipFailure,reason==='PRIVATE unknown'?undefined:reason);
  assert(!JSON.stringify(facts).includes('PRIVATE'));
}
console.log('Unix ownership failure diagnostic: same queries and throw semantics passed');
