const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {directCDPPolicy,bindRecordedMain}=require('./codex-dom.cjs');
const {createHeldMainGuard}=require('./codex-main-guard.cjs');
const env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'macOS',NANH_DESKTOP_RENDERER_APP:'chatgpt-desktop',NANH_CODEX_INPUT_CHANNEL:'cdp-dom'};
assert.equal(directCDPPolicy('darwin',env),true);
for(const key of Object.keys(env))assert.equal(directCDPPolicy('darwin',{...env,[key]:'wrong'}),false,key);
const source=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
const policyStart=source.indexOf('const directCDP=trial'),policyEnd=source.indexOf(';',policyStart);
for(const [platform,request,accepted] of [
 ['win32',{},false],['win32',{codexProfileIsolation:'prepared-windows'},true],
 ['win32',{codexProfileIsolation:'unknown'},false],['win32',{codexProfileLoan:{}},false],
 ['linux',{},false],['linux',{codexProfileIsolation:'prepared-windows'},false],
 ['linux',{codexProfileLoan:{}},true],['darwin',{codexProfileLoan:{}},true],['darwin',{},false]]) {
 const result=vm.runInNewContext(source.slice(policyStart,policyEnd)+';directCDP',
  {trial:true,request,process:{platform},require:()=>({directCDPPolicy:()=>true})});
 assert.equal(result,accepted,platform+JSON.stringify(request));
}
const start=source.indexOf('function mainConfirmationFacts()'),end=source.indexOf('async function observeMainAux(',start);
const bind=vm.runInNewContext(source.slice(start,end)+';bindCorrelationMain',{Date,require,
 heldMainGuard:(held,browser,owner,deadline,route,identity,pause,scope,appearance,focus,visible)=>
 createHeldMainGuard(held,browser,owner,deadline,route,identity,pause,scope,appearance,focus,
 {sameCorrelationIdentity:(a,b)=>a.key===b.key,now:Date.now,requireVisibleDocument:visible}),
 sameCorrelationIdentity:(a,b)=>a.key===b.key});
(async()=>{
 for(const scenario of ['good','hidden','replacement','owner','foreign','reload']) {
  const main={url:()=> 'app://-/index.html'},held={page:main,key:'same'};
  const browser={contexts:()=>[{pages:()=>scenario==='foreign'?[main,{}]:[main]}]};
  const identity=async()=>({key:['replacement','reload'].includes(scenario)?'other':'same',scope:{mainScope:true,focused:false,visibleDocument:scenario!=='hidden'}});
  const owner=()=>scenario!=='owner';
  const diagnostic={};
  assert.equal(!!await bind(held,browser,owner,Date.now()+1000,()=>null,identity,async()=>{},diagnostic,null,false),!['hidden','replacement','reload','owner','foreign'].includes(scenario),scenario);
  assert.equal(diagnostic.inputChannel,'cdp-dom');
  assert.equal(await bind(held,browser,owner,Date.now()+1000,()=>null,identity,async()=>{},null,null,true),null,'legacy focus '+scenario);
 }
 for(const scenario of ['inert','controls','focused','route','owner']) {
  const main={},aux={},held={page:main,key:'main'},browser={contexts:()=>[{pages:()=>[main,aux]}]};
  const empty={roleLegend:0,roleRadios:0,engineering:0,dialog:0,quickChatComposer:0,editable:0};
  aux.url=()=>scenario==='route'?'app://-/foreign':'app://-/avatar';
  const identity=async page=>({key:page===main?'main':'aux',scope:{mainScope:page===main,visibleDocument:true,
   focused:page===aux&&scenario==='focused',counts:{...empty,editable:page===aux&&scenario==='controls'?1:0}}});
  const guard=createHeldMainGuard(held,browser,()=>scenario!=='owner',Date.now()+1000,
   url=>url==='app://-/avatar'?'avatarOverlay':'unknown',identity,async()=>{},true,false,false,
   {sameCorrelationIdentity:(a,b)=>a.key===b.key,now:Date.now,requireVisibleDocument:true});
  assert.equal(await guard(),scenario==='inert',scenario);
 }
 {
  const main={},aux={url:()=> 'app://-/avatar'},held={page:main,key:'main'};
  let pages=[main],reads=0;
  const browser={contexts:()=>[{pages:()=>pages}]},owner=()=>true;
  const identity=async page=>{reads++;if(reads===1)pages=[main,aux];return {key:page===main?'main':'aux',scope:{
   mainScope:page===main,visibleDocument:true,focused:false,
   counts:{roleLegend:0,roleRadios:0,engineering:0,dialog:0,quickChatComposer:0,editable:0}}};};
  assert(await bind(held,browser,owner,Date.now()+1000,()=> 'avatarOverlay',identity,async()=>{},null,null,false));
  const existing=createHeldMainGuard(held,browser,owner,Date.now()+1000,()=> 'avatarOverlay',identity,async()=>{},false,false,false,
   {sameCorrelationIdentity:(a,b)=>a.key===b.key,now:Date.now,requireVisibleDocument:true});
  assert(await existing());assert(existing.binding().auxiliary);
  assert(await bind(held,browser,owner,Date.now()+1000,()=> 'avatarOverlay',identity,async()=>{},null,existing,false));
  assert(existing.binding().auxiliary);
 }
 const frame={url:'app://-/index.html',target:'main',frame:'frame',loader:'loader',frameUrl:'app://-/index.html',fragment:''};
 let visible=true;
 const main={evaluate:async(fn,arg)=>vm.runInNewContext(`(${fn})(${arg})`,{document:{visibilityState:visible?'visible':'hidden',hasFocus:()=>false}})};
 const browser={contexts:()=>[{pages:()=>[main]}]},binding={main:frame,auxiliary:null};
 assert.equal(await bindRecordedMain(browser,binding,()=>true,Date.now()+1000,async()=>frame,true),main);
 assert.equal(await bindRecordedMain(browser,binding,()=>true,Date.now()+1000,async()=>frame,false),null);
 visible=false;assert.equal(await bindRecordedMain(browser,binding,()=>true,Date.now()+1000,async()=>frame,true),null);
 assert.equal(await bindRecordedMain(browser,binding,()=>true,Date.now()+1000,async()=>({...frame,loader:'new'}),true),null);
 console.log('PASS: explicit hosted policy, visible unfocused exact document, original owner/loader/page set, hidden denial and legacy focus unchanged');
})().catch(e=>{console.error(e);process.exitCode=1});
