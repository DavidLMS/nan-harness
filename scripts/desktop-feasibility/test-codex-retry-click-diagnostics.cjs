'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const dom=require('./codex-dom.cjs'),{candidate}=require('./codex-onboarding.cjs'),spotlight=require('./codex-spotlight.cjs');
const source=fs.readFileSync(__dirname+'/codex-dom.cjs','utf8');
const start=source.indexOf('async function ordinaryClick('),end=source.indexOf('function directCDPPolicy(',start);
const sample=()=>({rect:[0,0,20,20],points:[{x:10,y:10}],testedViewportCount:9});
async function refusalTrial(scenario,observerThrows=false) {
  let now=0,guards=0,samples=0,clicks=0,attempts=0,disposed=0,refusals=0,phase;
  let receipt;
  const click=vm.runInNewContext(source.slice(start,end)+';ordinaryClick',{
    Date:{now:()=>now},pause:async ms=>{now+=ms;},candidate,sampleEditor:dom.sampleEditor});
  const handle={evaluate:async()=>{
    samples++;const value=sample();
    if(scenario==='first-blocked'&&samples===1)return {blocked:'pointer-disabled'};
    if(scenario.startsWith('first-blocked:')&&samples===1)return {blocked:scenario.slice('first-blocked:'.length)};
    if(scenario==='second-blocked'&&samples===2)return {blocked:'foreign-overlay'};
    if(scenario==='first-hit'&&samples===1)value.points=[];
    if(scenario==='second-hit'&&samples===2)value.points=[];
    if(scenario==='stationary-hit'&&samples===2)value.points=[{x:2,y:2}];
    if(scenario==='final-changed'&&samples===3)value.rect[0]=1;
    if(scenario==='final-point')value.points=samples===3?[{x:2,y:2}]:[{x:1,y:1},{x:2,y:2}];
    return value;
  },click:async()=>{clicks++;if(scenario==='post-deadline')now=1000;},dispose:async()=>{disposed++;}};
  const locator={count:async()=>scenario==='count'?2:1,isEnabled:async()=>scenario!=='disabled',
    elementHandle:async()=>scenario==='missing'?null:handle,evaluate:async()=>scenario!=='replaced'};
  const guard=async()=>{
    guards++;
    if(scenario==='admission-deadline'||scenario==='candidate-deadline'&&guards===3)now=1000;
    return !(scenario==='admission-guard'&&guards===1||scenario==='revalidate-guard'&&guards===2
      ||scenario==='candidate-guard'&&guards===3||scenario==='dispatch-guard'&&guards===4);
  };
  const result=await click(locator,guard,1000,()=>{attempts++;},scenario==='post-guard'?async()=>false:guard,
    value=>{phase=value;},value=>{refusals++;receipt=value;if(observerThrows)throw Error('PRIVATE observer');});
  return {result,receipt,phase,guards,samples,clicks,attempts,disposed,refusals};
}
function hitQueryCounts(covered=false,offscreen=false) {
  let queries=0;
  const button={tagName:'BUTTON',isConnected:true,disabled:false,readOnly:false,parentElement:null,
    getAttribute:()=>null,closest:()=>null,contains:()=>false,clientLeft:0,clientTop:0,clientWidth:20,clientHeight:20,
    getBoundingClientRect:()=>({left:offscreen?-30:10,top:10,width:20,height:20})};
  const document={querySelectorAll:()=>[],elementFromPoint:()=>{queries++;return covered?{}:button;}};button.ownerDocument=document;
  const value=vm.runInNewContext(`(${dom.sampleEditor})(button,'button')`,{button,document,innerWidth:100,innerHeight:100,
    getComputedStyle:()=>({display:'block',visibility:'visible',pointerEvents:'auto'})});
  return {value,queries};
}
async function runTurnGuardFailure(cached,throws=false) {
  let samples=0,clicks=0,handoffs=0,snapshots=0,disposed=0;
  const handle={evaluate:async()=>{samples++;return sample();},click:async()=>{clicks++;},dispose:async()=>{disposed++;}};
  const retry={count:async()=>1,isEnabled:async()=>true,innerText:async()=> 'Retry',elementHandle:async()=>handle,evaluate:async()=>true};
  const user={count:async()=>1,evaluate:async()=>true,locator:()=>({getByRole:()=>retry})};
  user.filter=()=>user;
  const page={evaluate:async fn=>fn.name==='codingScope'?true:{userCount:1,assistantCount:0,responseVerified:false},
    evaluateHandle:async()=>({dispose:async()=>{disposed++;}}),
    locator:selector=>selector.includes('ProseMirror')?{count:async()=>1}:user};
  const result=await dom.runTurn(page,async()=>samples<2,
    {action:'retry',purpose:'response',prompt:'Check the expected provider failure',expectedMarker:'NAN_CHECK_SYNTHETIC_RECOVERY'},
    Date.now()+5000,async()=>{handoffs++;},()=>{snapshots++;if(throws)throw Error('PRIVATE snapshot');return cached;});
  assert.equal(samples,2);assert.equal(clicks,0);assert.equal(handoffs,0);assert.equal(disposed,2);assert.equal(snapshots,1);
  assert.equal(result.errorCategory,'action-uncertain');assert.equal(result.retryClickPhase,'sample-second');
  assert.equal(result.retryAttempted,false);assert.equal(result.retryCompleted,false);
  assert.equal(result.retryClickRefusal.reason,'guard-rejected');
  if(throws)assert.equal(result.retryGuardSpotlight,undefined);else assert.deepEqual(result.retryGuardSpotlight,cached);
  return result;
}
(async()=>{
  const cases=[['admission-guard','guard-rejected','admission'],['admission-deadline','deadline','admission'],
    ['count','control-count','admission'],['disabled','control-disabled','admission'],['missing','missing-handle','capture'],
    ['first-blocked','sample-blocked','sample-first'],['revalidate-guard','guard-rejected','revalidate'],
    ['replaced','control-replaced','revalidate'],['second-blocked','sample-blocked','sample-second'],
    ['first-hit','first-hit-missing','sample-second'],['second-hit','second-hit-missing','sample-second'],
    ['stationary-hit','stationary-hit-changed','sample-second'],['candidate-guard','guard-rejected','sample-second'],
    ['candidate-deadline','deadline','sample-second'],['final-changed','final-sample-changed','sample-final'],
    ['final-point','final-point-lost','sample-final'],['dispatch-guard','guard-rejected','sample-final'],
    ['post-guard','guard-rejected','post-guard'],['post-deadline','deadline','post-guard']];
  const receipts=[];
  for(const [scenario,reason,phase] of cases) {
    const result=await refusalTrial(scenario);assert.equal(result.result,false,scenario);
    assert.equal(result.receipt.reason,reason,scenario);assert.equal(result.phase,phase,scenario);assert.equal(result.refusals,1);
    assert.equal(result.attempts,scenario.startsWith('post-')?1:0);assert.equal(result.clicks,result.attempts);
    assert.equal(result.disposed,['admission','capture'].includes(phase)?0:1);
    assert(result.receipt.remainingMs>=0&&result.receipt.remainingMs<=1000);
    if(reason==='deadline')assert.equal(result.receipt.remainingMs,0);
    assert(!JSON.stringify(result.receipt).includes('rect'));assert(!JSON.stringify(result.receipt).includes('points'));
    receipts.push({phase,...result.receipt});
  }
  const second=await refusalTrial('second-blocked');assert.equal(second.receipt.first.ownedHitCount,1);
  assert.equal(second.receipt.second.blocked,'foreign-overlay');assert.equal(second.receipt.second.ownedHitCount,null);
  assert.equal(second.receipt.second.testedViewportCount,null);assert.equal(second.receipt.final,null);
  for(const blocked of ['unsupported-control','detached-or-inert','disabled','hidden','foreign-overlay','pointer-disabled','ancestor-limit']) {
    const result=await refusalTrial(`first-blocked:${blocked}`);assert.equal(result.receipt.first.blocked,blocked);
    assert.equal(result.receipt.first.ownedHitCount,null);assert.equal(result.receipt.first.testedViewportCount,null);
    assert.equal(result.samples,1);assert.equal(result.clicks,0);receipts.push({phase:result.phase,...result.receipt});
  }
  const privateValue=await refusalTrial('first-blocked:PRIVATE_BLOCKED_VALUE');
  assert.equal(privateValue.receipt.first,null);assert(!JSON.stringify(privateValue.receipt).includes('PRIVATE_BLOCKED_VALUE'));
  const guarded=await refusalTrial('candidate-guard');assert.equal(guarded.guards,3);assert.equal(guarded.samples,2);
  const advisory=await refusalTrial('second-blocked',true);assert.equal(advisory.result,false);assert.equal(advisory.clicks,0);
  const success=await refusalTrial('success');assert.equal(success.result,true);assert.equal(success.refusals,0);
  assert.equal(success.guards,5);assert.equal(success.samples,3);assert.equal(success.clicks,1);assert.equal(success.attempts,1);
  for(const [covered,offscreen] of [[false,false],[true,false],[false,true]]) {
    const {value,queries}=hitQueryCounts(covered,offscreen);assert.equal(value.testedViewportCount,queries);
    assert.equal(queries,offscreen?0:9);assert.equal(value.points.length,covered||offscreen?0:9);
  }
  const verified={sourceVersion:spotlight.VERSION,rendererSha256:spotlight.RENDERER,status:'verified',failure:null,
    stableSamples:2,inputCount:1,listCount:1,editableCount:1,toastAnnouncerCount:1};
  const rejected={...verified,status:'rejected',failure:'foreign-controls',toastAnnouncerCount:null,rejectionCheck:'toast-announcer'};
  const actual=[await runTurnGuardFailure(verified),await runTurnGuardFailure(rejected),await runTurnGuardFailure(null,true)];
  if(process.argv[2])fs.writeFileSync(process.argv[2],JSON.stringify({ordinary:receipts,actual})+'\n',{mode:0o600});
  console.log('PASS: exact cached Retry refusal, existing hit-query counts, final guard/deadline discrimination, no input replay and advisory snapshot failure');
})().catch(error=>{console.error(error);process.exitCode=1;});
