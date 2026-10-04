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
