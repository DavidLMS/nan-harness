const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const parseNative=require('./codex-point-observation.cjs').parseNative;
const moduleFixture={exports:{}};vm.runInNewContext(fs.readFileSync(require.resolve('./codex-owned-move.cjs'),'utf8'),{module:moduleFixture,exports:moduleFixture.exports,process:{env:{}},JSON,Number,Date,Promise,setTimeout,clearTimeout,require:key=>key==='./codex-point-observation.cjs'?{parseNative}:key==='./codex-onboarding.cjs'?{scopeFingerprint:'div.frozen'}:null});
const policy={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'macOS',NANH_CODEX_OWNED_MOVE:'source-point',NANH_CODEX_PROJECT_POLICY:'open-project',NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_PROJECT_ARTIFACT_SHA256:'f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7'};
async function fixture(scenario){let moves=0,released=0,samples=0,points=0,owner=true;
 const held={target:'T',url:'app://source',frameUrl:'app://source',frame:'F',loader:'L',fragment:''};
 const metrics={cssLayoutViewport:{clientWidth:600,clientHeight:400,pageX:0,pageY:0},cssVisualViewport:{clientWidth:600,clientHeight:400,pageX:0,pageY:0,offsetX:0,offsetY:0,scale:1}};
 const session={async send(method){if(method==='Target.getTargetInfo')return {targetInfo:{targetId:'T',url:'app://source'}};
  if(method==='Page.getFrameTree')return {frameTree:{frame:{id:'F',loaderId:scenario==='loader-change'?'FOREIGN':'L',url:'app://source'}}};
  if(method==='Page.getLayoutMetrics')return metrics;
  if(method==='Runtime.evaluate')return {result:scenario==='source-missing'?{subtype:'null'}:{objectId:'SOURCE'}};
  if(method==='Runtime.callFunctionOn'){samples++;if(scenario==='source-replaced'&&samples===2)return {result:{value:null}};
   if(scenario==='owner-lost'&&samples===2)owner=false;
   return {result:{value:{rect:[100,100,100,40],css:[600,400,scenario==='post-dom-drift'&&moves?155:150,120]}}};}
  if(method==='Runtime.releaseObjectGroup'){released++;return {}};throw Error(method);
 }};
 const native={pointObserve(){points++;return points===1?'point-observation point-occluded 1 1 1 1 1 0 0 1 1 10 20 600 400\n':'point-observation mapping-observed 1 1 1 1 1 1 1 1 1 30 40 600 400\n';},moveOwned(css,url,kind){moves++;assert.equal(kind,'onboarding-engineering');assert.equal(css[2],150);if(scenario==='uncertain')throw Error('private');return {reason:scenario==='full'?'full-workarea-bounds-blocker':'moved-point-observed',inputAuthorized:false}}};
 const result=await moduleFixture.exports.run({session,native,held,deadline:1000,now:()=>0,owner:()=>owner,policy});assert.equal(result.inputAuthorized,false);assert(!JSON.stringify(result).includes('app://'));return {result,moves,released};}
(async()=>{const r=await fixture('success');assert.equal(r.result.reason,'moved-source-point-observed');assert.equal(r.moves,1);assert.equal(r.released,1);
 for(const [scenario,reason,count] of [['loader-change','renderer-changed',0],['source-missing','source-point-unavailable',0],['source-replaced','source-point-unavailable',0],['owner-lost','deadline-or-owner',0],['post-dom-drift','renderer-changed',1],['uncertain','move-unavailable-or-uncertain',1],['full','native-move-rejected',1]]){const r=await fixture(scenario);assert.equal(r.result.reason,reason);assert.equal(r.moves,count)}
 console.log('8 retained source-point, loader/owner/drift, uncertain and post-mapping fixtures passed');})();
