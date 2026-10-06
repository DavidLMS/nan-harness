const assert=require('node:assert/strict');
const {parseNative,observe}=require('./codex-point-observation.cjs');
const good='point-observation mapping-observed 1 1 1 1 1 1 1 1 1 10 20 600 400\n';
const parsed=parseNative(good);
assert.equal(parsed.facts.mappingObserved,true);assert.equal(parsed.facts.inputAuthorized,false);
assert(!JSON.stringify(parsed.facts).includes('600'));
for(const bad of [good+'PRIVATE',good.replace('mapping-observed','PRIVATE'),
  good.replace(' 1 1 1 1 1 1 1 1 1 ',' 1 1 1 1 1 1 1 1 0 '),
  good.replace('600','NaN'),good.replace('600','-1'),good.replace('1 1 1','2 1 1')])
  assert.throws(()=>parseNative(bad));
for(const cause of ['point-occluded','stack-unavailable','metadata-invalid','held-window-missing','held-window-changed']) {
  const wire=good.replace('mapping-observed',cause).replace('1 1 1 1 1 1 1 1 1','1 1 1 1 1 0 1 1 1');
  const facts=parseNative(wire).facts;
  assert.equal(facts.reason,cause);assert.equal(facts.nativePointClear,false);
  assert.equal(facts.inputAuthorized,false);assert.equal(facts.mappingObserved,false);
  assert(!JSON.stringify(facts).includes('600'));
  assert.throws(()=>parseNative(good.replace('mapping-observed',cause)));
  assert.throws(()=>parseNative(wire.replace('1 1 1 1 1 0 1 1 1','1 1 1 0 1 0 1 1 1')));
}
const held={url:'app://-/index.html',target:'PRIVATE-target',frame:'PRIVATE-frame',loader:'PRIVATE-loader',
  frameUrl:'app://-/index.html',fragment:''};
const layout={clientWidth:600,clientHeight:400,pageX:0,pageY:0};
const visual={...layout,offsetX:0,offsetY:0,scale:1};
async function run(scenario) {
  let sends=0,pointCalls=0,clock=0,layouts=0;
  const session={async send(method) {
    sends++;
    if(scenario==='deadline')clock=100;
    if(method==='Target.getTargetInfo')return {targetInfo:{targetId:held.target,url:held.url}};
    if(method==='Page.getFrameTree')return {frameTree:{frame:{id:held.frame,loaderId:scenario==='loader-change'?'PRIVATE-other':held.loader,url:held.frameUrl},
      ...(scenario==='child-frame'?{childFrames:[{}]}:{})}};
    if(method==='Page.getLayoutMetrics') {
      layouts++;
      const l=scenario==='metrics-change'&&layouts===2?{...layout,clientWidth:601}:layout;
      const v=scenario==='scale'?{...visual,scale:2}:{...visual,clientWidth:l.clientWidth};
      return scenario==='metrics-missing'?{}:{cssLayoutViewport:l,cssVisualViewport:v};
    }
    throw Error('PRIVATE error');
  }};
  const native={pointObserve(css,url) {
    pointCalls++;assert.deepEqual(css,[600,400,300,200]);assert.equal(url,held.frameUrl);
    if(scenario==='native-error')throw Error('PRIVATE paths');
    if(scenario==='correlation-missing')return good.replace('mapping-observed','renderer-webarea-correlation-unproved').replace('1 10 20 600','0 10 20 600');
    return good;
  }};
  const facts=await observe({session,native,held,deadline:100,owner:()=>scenario!=='owner-lost',now:()=>clock});
  assert.equal(facts.inputAuthorized,false);assert(!JSON.stringify(facts).includes('PRIVATE'));
  return {facts,pointCalls,sends};
}
(async()=>{
  assert.equal((await run('good')).facts.reason,'mapping-observed');
  assert.equal((await run('correlation-missing')).facts.reason,'renderer-webarea-correlation-unproved');
  for(const [scenario,reason] of [['loader-change','cdp-identity-changed'],['child-frame','cdp-child-frame-present'],
    ['metrics-missing','css-viewport-unavailable'],['scale','css-viewport-transform-unproved'],
    ['metrics-change','css-viewport-changed'],['deadline','deadline'],['owner-lost','observation-unavailable'],
    ['native-error','observation-unavailable']]) {
    const result=await run(scenario);assert.equal(result.facts.reason,reason);
    if(!['metrics-change','native-error'].includes(scenario))assert.equal(result.pointCalls,0);
  }
  console.log('Closed point observation wire and passive CDP/native bracketing passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
