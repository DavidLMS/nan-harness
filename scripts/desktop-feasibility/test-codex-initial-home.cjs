'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const home=require('./codex-initial-home.cjs');
const {codingScope}=require('./codex-onboarding.cjs');
const {createHeldMainGuard}=require('./codex-main-guard.cjs');
const env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Linux',
  NANH_DESKTOP_RENDERER_APP:'chatgpt-desktop',NANH_CODEX_INPUT_CHANNEL:'cdp-dom',
  NANH_CODEX_PUBLIC_ONBOARDING:'engineering',
  NANH_CODEX_PROJECT_ARTIFACT_SHA256:'ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c'};
const loan={schemaVersion:1,platform:'linux'};
assert.equal(home.policy('linux',env,loan),true);
for(const key of Object.keys(env))assert.equal(home.policy('linux',{...env,[key]:'wrong'},loan),false,key);
for(const platform of ['darwin','win32'])assert.equal(home.policy(platform,env,loan),false);
for(const value of [null,{}, {...loan,platform:'macos'},{...loan,schemaVersion:2}])
  assert.equal(home.policy('linux',env,value),false);
function fixture(deadline=1000) {
  let clock=0,owned=true,profileOwned=true,reads=0,seals=0,disposed=0,afterPause=()=>{},onRead=()=>{};
  let doc={visibilityState:'visible',hasFocus:()=>true},nodes=[];
  const node=(tag,attrs={},classes=[])=>({tagName:tag,attrs,isConnected:true,ownerDocument:doc,
    style:{display:'block',visibility:'visible'},children:[],classList:{contains:key=>classes.includes(key)},
    getAttribute(key){return this.attrs[key]??null},hasAttribute(key){return Object.hasOwn(this.attrs,key)},
    contains(other){return this===other||this.children.some(child=>child.contains(other))},
    closest(selector){return selector==='[inert]'&&this.inert?this:null},
    getBoundingClientRect:()=>({width:100,height:40})});
  let root=node('DIV',{'data-codex-composer-root':'','data-composer-placement':'home'});
  let editor=node('DIV',{contenteditable:'true'},['ProseMirror']);root.children=[editor];nodes=[root,editor];
  const context=vm.createContext({document:doc,getComputedStyle:e=>e.style});
  const refresh=()=>{context.document=doc;doc.querySelectorAll=()=>nodes;};refresh();
  const browserCall=(fn,arg)=>{context.argument=arg;
    return vm.runInContext('('+fn.toString()+')(argument)',context);};
  const page={url:()=> 'app://-/index.html',
    evaluate:async(fn,arg)=>fn===codingScope?false:browserCall(fn,arg),
    evaluateHandle:async(fn,arg)=>{const value=browserCall(fn,arg);return {
      evaluate:async callback=>browserCall(callback,value),dispose:async()=>{disposed++;}};}};
  let pages=[page];const browser={contexts:()=>[{pages:()=>pages}]};
  const original={page,url:page.url(),target:'PRIVATE-target',frame:'PRIVATE-frame',loader:'PRIVATE-loader',
    frameUrl:page.url(),fragment:''};
  let current={...original};
  const scope=()=>({mainScope:false,focused:doc.hasFocus(),visibleDocument:doc.visibilityState==='visible',
    counts:{roleLegend:0,roleRadios:0,engineering:0,dialog:0,quickChatComposer:0,editable:1}});
  const identity=async()=>{onRead(++reads);return {...current,scope:scope()};};
  const same=(a,b)=>['page','url','target','frame','loader','frameUrl','fragment'].every(k=>a[k]===b[k]);
  const owner=()=>owned&&profileOwned;
  const pause=async ms=>{clock+=ms;afterPause();};
  const guard=createHeldMainGuard(original,browser,owner,deadline,()=> 'unknown',identity,pause,false,false,false,
    {sameCorrelationIdentity:same,settleFolderAuxiliary:async()=>false,now:()=>clock,requireVisibleDocument:true});
  const seal=guard.sealInitialActions;guard.sealInitialActions=()=>{seals++;seal();};
  const admission=home.create({held:original,guard,owner,profile:{verify:()=>profileOwned},identity,same,
    deadline,now:()=>clock,pause});
  return {admission,guard,page,root,editor,node,scope,identity,original,same,browser,
    get nodes(){return nodes},set nodes(value){nodes=value;refresh()},
    current,doc,loseOwner:()=>{owned=false},loseProfile:()=>{profileOwned=false},
    expire:()=>{clock=deadline},onPause:fn=>{afterPause=fn},onRead:fn=>{onRead=fn},
    setPages:value=>{pages=value},
    replaceDocument:()=>{doc={...doc};for(const e of nodes)e.ownerDocument=doc;refresh();},
    seals:()=>seals,disposed:()=>disposed,reads:()=>reads,now:()=>clock,pause};
}
(async()=>{
  const accepted=fixture(),bound=await accepted.admission.tryBind();
  assert.equal(bound.status,'confirmed');assert.equal(accepted.seals(),1);
  assert.equal(bound.binding.guard,accepted.guard);assert.equal(await bound.binding.verify(),true);
  assert.equal(Object.keys(bound.binding.proof).length,11);
  assert.equal(bound.binding.proof.stableSamples,2);
  assert.ok(!JSON.stringify(bound.binding.proof).includes('PRIVATE'));
  const receipt=home.onboardingReceipt({status:'absent',clickAttempted:false,clickCompleted:false});
  assert.equal(receipt.stage,'fresh-linux-home');assert.equal(receipt.roleScopeAbsent,true);
  assert.equal(receipt.codingComposerReady,true);
  for(const key of ['roleClickAttempted','roleClickCompleted','engineeringChecked','continueClickAttempted',
    'continueClickCompleted','taskClickAttempted','taskClickCompleted','taskScopeProved'])assert.equal(receipt[key],false);
  await accepted.admission.close();assert.equal(accepted.disposed(),1);
  for(const [name,alter] of [
    ['duplicate root',f=>{f.nodes=[...f.nodes,f.node('DIV',{'data-codex-composer-root':'','data-composer-placement':'home'})]}],
    ['duplicate editor',f=>{f.nodes=[...f.nodes,f.node('DIV',{contenteditable:'true'},['ProseMirror'])]}],
    ['role ambiguity',f=>{f.nodes=[...f.nodes,f.node('INPUT',{type:'radio',name:'conversational-onboarding-inline-role'})]}],
    ['dialog',f=>{f.nodes=[...f.nodes,f.node('DIV',{role:'dialog'})]}],
    ['menu',f=>{f.nodes=[...f.nodes,f.node('DIV',{role:'menu'})]}],
    ['conversation',f=>{f.nodes=[...f.nodes,f.node('DIV',{'data-thread-find-target':'conversation'})]}],
    ['foreign frame',f=>{f.nodes=[...f.nodes,f.node('IFRAME')]}],
    ['foreign auxiliary',f=>f.setPages([f.page,{url:()=> 'app://-/foreign'}])],
    ['quick chat',f=>{f.nodes=[...f.nodes,f.node('TEXTAREA',{'data-avatar-overlay-composition-autofocus':''})]}],
    ['generic editor',f=>{f.editor.classList={contains:()=>false}}],
    ['disabled',f=>{f.editor.disabled=true}],['readonly',f=>{f.editor.readOnly=true}],
    ['inert',f=>{f.editor.inert=true}],['hidden',f=>{f.doc.visibilityState='hidden'}],
    ['unfocused',f=>{f.doc.hasFocus=()=>false}],['detached',f=>{f.editor.isConnected=false}],
    ['owner',f=>f.loseOwner()],['profile',f=>f.loseProfile()],['cutoff',f=>f.expire()],
    ['thread source',f=>{const evaluate=f.page.evaluate;f.page.evaluate=(fn,arg)=>fn===codingScope?true:evaluate(fn,arg)}],
    ['overflow',f=>{f.nodes=Array(4097).fill(f.root)}],
    ...['target','frame','loader','frameUrl','url','fragment'].map(key=>[key,f=>{f.current[key]='PRIVATE-changed'}]),
  ]) {
    const f=fixture();alter(f);assert.equal((await f.admission.tryBind()).status,'rejected',name);
    assert.equal(f.seals(),0,name);await f.admission.close();
  }
  for(const [name,alter] of [
    ['document',f=>f.replaceDocument()],['root',f=>{const r=f.node('DIV',f.root.attrs);r.children=[f.editor];f.nodes=[r,f.editor]}],
    ['editor',f=>{const e=f.node('DIV',{contenteditable:'true'},['ProseMirror']);f.root.children=[e];f.nodes=[f.root,e]}],
    ['owner',f=>f.loseOwner()],['profile',f=>f.loseProfile()],['focus',f=>{f.doc.hasFocus=()=>false}],
    ['cutoff',f=>f.expire()],['loader',f=>{f.current.loader='PRIVATE-new-loader'}],
    ['foreign auxiliary',f=>f.setPages([f.page,{url:()=> 'app://-/foreign'}])],
  ]) {
    const f=fixture();f.onPause(()=>alter(f));assert.equal((await f.admission.tryBind()).status,'rejected',name);
    assert.equal((await f.admission.tryBind()).status,'rejected','no rebind '+name);await f.admission.close();
  }
  for(const read of [1,2,4,7,10]) {
    const f=fixture();f.onRead(n=>{if(n===read)f.loseProfile()});
    assert.equal((await f.admission.tryBind()).status,'rejected','profile lost at '+read);await f.admission.close();
  }
  {
    const f=fixture();f.nodes=[];assert.equal((await f.admission.tryBind()).status,'absent');
    f.nodes=[f.editor];assert.equal((await f.admission.tryBind()).status,'absent','generic body is not home');
    assert.equal(f.seals(),0);
    f.nodes=[f.root,f.editor];assert.equal((await f.admission.tryBind()).status,'confirmed');
    await f.admission.close();
  }
  {
    const f=fixture(),result=await f.admission.tryBind();assert.equal(result.status,'confirmed');
    f.editor.isConnected=false;assert.equal(await result.binding.verify(),false);
    f.editor.isConnected=true;assert.equal(await result.binding.verify(),false,'lost tuple never rebinds');
    assert.equal((await f.admission.tryBind()).status,'rejected');await f.admission.close();
  }
  {
    const f=fixture(5);let release,disposed=false,finish;
    const cleanup=new Promise(resolve=>{finish=resolve});
    f.page.evaluateHandle=()=>new Promise(resolve=>{release=resolve});
    assert.equal((await f.admission.tryBind()).status,'rejected','query bounded by original cutoff');
    release({dispose:async()=>{disposed=true;finish()}});await cleanup;
    assert.equal(disposed,true,'late passive handle is released');assert.equal(f.seals(),0);
  }
  const source=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
  const policyStart=source.indexOf("const initialHomeSource=require('./codex-initial-home.cjs');");
  const policyEnd=source.indexOf('const heldMain=trial?',policyStart);
  const policySource='let initialHome;'+source.slice(policyStart,policyEnd)+'initialHome;';
  const inputs={directCDP:true,profileAuthority:{verify:()=>true},trustGuard:{},folderTrust:{status:'completed'},
    process:{platform:'linux',env},request:{codexProfileLoan:loan},initialMain:{},onboardingOwnerGuard:()=>true,
    correlationIdentity:()=>{},sameCorrelationIdentity:()=>true,correlationDeadline:1000,require};
  assert.ok(vm.runInNewContext(policySource,inputs));
  for(const changed of [
    {directCDP:false},{profileAuthority:null},{trustGuard:null},{folderTrust:{status:'blocked'}},
    {process:{platform:'win32',env}},{process:{platform:'darwin',env}},
    {process:{platform:'linux',env:{...env,NANH_CODEX_PROJECT_ARTIFACT_SHA256:'other'}}},
    {request:{codexProfileLoan:{schemaVersion:1,platform:'macos'}}},
  ])assert.equal(vm.runInNewContext(policySource,{...inputs,...changed}),undefined);
  const begin=source.indexOf('async function bindCorrelationMain('),end=source.indexOf('async function observeMainAux(',begin);
  for(const throws of [false,true]) {
    const f=fixture(),diagnostic={};
    const bind=vm.runInNewContext(source.slice(begin,end)+';bindCorrelationMain',{
      require,Date:{now:f.now},sameCorrelationIdentity:f.same,heldMainGuard:()=>{throw Error('must retain original guard')}});
    const result=await bind(f.original,f.browser,()=>true,1000,()=> 'unknown',f.identity,f.pause,
      diagnostic,f.guard,false,()=>{if(throws)throw Error('PRIVATE-observer');},null,f.admission);
    assert.equal(result.initialHomeBinding.guard,f.guard);assert.equal(f.seals(),1);
    assert.equal(diagnostic.sourceKind,'fresh-linux-home');assert.equal(diagnostic.mainScopeUnique,false);
    assert.equal(diagnostic.documentFocused,true);assert.equal(diagnostic.status,'confirmed');
    assert.deepEqual({...diagnostic.sourceScopes},{coding:false,home:true});
    assert.equal(diagnostic.freshHomeProof,result.initialHomeBinding.proof);
    assert.ok(!JSON.stringify(diagnostic).includes('PRIVATE'));
    if(!throws&&process.argv[2]) {
      const record={schemaVersion:1,mechanism:'renderer-inventory',diagnosticsOnly:true,
        app:'chatgpt-desktop',endpointOwned:true,launcherOwned:true,attached:true,
        pageCount:1,textareaCount:0,editableCount:1,sendCount:0,retryCount:0,newThreadCount:0,
        loginCount:0,dialogCount:0,errorCategory:null,observerStage:'complete',
        initialMainConfirmation:diagnostic,publicOnboarding:receipt,
        codexSession:{inputChannel:'cdp-dom',bindingVerified:true,codingComposerReady:true,
          auxiliaryInert:true,pageCount:1}};
      fs.writeFileSync(process.argv[2],JSON.stringify(record)+'\n',{mode:0o600});
    }
    await f.admission.close();
  }
  {
    let calls=0;const f=fixture(),diagnostic={};
    const bind=vm.runInNewContext(source.slice(begin,end)+';bindCorrelationMain',{
      require,Date:{now:f.now},sameCorrelationIdentity:f.same,heldMainGuard:()=>async()=>true});
    const roleIdentity=async()=>({...f.original,scope:{...f.scope(),mainScope:true}});
    assert.ok(await bind(f.original,f.browser,()=>true,1000,()=> 'unknown',roleIdentity,f.pause,
      diagnostic,null,true,()=>{},null,{tryBind:()=>{calls++;throw Error('unexpected home fallback')}}));
    assert.equal(calls,0);assert.equal(diagnostic.sourceKind,undefined,'role serialization stays unchanged');
    await f.admission.close();
  }
  console.log('Frozen Linux fresh-home policy, retained tuple, sealing and refusal fixtures PASS');
})().catch(error=>{console.error(error);process.exitCode=1});
