'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const source=require('./codex-spotlight.cjs');
const {createHeldMainGuard}=require('./codex-main-guard.cjs');
const {bindRecordedMain}=require('./codex-dom.cjs');
const URL_VALUE='app://-/index.html?initialRoute=%2Fspotlight';
const env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',
  NANH_DESKTOP_RENDERER_APP:'chatgpt-desktop',NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_INPUT_CHANNEL:'cdp-dom',
  NANH_CODEX_PROJECT_POLICY:'open-project',NANH_DESKTOP_QUALIFICATION_MODE:'renderer',NANH_CODEX_PROJECT_ARTIFACT_SHA256:source.ARTIFACT};
function dom() {
  const node=(attributes={},parent=null)=>{const n={attributes,parentElement:parent,children:[],isConnected:true,
    style:{opacity:'1',pointerEvents:'none',display:'block',visibility:'visible'},animations:[],
    getAttribute(k){return this.attributes[k]??null;},hasAttribute(k){return Object.hasOwn(this.attributes,k);},
    contains(e){for(;e;e=e.parentElement)if(e===this)return true;return false;},
    getBoundingClientRect:()=>({width:30,height:20}),getAnimations(){return this.animations;}};
    parent?.children.push(n);return n;};
  const root=node(),motion=node({'data-open':'false'},root),command=node({},motion);
  motion.style.opacity='0';
  const input=node({},command),list=node({},command),option=node({},list);
  input.type='text';
  const state={root,motion,command,input,list,option,foreign:[],foreignEditables:[],frames:[],ready:'complete',focus:false};
  const document={hasFocus:()=>state.focus,visibilityState:'visible',get readyState(){return state.ready;},
    querySelectorAll(selector){
      switch(selector) {
        case '.command-menu-dialog.global-command-menu-dialog':return state.root?[state.root]:[];
        case '[cmdk-root][data-command-menu-variant="standalone"]':return state.command?[state.command]:[];
        case 'input[cmdk-input][role="combobox"]':return state.input?[state.input]:[];
        case '[cmdk-list][role="listbox"]':return state.list?[state.list]:[];
        case 'textarea,[contenteditable="true"],input:not([type="radio"]):not([type="checkbox"]):not([type="hidden"])':return [state.input,...state.foreignEditables].filter(Boolean);
        case 'input,textarea,[contenteditable="true"],button,a[href],select,[tabindex],[role="button"],[role="option"],[role="combobox"],[role="listbox"]':return [state.command,state.input,state.list,state.option,...state.foreign,...state.foreignEditables].filter(Boolean);
        case 'iframe,frame,object,embed,textarea,[contenteditable="true"],[role="dialog"],[role="alertdialog"],[aria-modal="true"],[role="menu"]':return [...state.frames,...state.foreignEditables];
        case 'fieldset > legend':case 'input[type="radio"][name="conversational-onboarding-inline-role"]':
        case '[role="dialog"],[role="alertdialog"]':case 'textarea[data-avatar-overlay-composition-autofocus]':return [];
        default:throw Error('unexpected selector');
      }
    }};
  return {state,document,node,context:{document,getComputedStyle:e=>e.style}};
}
class Handle {
  constructor(value){this.value=value;this.disposed=false;}
  async evaluate(fn){assert.equal(this.disposed,false);return fn(this.value);}
  async dispose(){this.disposed=true;}
}
function fixture(options={}) {
  let clock=0,reads=0,ownerReads=0,mainFocus=true,mainLoader='ML',handles=[],heldDocument=dom();
  const main={url:()=> 'app://-/index.html',evaluate:async()=>mainFocus,
    evaluateHandle:async()=>new Handle({status:'absent'})};
  const auxiliary={url:()=>options.url??URL_VALUE,evaluateHandle:async(fn,held)=>{
    reads++;options.beforeRead?.({reads,dom:heldDocument,setDocument:value=>{heldDocument=value;},setFocus:value=>{mainFocus=value;},
      setMainLoader:value=>{mainLoader=value;}});
    const value=vm.runInNewContext(`(${fn.toString()})(held)`,{...heldDocument.context,held:held?.value??null});
    const handle=new Handle(value);handles.push(handle);return handle;
  }};
  const identities=p=>({page:p,url:p.url(),target:p===main?'M':'A',frame:p===main?'MF':'AF',loader:p===main?mainLoader:'AL',frameUrl:p.url(),fragment:'',
    scope:{mainScope:true,focused:mainFocus,visibleDocument:true,counts:{}}});
  const browser={contexts:()=>[{pages:()=>options.extra?[main,auxiliary,{}]:[main,auxiliary]}]};
  const owner=()=>{ownerReads++;return !options.ownerLoss||ownerReads<options.ownerLoss;};
  const identity=async(p,deadline,include,diagnostic,lease)=>{
    const value=identities(p);if(lease)value.scope=await lease.read(p,value);return value;
  };
  const guard=createHeldMainGuard(identities(main),browser,owner,1000,raw=>source.route(raw)?'spotlight':'unknown',identity,
    async ms=>{clock+=ms;},false,true,false,{sameCorrelationIdentity:(a,b)=>a.page===b.page&&a.loader===b.loader&&a.url===b.url,
      settleFolderAuxiliary:async()=>false,now:()=>clock,requireVisibleDocument:true,spotlightPolicy:options.policy!==false});
  return {guard,main,auxiliary,browser,identities,owner,readCount:()=>reads,handles:()=>handles,
    dom:()=>heldDocument,setClock:x=>{clock=x;},clock:()=>clock};
}
(async()=>{
  assert(source.policy('win32',env));
  for(const platform of ['linux','darwin'])assert(!source.policy(platform,env));
  for(const key of Object.keys(env))assert(!source.policy('win32',{...env,[key]:'other'}),key);
  for(const value of ['app://-/index.html?initialRoute=/spotlight','app://-/index.html?initialRoute=/spotlight#x',URL_VALUE+'&x=1',
    'app://-/other.html?initialRoute=%2Fspotlight','app://foreign/index.html?initialRoute=%2Fspotlight',
    'app://-/index.html?initialRoute=%2Fspotlight&initialRoute=%2Fspotlight'])assert(!source.route(value));
  let f=fixture();assert(await f.guard());assert.equal(f.readCount(),2);
  assert.deepEqual(f.guard.spotlightAuxiliary(),{sourceVersion:source.VERSION,rendererSha256:source.RENDERER,
    status:'verified',failure:null,stableSamples:2,inputCount:1,listCount:1,editableCount:1});
  assert(await f.guard());assert.equal(f.readCount(),3,'later proof uses the retained source tuple');
  const folderTrust=await require('./codex-folder-trust.cjs').run(f.main,f.guard,Date.now()+1000,{workspace:'synthetic',verify:async()=>true});
  assert.equal(folderTrust.status,'absent');assert.equal(folderTrust.spotlightAuxiliary.status,'verified');
  const inventory=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
  const begin=inventory.indexOf('facts.codexSession='),end=inventory.indexOf('if(!bindingVerified||!codingComposerReady)',begin);
  const facts={};vm.runInNewContext(inventory.slice(begin,end),{facts,directCDP:true,bindingVerified:true,codingComposerReady:true,browser:f.browser,mainGuard:f.guard});
  const fixturePath=process.argv[2];if(fixturePath)fs.writeFileSync(fixturePath,JSON.stringify({folderTrust,codexSession:facts.codexSession})+'\n',{mode:0o600});
  await f.guard.close();assert(f.handles().every(h=>h.disposed));
  const cases=[
    ['source-open',d=>{d.state.motion.attributes['data-open']='true';}],
    ['source-open',d=>{d.state.ready='loading';d.state.motion.attributes['data-open']='true';}],
    ['source-transition',d=>{d.state.motion.style.opacity='0.1';}],
    ['source-transition',d=>{d.state.ready='loading';d.state.motion.style.opacity='0.1';}],
    ['source-transition',d=>{d.state.motion.animations=[{playState:'running'}];}],
    ['source-transition',d=>{d.state.motion.style.pointerEvents='auto';}],
    ['foreign-controls',d=>{d.state.input.style.pointerEvents='auto';}],
    ['auxiliary-focus',d=>{d.state.focus=true;}],
    ['foreign-controls',d=>{d.state.foreign=[d.node()];}],
    ['foreign-controls',d=>{d.state.frames=[d.node()];}],
    ['initial-scope',d=>{d.state.root.children=[];}],
  ];
  for(const [reason,change] of cases) {
    f=fixture({beforeRead:({dom})=>change(dom)});assert.equal(await f.guard(),false,reason);
    assert.equal(f.guard.spotlightAuxiliary().failure,reason);assert.equal(f.readCount(),1);
    // A rejected lease cannot reopen after its source returns to a safe shape.
    assert.equal(await f.guard(),false);assert.equal(f.readCount(),1);await f.guard.close();
  }
  for(const change of ['document','root','input','main-focus','owner','deadline']) {
    f=fixture({beforeRead:({reads,dom:d,setDocument,setFocus})=>{
      if(reads!==2)return;
      if(change==='document')setDocument(dom());
      if(change==='root'){const next=dom();d.state.root=next.state.root;d.state.motion=next.state.motion;d.state.command=next.state.command;d.state.input=next.state.input;d.state.list=next.state.list;d.state.option=next.state.option;}
      if(change==='input')d.state.input=d.node({},d.state.command);
      if(change==='main-focus')setFocus(false);
      if(change==='deadline')f.setClock(1000);
    },ownerLoss:change==='owner'?4:undefined});
    assert.equal(await f.guard(),false,change);
    assert.equal(await f.guard(),false);await f.guard.close();
  }
  f=fixture({policy:false});assert.equal(await f.guard(),false);assert.equal(f.readCount(),0);
  f=fixture({extra:true});assert.equal(await f.guard(),false);assert.equal(f.readCount(),0);
  f=fixture({beforeRead:({reads,dom:d})=>{if(reads===1){d.state.ready='loading';}else d.state.ready='complete';}});
  assert(await f.guard());assert.equal(f.readCount(),3);await f.guard.close();
  f=fixture({beforeRead:({dom:d})=>{d.state.ready='loading';}});f.guard.sealInitialActions();
  assert.equal(await f.guard(),false);assert.equal(f.readCount(),1);await f.guard.close();
  for(const ready of ['loading','complete'])for(const foreign of ['textarea','frame']) {
    f=fixture();const d=f.dom(),original={...d.state};
    Object.assign(d.state,{root:null,motion:null,command:null,input:null,list:null,option:null,ready});
    if(foreign==='textarea')d.state.foreignEditables=[d.node()];else d.state.frames=[d.node()];
    assert.equal(await f.guard(),false,`${ready} missing root with ${foreign}`);
    assert.equal(f.guard.spotlightAuxiliary().failure,'foreign-controls');assert.equal(f.readCount(),1);
    Object.assign(d.state,original);
    assert.equal(await f.guard(),false,'later clean root cannot erase observed foreign content');
    assert.equal(f.readCount(),1);await f.guard.close();
  }
  {
    let original;
    f=fixture({beforeRead:({reads,dom:d})=>{
      if(reads===1){original={...d.state};Object.assign(d.state,{root:null,motion:null,command:null,input:null,list:null,option:null,ready:'loading'});}
      if(reads===2)Object.assign(d.state,original);
    }});
    assert(await f.guard(),'control-free initial document may hydrate under the original deadline');
    assert.equal(f.readCount(),3);await f.guard.close();
  }
  // Later controllers first match the private original document identity and
  // then retain one independent closed tuple across every functional proof.
  f=fixture();const lease=source.create({deadline:Date.now()+2000});
  const binding={main:f.identities(f.main),auxiliary:f.identities(f.auxiliary)};
  assert.equal(await bindRecordedMain(f.browser,binding,f.owner,Date.now()+2000,async p=>f.identities(p),true,lease),f.main);
  assert.equal(f.readCount(),2);assert.equal(lease.receipt().editableCount,1);
  f.dom().state.input.style.pointerEvents='auto';
  assert.equal(await bindRecordedMain(f.browser,binding,f.owner,Date.now()+2000,async p=>f.identities(p),true,lease).catch(()=>null),null);
  assert(lease.failed());await lease.close();assert(f.handles().every(h=>h.disposed));
  // The same Page may reload while its auxiliary source read is pending. Main
  // focus, native ownership and page membership do not preserve that document.
  f=fixture({beforeRead:({reads,setMainLoader})=>{if(reads===1)setMainLoader('RELOADED');}});
  {
    const loan=source.create({deadline:Date.now()+2000});
    const original={main:f.identities(f.main),auxiliary:f.identities(f.auxiliary)};
    const result=await bindRecordedMain(f.browser,original,f.owner,Date.now()+2000,async p=>f.identities(p),true,loan);
    assert.equal(result,null,'main document reload during auxiliary read must refuse binding before input');
    assert.equal(loan.receipt().failure,'identity');assert.equal(f.readCount(),1);
    await loan.close();assert(f.handles().every(h=>h.disposed));
  }
  for(const change of ['identity','url','main-focus','owner','root','document']) {
    f=fixture();const loan=source.create({deadline:Date.now()+2000});
    const original={main:f.identities(f.main),auxiliary:f.identities(f.auxiliary)};
    const identify=async p=>({...f.identities(p),...(change==='identity'&&p===f.auxiliary?{loader:'changed'}:{}),
      ...(change==='url'&&p===f.auxiliary?{url:'app://-/index.html?initialRoute=/spotlight'}:{})});
    if(['root','document'].includes(change)) {
      assert.equal(await bindRecordedMain(f.browser,original,f.owner,Date.now()+2000,async p=>f.identities(p),true,loan),f.main);
      if(change==='root'){const fresh=dom();Object.assign(f.dom().state,fresh.state);}
      else {const prior=f.auxiliary.evaluateHandle;f.auxiliary.evaluateHandle=async(fn,held)=>{
        const fresh=dom();return new Handle(vm.runInNewContext(`(${fn.toString()})(held)`,{...fresh.context,held:held?.value}));};}
    }
    if(change==='main-focus')f.main.evaluate=async()=>false;
    const result=await bindRecordedMain(f.browser,original,change==='owner'?()=>false:f.owner,Date.now()+2000,identify,true,loan).catch(()=>null);
    assert.equal(result,null,change);assert(loan.failed(),change);await loan.close();
  }
  for(const loss of ['focus','visibility']) {
    f=fixture();let focusReads=0;
    const loan=source.create({deadline:Date.now()+2000});
    const original={main:f.identities(f.main),auxiliary:f.identities(f.auxiliary)};
    f.main.evaluate=async(fn,arg)=>{
      focusReads++;
      return vm.runInNewContext(`(${fn.toString()})(arg)`,{arg,document:{
        visibilityState:focusReads===5&&loss==='visibility'?'hidden':'visible',
        hasFocus:()=>!(focusReads===5&&loss==='focus')}});
    };
    assert.equal(await bindRecordedMain(f.browser,original,f.owner,Date.now()+2000,async p=>f.identities(p),true,loan),null,loss);
    assert.equal(focusReads,5);assert.equal(f.readCount(),2);
    assert.equal(loan.receipt().failure,'main-focus');
    // Focus/visibility is now restored; the capability must remain rejected.
    assert.equal(await bindRecordedMain(f.browser,original,f.owner,Date.now()+2000,async p=>f.identities(p),true,loan),null);
    assert.equal(focusReads,5);assert.equal(f.readCount(),2);await loan.close();
  }
  // A handle resolving after the original deadline is disposed and cannot
  // reopen a failed capability or become an unbounded controller resource.
  {
    const d=dom(),late=new Handle(vm.runInNewContext(`(${source.sample.toString()})(null)`,d.context));
    const loan=source.create({deadline:Date.now()+15});
    const page={evaluateHandle:()=>new Promise(resolve=>setTimeout(()=>resolve(late),40))};
    await assert.rejects(loan.read(page,{url:URL_VALUE,frameUrl:URL_VALUE,fragment:'',target:'A',frame:'F',loader:'L'}));
    assert.equal(loan.receipt().failure,'deadline');await new Promise(resolve=>setTimeout(resolve,50));
    assert(late.disposed);await loan.close();
  }
  console.log('PASS: frozen closed Spotlight policy, atomic truthful counts, retained DOM/identity, refusal latches and no auxiliary input');
})().catch(error=>{console.error(error);process.exitCode=1;});
