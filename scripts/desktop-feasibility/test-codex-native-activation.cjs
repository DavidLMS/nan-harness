// Pure private wire and action consumption; no processes/files/windows are queried.
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'codex-native-activation.cjs'),'utf8');
let symlink=false;
const context={module:{exports:{}},process:{pid:30},Buffer,require(name){
  if(name==='node:fs')return {realpathSync:p=>p,lstatSync:()=>({isFile:()=>true,isSymbolicLink:()=>symlink})};
  if(name==='node:path')return path;
  if(name==='node:child_process')return {execFileSync(){throw Error('must be mocked');}};
  throw Error('unexpected dependency');
}};
vm.runInNewContext(source,context);
const {binding,controller}=context.module.exports;
for(const bad of ['','1 2 0 0 600 400 1 0 1000','1 2 0 0 NaN 400 1 0 1000\n',
  '1 2 0 0 600 400 0 0 1000\n','1 2 0 0 600 400 1 0 1000 PRIVATE\n'])assert.throws(()=>binding(bad));
const config={helper:'/private/helper',executable:'/public/Codex',cutoffNanos:'9000000000'};
for(const scenario of ['ready','expired','late','action-error','malformed']) {
  let now=1000,calls=[];
  const run=(_helper,args,options)=>{
    assert.deepEqual(Array.from(args),['--codex-activate-main']);
    assert.equal(options.timeout,2000-now);assert.equal(options.stdio[2],'ignore');
    const tokens=options.input.trimEnd().split(' ');calls.push(tokens[0]);
    assert.equal(tokens[1],'30');assert.equal(tokens[2],'20');assert.equal(tokens[3],'25');
    assert.equal(tokens[5],Buffer.from('/public/Codex').toString('hex'));
    if(tokens[0]==='prepare')return scenario==='malformed'?'PRIVATE\n':'42 100 10 20 600 400 500 0 1000000000\n';
    assert(BigInt(tokens[4])<=1998000000n);
    if(scenario==='late')now=2000;
    if(scenario==='action-error')throw Error('PRIVATE transport uncertainty');
    return tokens[0]==='activate'?'activated\n':'verified\n';
  };
  const native=controller(config,20,25,2000,run,()=>now);
  if(scenario==='malformed'){assert.throws(()=>native.prepare());assert.deepEqual(calls,['prepare']);continue;}
  native.prepare();
  if(scenario==='expired')now=2000;
  if(scenario==='ready'){native.activate();assert.equal(native.verify(),true);}
  else assert.throws(()=>native.activate());
  assert.throws(()=>native.activate());
  assert.equal(calls.filter(v=>v==='activate').length,scenario==='expired'?0:1);
}
symlink=true;assert.throws(()=>controller(config,20,25,2000));
console.log('Codex native activation private framing, cutoff, retained binding and consumed action passed');

symlink=false;
for(const boundary of ['request','cg-inventory-before','ax-main-before','cg-inventory-after','ax-main-after','identity','trust','PRIVATE']) {
  let calls=0;
  const native=controller(config,20,25,2000,()=>{
    calls++;const error=Error('private transport');error.stdout=`activation-rejected ${boundary}\n`;throw error;
  },()=>1000);
  assert.throws(()=>native.prepare());
  assert.equal(native.failure(),boundary==='PRIVATE'?null:boundary);
  assert.equal(calls,1);
}

for(const [line,expected] of [
 ['activation-rejected cg-inventory-before candidates-missing 0 2 1\n',true],
 ['activation-rejected cg-inventory-before candidates-ambiguous 2 0 0\n',true],
 ['activation-rejected cg-inventory-before geometry 0 0 0\n',true],
 ['activation-rejected cg-inventory-before PRIVATE 0 0 0\n',false],
 ['activation-rejected trust geometry 0 0 0\n',false],
 ['activation-rejected cg-inventory-before geometry 1025 0 0\n',false],
 ['activation-rejected cg-inventory-before geometry 0 0 0\nPRIVATE',false]]) {
 const native=controller(config,20,25,2000,()=>{const e=Error('private');e.stdout=line;throw e;},()=>1000);
 assert.throws(()=>native.prepare());
 assert.equal(native.inventoryFailure()!==null,expected);
}

for(const phase of ['activate','verify'])for(const boundary of ['app-unavailable','app-unfocused','foreground-unfocused',
 'focused-window-query','focused-window-type','focused-window-identity','app-activate','raise','deadline','PRIVATE']) {
 let calls=[];
 const native=controller(config,20,25,2000,(_h,_a,options)=>{
  const current=options.input.split(' ')[0];calls.push(current);
  if(current==='prepare')return '42 100 10 20 600 400 500 0 1000000000\n';
  if(current!==phase)return 'activated\n';
  const e=Error('PRIVATE');e.stdout=`activation-rejected ${boundary}\n`;throw e;
 },()=>1000);
 native.prepare();
 if(phase==='verify')native.activate();
 assert.throws(()=>phase==='activate'?native.activate():native.verify());
 const failure=native.actionFailure();assert.equal(failure!==null,boundary!=='PRIVATE');
 if(failure){assert.equal(failure.phase,phase==='activate'?'activation':'verification');assert.equal(failure.boundary,boundary);}
 assert.throws(()=>native.activate()); // A diagnostic cannot replay consumed activation.
 assert.equal(calls.filter(x=>x==='activate').length,1);
}

for(const result of ['pending-external-stack\n','PRIVATE\n']) {
 let calls=[],verified=false;
 const native=controller(config,20,25,2000,(_h,_a,options)=>{
  const phase=options.input.split(' ')[0];calls.push(phase);
  if(phase==='prepare')return '42 100 10 20 600 400 500 0 1000000000\n';
  if(phase==='activate')return 'activated\n';
  if(verified)return 'verified\n';
  return result;
 },()=>1000);
 native.prepare();native.activate();
 if(result==='pending-external-stack\n') {
  assert.equal(native.verify(),false);assert.equal(native.pending(),true);
  verified=true;assert.equal(native.verify(),true);assert.equal(native.pending(),false);
 } else {assert.throws(()=>native.verify());assert.equal(native.pending(),false);}
 assert.throws(()=>native.activate());assert.equal(calls.filter(x=>x==='activate').length,1);
}

for(const [line,accepted] of [
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 1 0 0 0 0 0 0 0 0\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 0 1\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 1 0 0 0 0 0 0 0 1\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 0 0\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 0\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 PRIVATE 0 0 0 0 0 0 0 1\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 1 0 1 0 0\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 1 0 0 0 0\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 1 0 1 1 0\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 1 0 1 0\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1 0 0 0 0 0 0 0 1 0 PRIVATE 0 0\n',false],
 ['pending-external-stack after 0 1 0 1\n',true],
 ['pending-external-stack after 0 1 0 1 1 0 0 0\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 0 0\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 1\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 0 0 0\n',true],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 0 1 0\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 2\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1 0\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1 1 1\n',false],
 ['pending-external-stack after 0 1 0 1 0 0 0 1\n',true],
 ['pending-external-stack after 0 1 0 1 1 1 0 0\n',false],
 ['pending-external-stack after 0 1 0 1 1 0 0\n',false],
 ['pending-external-stack before 1 0 0 1\n',true],
 ['pending-external-stack after 0 0 0 1\n',false],
 ['pending-external-stack after 1024 1 0 1\n',false],
 ['pending-external-stack after 0 1 0 0\n',false],
 ['pending-external-stack PRIVATE 0 1 0 1\n',false],
 ['pending-external-stack after 0 1 0 1\nPRIVATE',false]]) {
 let calls=[];
 const native=controller(config,20,25,2000,(_helper,_args,options)=>{
  const phase=options.input.split(' ')[0];calls.push(phase);
  return phase==='prepare'?'42 100 10 20 600 400 500 0 1000000000\n':phase==='activate'?'activated\n':line;
 },()=>1000);
 native.prepare();native.activate();
 if(accepted) {
  assert.equal(native.verify(),false);assert.equal(native.pending(),true);
  const stack=native.pendingStack();assert.equal(stack.displayContained,true);
  assert.equal(stack.normalOverlapCount+stack.elevatedOverlapCount+stack.lowerOverlapCount,1);
 } else {assert.throws(()=>native.verify());assert.equal(native.pendingStack(),null);}
 assert.equal(calls.filter(v=>v==='activate').length,1);
}
