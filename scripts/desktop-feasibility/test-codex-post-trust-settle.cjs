const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
const begin=source.indexOf('async function bindCorrelationMain('),end=source.indexOf('async function observeMainAux(',begin);
(async()=>{
 for(const scenario of ['settled','legacy','expired','identity','focus','guard']) {
  let now=0,reads=0,proofs=0;
  const bind=vm.runInNewContext(source.slice(begin,end)+';bindCorrelationMain',{
   require,Date:{now:()=>now},sameCorrelationIdentity:(a,b)=>a.id===b.id,heldMainGuard:()=>async()=>true});
  const held={id:1,page:{}},diagnostic={};
  const identity=async()=>{reads++;return {id:scenario==='identity'&&reads===2?2:1,scope:{mainScope:scenario!=='expired'&&reads>=3,focused:scenario!=='focus',counts:{}}}};
  const settle=async()=>{proofs++;return scenario!=='guard'||proofs<3};
  const result=await bind(held,{},()=>true,500,()=>{},identity,async ms=>{now+=ms},diagnostic,scenario==='legacy'?null:settle);
  assert.equal(!!result,scenario==='settled',scenario);
  assert(reads<=6,scenario);
  if(scenario==='legacy')assert.equal(reads,1);
 }
 console.log('post-trust passive settle fixtures PASS');
})().catch(e=>{console.error(e);process.exitCode=1});
