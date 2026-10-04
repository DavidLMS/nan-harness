const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
const begin=source.indexOf('async function focusCapturedMain('),end=source.indexOf('function publishCodexBinding(',begin);
const api=vm.runInNewContext(source.slice(begin,end)+';focusCapturedMain',{Date,setTimeout});
(async()=>{
 for(const scenario of ['success','focused','throw','late','unfocused','replacement','owner']) {
  let activations=0,reads=0,proofs=0,focus=scenario==='focused';
  const deadline=Date.now()+1000;
  const page={bringToFront:async()=>{activations++;if(scenario==='throw')throw Error('private');focus=scenario!=='unfocused';}};
  const held={page,key:'held'};
  const proof=async()=>{proofs++;return scenario!=='owner'||proofs<3};
  proof.requireDocumentFocus=()=>{};
  const identity=async()=>{reads++;return {key:scenario==='replacement'&&reads>1?'new':'held',scope:{mainScope:true,focused:focus}}};
  const result=await api(held,proof,scenario==='late'?Date.now()-1:deadline,identity,(a,b)=>a.key===b.key,async()=>{});
  assert.equal(result,['success','focused'].includes(scenario),scenario);
  assert.equal(activations,scenario==='focused'||scenario==='late'?0:1,scenario);
 }
 console.log('held page focus fixtures PASS');
})().catch(e=>{console.error(e);process.exitCode=1});
