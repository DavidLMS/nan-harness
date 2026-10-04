// Synthetic ownership proofs; macOS also checks one disposable child listener.
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
        assert.deepEqual(Array.from(args), ['-nP', '-a', '-iTCP:43210', '-sTCP:LISTEN', '-Fpfn']);
        return listing;
      }
      assert.equal(command, '/bin/ps');
      assert.deepEqual(Array.from(args).slice(0, 3), ['-o', 'ppid=', '-p']);
      return String(parents[args[3]] ?? 0);
    } };
  } };
  vm.runInNewContext(proof, context);
  const owned = context.ownedEndpoint();
  assert(calls <= 129);
  return details?{owned,reason:context.failure(),shape:context.failureDetails(),calls}:owned;
}
assert(trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 30, 30: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 1 }));
assert(!trial('p40\nf3\nn*:43210\n', { 40: 20 }));
assert(!trial('p40\nf3\nn[::1]:43210\n', { 40: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43211\n', { 40: 20 }));
assert(trial('p40\nf3\nn127.0.0.1:43210\np41\nf4\nn127.0.0.1:43210\n', { 40: 20, 41: 20 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 41, 41: 40 }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 'private-invalid' }));
assert(!trial('p40\nf3\nn127.0.0.1:43210\n', { 40: 20 }, true));
assert(!trial('n127.0.0.1:43210\n', {}));
assert(!trial('p40\nn127.0.0.1:43210\n', { 40: 20 }));
assert(!trial('p40\nfPRIVATE\nn127.0.0.1:43210\n', { 40: 20 }));
for (const [listing, reason, count, pids] of [
  ['p40\nfPRIVATE\nn127.0.0.1:43210\n', 'malformed-descriptor', 0, 0],
  ['p40\nn127.0.0.1:43210\n', 'missing-descriptor', 0, 0],
  ['p40\nf3\nxPRIVATE\nn127.0.0.1:43210\n', 'unexpected-field', 1, 1],
  ['p40\nf3\nn*:43210\n', 'endpoint-mismatch', 1, 1],
  [Array.from({length:5},(_,i)=>`p${40+i}\nf3\nn127.0.0.1:43210`).join('\n'), 'multiple-listeners', 5, 5],
]) {
  const result = trial(listing, {40:20,41:20}, false, true);
  assert.equal(result.owned, false);
  assert.equal(result.reason, 'listener-shape');
  assert.equal(result.shape.reason, reason);
  assert.equal(result.shape.listenerCount, count);
  assert.equal(result.shape.uniquePidCount, pids);
  assert.equal(result.calls, 1); // No ancestry or second native query after shape rejection.
  assert(!JSON.stringify(result).includes('PRIVATE'));
  assert(!JSON.stringify(result).includes('43210'));
}
for (const [listing, failure] of [['',false], ['p40\nf3\nn127.0.0.1:43210\n',false], ['',true]]) {
  assert.equal(trial(listing,{40:20},failure,true).shape, null);
}
const overflowListing = Array.from({length:4097}, (_,i)=>`p${i+40}\nf3\nn127.0.0.1:43210`).join('\n');
const overflowShape = trial(overflowListing, {}, false, true).shape;
assert.equal(overflowShape.reason, 'multiple-listeners');
assert.equal(overflowShape.listenerCount, null);
assert.equal(overflowShape.uniquePidCount, null);
console.log('Native macOS listener shape: closed same-query diagnostic passed');
const twoListeners = 'p40\nf3\nn127.0.0.1:43210\np41\nf4\nn127.0.0.1:43210\n';
assert(!trial(twoListeners, {40:20,41:1}));
assert(!trial(twoListeners + 'f99\n', {40:20,41:20}));
assert(!trial(twoListeners.replace('p41', 'p9007199254740993'), {40:20}));
assert(!trial(twoListeners.replace('f4', 'f9007199254740993'), {40:20,41:20}));
assert(!trial(twoListeners.replace('p41\nf4', 'p41\nfPRIVATE'), {40:20,41:20}));
assert(!trial(twoListeners.replace('p41\nf4', 'p41'), {40:20,41:20}));
assert(!trial(twoListeners.replace(/43210\n$/, '43211\n'), {40:20,41:20}));
assert.equal(trial(twoListeners.replace('p41','p40'), {40:20}, false, true).calls, 2);
function timedListeners(advance, callerDeadline) {
  let now = 1000;
  const calls = [];
  const context = {owner:'20',port:'43210',Date:{now:()=>now},
    process:{platform:'darwin'},require() { return {execFileSync(command,args,options) {
      assert(options.timeout > 0 && options.timeout <= 2000);
      calls.push({command,timeout:options.timeout});
      now += advance;
      return command === '/usr/sbin/lsof' ? twoListeners : '20';
    }}; }};
  vm.runInNewContext(proof, context);
  return {owned:context.ownedEndpoint(callerDeadline),calls,reason:context.failure()};
}
assert.equal(timedListeners(1000, undefined).owned, true);
assert.equal(timedListeners(3000, undefined).owned, false); // Same8s, not8s per PID.
const clipped = timedListeners(500, 2200);
assert.equal(clipped.owned, false);
assert.equal(clipped.calls.at(-1).timeout, 200);
assert.equal(timedListeners(0,1000).calls.length,0);
assert.equal(timedListeners(0,'PRIVATE').calls.length,0);
{
  let listenerCalls = 0, parentCalls = 0;
  const context = {owner:'20',port:'43210',process:{platform:'darwin'},require() {
    return {execFileSync(command,args) {
      if (command === '/usr/sbin/lsof') { listenerCalls++; return twoListeners; }
      parentCalls++;
      return listenerCalls === 2 && args[3] === '41' ? '1' : '20';
    }};
  }};
  vm.runInNewContext(proof, context);
  assert.equal(context.ownedEndpoint(), true);
  assert.equal(context.ownedEndpoint(), false);
  assert.equal(context.failure(), 'listener-unowned');
  assert.equal(listenerCalls, 2);
  assert.equal(parentCalls, 4); // Every distinct PID is reproved on each invocation.
}
console.log('Native macOS bounded owned listeners: all-owned and shared deadline passed');
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
  ['listener','listener-shape',false],
  ['listener','PRIVATE unknown',false],
]) {
  const facts={},calls=[];
  const shape={reason:'multiple-listeners',listenerCount:2,uniquePidCount:1};
  const native=part=>({failure:()=>reason,failureDetails:()=>reason==='listener-shape'?shape:null,
    descendant(_pid,deadline){assert.equal(deadline,123456);calls.push(part);if(component===part&&throwing)throw Error('PRIVATE');return component!==part;},
    ownedEndpoint(deadline){assert.equal(deadline,123456);calls.push(part);if(component===part&&throwing)throw Error('PRIVATE');return component!==part;}});
  const guard=vm.runInNewContext(`(()=>{${renderer.slice(guardStart,guardEnd)}return ownerGuard;})()`,
    {facts,totalDeadline:123456,connection:{launcherPid:40},rootProof:native('root'),ownership:native('listener')});
  if(throwing)assert.throws(guard);else assert.equal(guard(),false);
  assert.equal(calls.length,component==='root'?1:2);
  assert.equal(facts.nativeOwnershipFailure,reason==='PRIVATE unknown'?undefined:reason);
  assert.deepEqual(facts.nativeListenerShape,reason==='listener-shape'?shape:undefined);
  assert(!JSON.stringify(facts).includes('PRIVATE'));
}
console.log('Unix ownership failure diagnostic: same queries and throw semantics passed');

if (process.platform === 'darwin') {
  (async () => {
    const {spawn} = require('node:child_process');
    const child = spawn(process.execPath, ['-e',
      "const server=require('node:net').createServer();server.listen(0,'127.0.0.1',()=>process.send(server.address().port));process.on('disconnect',()=>server.close());"],
      {stdio: ['ignore', 'ignore', 'ignore', 'ipc']});
    try {
      const port = await new Promise((resolve, reject) => {
        const timeout = setTimeout(() => reject(Error('Synthetic listener startup timed out')), 5000);
        child.once('message', value => {clearTimeout(timeout); resolve(value);});
        child.once('error', () => {clearTimeout(timeout); reject(Error('Synthetic listener failed'));});
        child.once('exit', () => {clearTimeout(timeout); reject(Error('Synthetic listener exited'));});
      });
      assert(Number.isInteger(port) && port > 1 && port <= 65535);
      const owned = require('./endpoint-ownership.cjs').proof(String(process.pid), port);
      assert.equal(owned.descendant(child.pid), true);
      assert.equal(owned.ownedEndpoint(), true, 'Explicit lsof fields must prove the owned listener');
      const foreign = require('./endpoint-ownership.cjs').proof('1', port);
      assert.equal(foreign.ownedEndpoint(), false);
      assert.equal(foreign.failure(), 'listener-unowned');
      console.log('Native macOS synthetic listener: owned and foreign ancestry passed');
    } finally {
      child.kill('SIGKILL');
    }
  })().catch(() => {
    console.error('Native macOS synthetic listener contract failed');
    process.exitCode = 1;
  });
}

// Windows subprocesses consume the caller's original absolute budget.
{
  let now = 1000, calls = 0, timeout, late = false;
  const context = {process: {platform: 'win32'}, owner: '20', port: '43210',
    __dirname: '/owned', Date: {now: () => now}, require(name) {
      assert.equal(name, 'node:child_process');
      return {execFileSync(_command, _args, options) {
        calls++; timeout = options.timeout;
        if (late) now = 1200;
        return 'true';
      }};
    }};
  vm.runInNewContext(proof, context);
  assert.equal(context.ownedEndpoint(1200), true);
  assert.equal(timeout, 200);
  assert.equal(context.descendant(30, 1150), true);
  assert.equal(timeout, 150);
  for (const invalid of [NaN, Infinity, 1.5, '1200', null]) {
    assert.equal(context.ownedEndpoint(invalid), false);
  }
  assert.equal(calls, 2);
  assert.equal(context.ownedEndpoint(1000), false);
  assert.equal(calls, 2);
  late = true;
  assert.equal(context.ownedEndpoint(1200), false);
  assert.equal(calls, 3);
  now = 1000; late = false;
  assert.equal(context.windowsProof('endpoint', 43210, 20), true);
  assert.equal(timeout, 8000);
}
console.log('Windows original caller deadline: clipped, invalid, expired and late proofs passed');

{
  let now = 1000;
  const receipts = [];
  const sandbox = {exports: {}, __dirname: '/owned', Date: {now: () => now},
    process: {platform: 'win32', pid: 7, env: {GITHUB_ACTIONS: 'true',
      RUNNER_ENVIRONMENT: 'github-hosted', NANH_DESKTOP_QUALIFICATION_FACTS: '/facts'}},
    require(name) {
      if (name === 'node:fs') return {lstatSync: () => ({isDirectory: () => true}),
        writeFileSync: (_path, bytes) => receipts.push(JSON.parse(bytes)), renameSync: () => {}};
      if (name === 'node:path') return require(name);
      assert.equal(name, 'node:child_process');
      return {execFileSync: () => {now = 1200; return 'true';}};
    }};
  vm.runInNewContext(source, sandbox);
  assert.equal(sandbox.exports.proof('20', 43210).ownedEndpoint(1200), false);
  assert.equal(receipts.at(-1).category, 'transport-timeout');
  assert.equal(receipts.at(-1).categoryCounts.owned, 0);
  assert.equal(receipts.at(-1).categoryCounts['transport-timeout'], 1);
}
