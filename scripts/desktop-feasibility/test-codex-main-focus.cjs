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
 for(const scenario of ['settled','deadline','changed','query-failed','rejected']) {
  let now=0,activations=0,verifications=0,focus=false;
  const clockApi=vm.runInNewContext(source.slice(begin,end)+';focusCapturedMain',{Date:{now:()=>now},setTimeout});
  const held={page:{},key:'held'},proof=async()=>true;
  proof.requireDocumentFocus=()=>{};
  const native={prepare(){},activate(){activations++;focus=true;},verify(){
   verifications++;if(scenario==='query-failed')throw Error('PRIVATE');
   return scenario==='settled'&&verifications>2;
  },pending:()=>!['query-failed','rejected'].includes(scenario)};
  const identity=async()=>({key:scenario==='changed'&&verifications>0?'changed':'held',scope:{mainScope:true,focused:focus}});
  const result=await clockApi(held,proof,500,identity,(a,b)=>a.key===b.key,async ms=>{now+=ms;},null,native);
  assert.equal(result,scenario==='settled',scenario);assert.equal(activations,1,scenario);
  if(['query-failed','rejected','changed'].includes(scenario))assert.equal(verifications,1,scenario);
 }
 {
  let now=0,observations=0,activations=0;
  const clockApi=vm.runInNewContext(source.slice(begin,end)+';focusCapturedMain',{Date:{now:()=>now},setTimeout});
  const held={page:{},key:'held'},proof=async()=>true;
  const identity=async()=>({key:'held',scope:{mainScope:true,focused:activations===1}});
  const native={prepare(){},activate(){activations++;},verify:()=>false,pending:()=>true};
  const diagnostic={};
  const result=await clockApi(held,proof,500,identity,(a,b)=>a.key===b.key,async ms=>{now+=ms;},diagnostic,native,
    async()=>{observations++;return {reason:'mapping-observed',mappingObserved:true,inputAuthorized:false};});
  assert.equal(result,false);assert.equal(activations,1);assert.equal(observations,1);
  assert.equal(diagnostic.status,'deadline');assert.equal(diagnostic.nativePointObservation.inputAuthorized,false);
 }
 for(const scenario of ['focused-after-prepare','changed-after-prepare']) {
  let reads=0,activations=0;
  const held={page:{},key:'held'},proof=async()=>true,diagnostic={};
  proof.requireDocumentFocus=()=>{};
  const native={prepare(){},activate(){activations++;throw Error('closed');},verify:()=>true};
  const identity=async()=>({key:scenario==='changed-after-prepare'&&++reads===2?'changed':'held',
    scope:{mainScope:true,focused:scenario==='focused-after-prepare'?++reads>=2:false}});
  assert.equal(await api(held,proof,Date.now()+1000,identity,(a,b)=>a.key===b.key,
    async()=>{},diagnostic,native),false);
  assert.equal(diagnostic.focusSamples.beforePrepare,'unfocused');
  assert.equal(diagnostic.focusSamples.afterPrepare,scenario==='focused-after-prepare'?'focused':'unmeasured');
  assert.equal(diagnostic.focusSamples.afterActivation,'unmeasured');
  assert.equal(activations,scenario==='focused-after-prepare'?1:0);
 }
 console.log('held page focus fixtures PASS');
})().catch(e=>{console.error(e);process.exitCode=1});
