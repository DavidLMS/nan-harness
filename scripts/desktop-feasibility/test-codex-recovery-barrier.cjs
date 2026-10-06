'use strict';
const assert=require('node:assert/strict');
const {release}=require('./codex-recovery-barrier.cjs');
(async()=>{
 for(const scenario of ['ready','pending','partial','absent','malformed','symlink','public','duplicate','owner-lost','late']) {
  let now=0,writes=0,reads=0,guards=0;
  const io={writeFileSync(path,bytes,options){writes++;assert(path.endsWith('.retry-ready.private'));
    assert.equal(bytes,'ready\n');assert.deepEqual(options,{mode:0o600,flag:'wx'});
    if(scenario==='duplicate')throw Object.assign(Error('exists'),{code:'EEXIST'});},
   lstatSync(path){assert(path.endsWith('.retry-release.private'));reads++;
    if(scenario==='absent'||scenario==='pending'&&reads===1)throw Object.assign(Error('missing'),{code:'ENOENT'});
    return {isFile:()=>true,isSymbolicLink:()=>scenario==='symlink',mode:scenario==='public'?0o644:0o600,size:9};},
   readFileSync(){return scenario==='malformed'?'PRIVATE':scenario==='partial'&&reads===1?'relea':'released\n';}};
  const guard=async()=>{guards++;if(scenario==='late'&&guards===2)now=1000;return scenario!=='owner-lost'||guards===1;};
  const action=()=>release('/owned/request.private',200,guard,io,()=>now,async ms=>{now+=ms;});
  if(['ready','pending','partial'].includes(scenario)||scenario==='public'&&process.platform==='win32')await action();
  else await assert.rejects(action());
  assert.equal(writes,1);assert(now<=1000);
  if(scenario==='owner-lost'||scenario==='late')assert.equal(reads,0);
 }
 console.log('Codex recovery barrier: exact private acknowledgement, no replay, fresh custody and original deadline passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
