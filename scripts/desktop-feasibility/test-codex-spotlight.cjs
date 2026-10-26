'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const source=require('./codex-spotlight.cjs');
const {createHeldMainGuard}=require('./codex-main-guard.cjs');
const {bindRecordedMain}=require('./codex-dom.cjs');
const URL_VALUE='app://-/index.html?initialRoute=%2Fspotlight';
const env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',
  NANH_DESKTOP_RENDERER_APP:'chatgpt-desktop',NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_INPUT_CHANNEL:'cdp-dom',
  NANH_CODEX_PROJECT_POLICY:'open-project',NANH_DESKTOP_QUALIFICATION_MODE:'renderer',NANH_CODEX_PROJECT_ARTIFACT_SHA256:source.ARTIFACT};
function inlineStyle(raw) {
  const values=new Map(),priorities=new Map();
  for(const declaration of raw.split(';')) {
    const colon=declaration.indexOf(':');if(colon<0)continue;
    const name=declaration.slice(0,colon).trim(),text=declaration.slice(colon+1).trim();
    const priority=/!important$/.test(text)?'important':'',value=text.replace(/\s*!important$/,'');
    // Real Chromium enumerates the outline shorthand as these three longhands.
    const expanded=name==='outline'?[['outline-color','initial'],['outline-style',value],['outline-width','initial']]:
      name==='inset'?['top','right','bottom','left'].map(key=>[key,value]):[[name,value]];
    for(const [key,item] of expanded) {
      values.set(key,item);priorities.set(key,priority);
    }
  }
  return {[Symbol.iterator]:()=>values.keys(),getPropertyPriority:name=>priorities.get(name)??'',
    getPropertyValue:name=>name==='outline'?(values.get('outline-style')==='none'
      &&values.get('outline-color')==='initial'&&values.get('outline-width')==='initial'?'none':'other'):
      name==='inset'?(['top','right','bottom','left'].every(key=>values.get(key)===values.get('top'))?values.get('top')??'':'other'):values.get(name)??''};
}
function dom() {
  const node=(attributes={},parent=null)=>{const n={attributes,parentElement:parent,children:[],isConnected:true,tagName:'DIV',isContentEditable:false,
    style:{opacity:'1',pointerEvents:'none',display:'block',visibility:'visible'},animations:[],
    getAttribute(k){return this.attributes[k]??null;},hasAttribute(k){return Object.hasOwn(this.attributes,k);},
    getAttributeNames(){return Object.keys(this.attributes);},
    contains(e){for(;e;e=e.parentElement)if(e===this)return true;return false;},
    getBoundingClientRect:()=>({width:30,height:20}),getAnimations(){return this.animations;}};
    Object.defineProperty(n,'childNodes',{get:()=>n.children});
    Object.defineProperty(n,'classList',{get:()=>{const tokens=(n.attributes.class??'').split(/\s+/).filter(Boolean);return {length:tokens.length,contains:x=>tokens.includes(x)};}});
    parent?.children.push(n);return n;};
  const html=node(),body=node({tabindex:'0',style:'outline: none'},html);
  Object.assign(body,{tagName:'BODY',tabIndex:0,isContentEditable:false});
  Object.defineProperty(body,'style',{get:()=>inlineStyle(body.attributes.style??'')});
  const root=node({},body),motion=node({'data-open':'false'},root),command=node({},motion);
  motion.style.opacity='0';
  const input=node({},command),list=node({},command),option=node({},list);
  input.type='text';input.tagName='INPUT';
  const cssom=node=>{node.computed=node.style;Object.defineProperty(node,'style',{get:()=>inlineStyle(node.attributes.style??'')});};
  const makeToast=()=>{
    const portal=node({class:'codex-toast-area pointer-events-none fixed z-[60]',style:'inset: 0px;'},body);cssom(portal);
    const outer=node({class:'absolute inset-y-0 start-0 my-2 flex flex-col items-end justify-start px-2 md:pb-5',style:'inset-inline-end: 0px;'},portal);outer.tagName='SPAN';cssom(outer);
    const spacer=node({class:'h-10 shrink-0'},outer),inner=node({class:'relative w-full'},outer);spacer.tagName=inner.tagName='SPAN';
    const section=node({'aria-label':'SYNTHETIC_PRIVATE_NOTIFICATION_LABEL','tabindex':'-1','aria-live':'polite','aria-relevant':'additions text','aria-atomic':'false'},inner);
    section.tagName='SECTION';section.tabIndex=-1;return {portal,outer,spacer,inner,section};
  };
  const state={root,motion,command,input,list,option,toast:makeToast(),extraPortals:[],foreign:[],foreignEditables:[],frames:[],legends:[],ready:'complete',focus:false};
  const document={body,documentElement:html,hasFocus:()=>state.focus,visibilityState:'visible',get readyState(){return state.ready;},
    querySelectorAll(selector){
      switch(selector) {
        case '.command-menu-dialog.global-command-menu-dialog':return state.root?[state.root]:[];
        case '.codex-toast-area':return [state.toast?.portal,...state.extraPortals].filter(Boolean);
        case '[cmdk-root][data-command-menu-variant="standalone"]':return state.command?[state.command]:[];
        case 'input[cmdk-input][role="combobox"]':return state.input?[state.input]:[];
        case '[cmdk-list][role="listbox"]':return state.list?[state.list]:[];
        case 'textarea,[contenteditable="true"],input:not([type="radio"]):not([type="checkbox"]):not([type="hidden"])':return [state.input,...state.foreignEditables].filter(Boolean);
        case 'input,textarea,[contenteditable="true"],button,a[href],select,[tabindex],[role="button"],[role="option"],[role="combobox"],[role="listbox"]':return [document.body,state.command,state.input,state.list,state.option,state.toast?.section,...state.foreign,...state.foreignEditables].filter(Boolean);
        case 'iframe,frame,object,embed,textarea,[contenteditable="true"],[role="dialog"],[role="alertdialog"],[aria-modal="true"],[role="menu"]':return [...state.frames,...state.foreignEditables];
        case 'fieldset > legend':return state.legends;
        case 'input[type="radio"][name="conversational-onboarding-inline-role"]':
        case '[role="dialog"],[role="alertdialog"]':case 'textarea[data-avatar-overlay-composition-autofocus]':return [];
        default:throw Error('unexpected selector');
      }
    }};
  return {state,document,node,makeToast,context:{document,getComputedStyle:e=>e.computed??e.style}};
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
  return {guard,main,auxiliary,browser,identities,identity,owner,readCount:()=>reads,handles:()=>handles,
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
    status:'verified',failure:null,stableSamples:2,inputCount:1,listCount:1,editableCount:1,toastAnnouncerCount:1});
  assert(await f.guard());assert.equal(f.readCount(),3,'later proof uses the retained source tuple');
  const folderTrust=await require('./codex-folder-trust.cjs').run(f.main,f.guard,Date.now()+1000,{workspace:'synthetic',verify:async()=>true});
  assert.equal(folderTrust.status,'absent');assert.equal(folderTrust.spotlightAuxiliary.status,'verified');
  const inventory=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
  const policyBegin=inventory.indexOf('function onboardingBudget('),policyEnd=inventory.indexOf('// This passive receipt',policyBegin);
  const preparedPolicy=vm.runInNewContext(inventory.slice(policyBegin,policyEnd)+';spotlightPreparedWindowsPolicy',{require});
  // Exact Rust Windows request contract: native profile custody remains in
  // RendererSession.guard; unlike Unix it deliberately contains no Node loan.
  const preparedRequest={ownerPid:9,connectionPath:'C:\\synthetic\\connection-9.json',ownedWorkspace:'C:\\synthetic\\workspace',
    codexProfileIsolation:'prepared-windows',inventoryDeadlineUnixMs:1000};
  assert(preparedPolicy(preparedRequest,true,'win32',env,0));
  for(const invalid of [{...preparedRequest,codexProfileIsolation:undefined},
    {...preparedRequest,codexProfileIsolation:'unknown'}, {...preparedRequest,codexProfileLoan:{}},
    {...preparedRequest,inventoryDeadlineUnixMs:undefined}, {...preparedRequest,inventoryDeadlineUnixMs:0},
    {...preparedRequest,inventoryDeadlineUnixMs:120001}, {...preparedRequest,inventoryDeadlineUnixMs:'1000'},
    {...preparedRequest,inventoryDeadlineUnixMs:NaN}, {...preparedRequest,inventoryDeadlineUnixMs:1.5}])
    assert.equal(preparedPolicy(invalid,true,'win32',env,0),false);
  assert(!preparedPolicy(preparedRequest,false,'win32',env,0));
  for(const platform of ['darwin','linux'])assert(!preparedPolicy({...preparedRequest,codexProfileLoan:{}},true,platform,env,0));
  for(const key of Object.keys(env))assert(!preparedPolicy(preparedRequest,true,'win32',{...env,[key]:'other'},0),key);
  {
    const actual=fixture();
    const factoryBegin=inventory.indexOf('function heldMainGuard('),factoryEnd=inventory.indexOf('// One public page activation',factoryBegin);
    const same=(a,b)=>a.page===b.page&&a.loader===b.loader&&a.url===b.url;
    const context={require,Date:{now:actual.clock},sameCorrelationIdentity:same,settleFolderAuxiliary:async()=>false};
    const actualFactory=vm.runInNewContext(inventory.slice(factoryBegin,factoryEnd)+';heldMainGuard',context);
    const wiringBegin=inventory.indexOf('const spotlightPolicy='),wiringEnd=inventory.indexOf('folderTrust=await',wiringBegin);
    const wired=vm.runInNewContext(`let trustGuard;${inventory.slice(wiringBegin,wiringEnd)}};trustGuard;`,{
      spotlightPreparedWindowsPolicy:preparedPolicy,request:preparedRequest,directCDP:true,started:0,trial:true,
      process:{platform:'win32',env},profileAuthority:null,focusGuard:null,heldMainGuard:actualFactory,
      initialMain:actual.identities(actual.main),browser:actual.browser,onboardingOwnerGuard:actual.owner,
      correlationDeadline:1000,correlationIdentity:actual.identity,checkpoint:()=>{},facts:{},save:()=>{},
      setTimeout:(resolve,ms)=>{actual.setClock(actual.clock()+ms);queueMicrotask(resolve);},
      require:name=>name==='./codex-folder-trust.cjs'?{authority:()=>({})}:require(name),
    });
    assert(await wired(),'production Windows wiring admits the closed source without a Unix Node loan');
    assert.equal(actual.readCount(),2);assert.equal(wired.spotlightAuxiliary().status,'verified');
    await wired.close();assert(actual.handles().every(h=>h.disposed));
  }
  const begin=inventory.indexOf('facts.codexSession='),end=inventory.indexOf('if(!bindingVerified||!codingComposerReady)',begin);
  const facts={};vm.runInNewContext(inventory.slice(begin,end),{facts,directCDP:true,bindingVerified:true,codingComposerReady:true,browser:f.browser,mainGuard:f.guard});
  const fixturePath=process.argv[2];if(fixturePath)fs.writeFileSync(fixturePath,JSON.stringify({folderTrust,codexSession:facts.codexSession})+'\n',{mode:0o600});
  await f.guard.close();assert(f.handles().every(h=>h.disposed));
  f=fixture();
  const providerNames=['--font-code-family','--font-ui-family','--codex-content-font-family','--font-ui-size',
    '--text-4xl','--text-3xl','--text-2xl','--text-xl','--text-lg','--text-base','--text-sm','--text-xs',
    '--text-dynamic-isle-row','--text-heading-lg','--text-heading-app','--text-heading-md','--text-heading-sm',
    '--cursor-interaction','--text-document'];
  f.dom().document.body.attributes.style+='; '+providerNames.map(name=>`${name}: SYNTHETIC_PRIVATE_VALUE`).join('; ');
  assert(await f.guard(),'the source theme/font provider may update BODY cosmetic variables');
  assert(!JSON.stringify(f.guard.spotlightAuxiliary()).includes('SYNTHETIC_PRIVATE_VALUE'));
  assert(!JSON.stringify(f.guard.spotlightAuxiliary()).includes('SYNTHETIC_PRIVATE_NOTIFICATION_LABEL'));
  await f.guard.close();
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
    ['foreign-controls',d=>{d.document.body.attributes.role='button';}],
    ['foreign-controls',d=>{d.document.body.attributes.contenteditable='true';}],
    ['foreign-controls',d=>{d.document.body.attributes['aria-modal']='true';}],
    ['foreign-controls',d=>{d.document.body.attributes.tabindex='1';d.document.body.tabIndex=1;}],
    ['foreign-controls',d=>{d.document.body.attributes.style='outline: none; pointer-events: auto';}],
    ...['opacity: 0','position: fixed','--unknown-property: none','--font-code-size: 12px',
      '--codex-window-zoom: 1','-webkit-font-smoothing: antialiased','outline-color: red',
      '--font-ui-family: synthetic !important'].map(style=>['foreign-controls',d=>{d.document.body.attributes.style+='; '+style;}]),
    ['initial-scope',d=>{d.state.root.children=[];}],
  ];
  for(const [reason,change] of cases) {
    f=fixture({beforeRead:({dom})=>change(dom)});assert.equal(await f.guard(),false,reason);
    assert.equal(f.guard.spotlightAuxiliary().failure,reason);assert.equal(f.readCount(),1);
    // A rejected lease cannot reopen after its source returns to a safe shape.
    assert.equal(await f.guard(),false);assert.equal(f.readCount(),1);await f.guard.close();
  }
  const rejectionFixtures=[];
  for(const [check,change] of [
    ['body-anchor',d=>{d.document.body.attributes.role='button';}],
    ['body-style',d=>{d.document.body.attributes.style+='; pointer-events: auto';}],
    ['initial-controls',d=>{d.state.root=null;}],
    ['forbidden-nodes',d=>{d.state.frames=[d.node()];}],
    ['control-location',d=>{d.state.foreign=[d.node()];}],
    ['control-pointer',d=>{d.state.input.style.pointerEvents='auto';}],
    ['editable-scope',d=>{d.state.input.style.display='none';}],
    ['main-controls',d=>{const legend=d.node({},d.state.command);legend.innerText='Select the kind of work you do';d.state.legends=[legend];}],
  ]) {
    f=fixture({beforeRead:({dom:d})=>change(d)});assert.equal(await f.guard(),false,check);
    const receipt=f.guard.spotlightAuxiliary();assert.equal(receipt.failure,'foreign-controls');
    assert.equal(receipt.rejectionCheck,check);assert.equal(f.readCount(),1);rejectionFixtures.push(receipt);
    assert.equal(Object.hasOwn(receipt,'controlKind'),['control-location','control-pointer'].includes(check));
    assert.equal(await f.guard(),false);assert.equal(f.readCount(),1);await f.guard.close();
  }
  for(const expected of ['command','input','list','disabled-button','button','anchor','select','other']) {
    f=fixture({beforeRead:({dom:d})=>{
      let control=d.state[expected];
      if(!control){control=d.node({},d.state.command);control.tagName=({button:'BUTTON','disabled-button':'BUTTON',anchor:'A',select:'SELECT'})[expected]??'SPAN';
        control.disabled=expected==='disabled-button';d.state.foreign=[control];}
      control.style.pointerEvents='auto';
    }});
    assert.equal(await f.guard(),false,expected);const receipt=f.guard.spotlightAuxiliary();
    assert.equal(receipt.rejectionCheck,'control-pointer');assert.equal(receipt.controlKind,expected);rejectionFixtures.push(receipt);
    assert.equal(f.readCount(),1);await f.guard.close();
  }
  f=fixture({beforeRead:({dom:d})=>{
    const guard=d.node({'data-radix-focus-guard':'',tabindex:'0'},d.document.body);
    guard.tagName='SPAN';guard.tabIndex=0;d.state.foreign=[guard];
  }});
  assert.equal(await f.guard(),false,'source Radix markers remain refusals, never an outside-control exception');
  assert.equal(f.guard.spotlightAuxiliary().rejectionCheck,'control-location');
  assert.equal(f.guard.spotlightAuxiliary().controlKind,'radix-focus-guard');rejectionFixtures.push(f.guard.spotlightAuxiliary());
  await f.guard.close();
  for(const change of [
    d=>{d.state.toast.section.children.push({nodeType:3});},
    d=>{d.node({},d.state.toast.section).tagName='OL';},
    d=>{d.node({},d.state.toast.spacer).tagName='BUTTON';},
    d=>{d.state.toast.section.attributes.role='button';},
    d=>{d.state.toast.section.attributes.tabindex='0';d.state.toast.section.tabIndex=0;},
    d=>{d.state.toast.section.attributes['aria-live']='assertive';},
    d=>{d.state.toast.section.attributes['aria-relevant']='all';},
    d=>{d.state.toast.section.attributes['aria-atomic']='true';},
    d=>{d.state.toast.portal.attributes.class+=' pointer-events-auto';},
    d=>{d.state.toast.outer.attributes.class+=' extra';},
    d=>{d.state.toast.portal.attributes.style='inset: 1px;';},
    d=>{d.state.toast.portal.attributes.style+=' opacity: 0;';},
    d=>{d.state.toast.outer.attributes.style='inset-inline-end: 1px;';},
    d=>{d.state.toast.outer.attributes.style+=' transform: none;';},
    d=>{d.state.toast.portal.attributes.style='inset: 0px !important;';},
    d=>{d.state.toast.portal.computed.pointerEvents='auto';},
    d=>{d.state.toast.section.style.pointerEvents='auto';},
    d=>{d.state.toast.inner.shadowRoot={};},
    d=>{d.state.toast.outer.attributes.tabindex='-1';},
    d=>{d.state.toast.inner.attributes.onclick='private';},
    d=>{d.state.toast.portal.parentElement=d.node({},d.document.body);},
    d=>{d.state.extraPortals=[d.makeToast().portal];},
  ]) {
    f=fixture({beforeRead:({dom:d})=>change(d)});assert.equal(await f.guard(),false,'only the exact empty passive source announcer is allowed');
    const receipt=f.guard.spotlightAuxiliary();assert.equal(receipt.failure,'foreign-controls');assert.equal(receipt.rejectionCheck,'toast-announcer');
    assert.equal(receipt.toastAnnouncerCount,null);assert.equal(receipt.controlKind,undefined);assert.equal(f.readCount(),1);
    rejectionFixtures.push(receipt);assert.equal(await f.guard(),false);assert.equal(f.readCount(),1);await f.guard.close();
  }
  for(const replace of ['portal','section','label','disappear']) {
    f=fixture({beforeRead:({reads,dom:d})=>{
      if(reads!==2)return;
      if(replace==='portal')d.state.toast=d.makeToast();
      if(replace==='section'){const t=d.state.toast,old=t.section,next=d.node({...old.attributes},t.inner);next.tagName='SECTION';next.tabIndex=-1;t.inner.children=[next];t.section=next;}
      if(replace==='label')d.state.toast.section.attributes['aria-label']='DIFFERENT_PRIVATE_NOTIFICATION_LABEL';
      if(replace==='disappear')d.state.toast=null;
    }});
    assert.equal(await f.guard(),false,replace);const receipt=f.guard.spotlightAuxiliary();
    assert.equal(receipt.rejectionCheck,'toast-announcer');assert.equal(f.readCount(),2);rejectionFixtures.push(receipt);
    assert.equal(await f.guard(),false);assert.equal(f.readCount(),2);await f.guard.close();
  }
  {
    let original;
    f=fixture({beforeRead:({reads,dom:d})=>{if(reads===1){original=d.state.toast;d.state.toast=null;}if(reads===3)d.state.toast=original;}});
    assert(await f.guard(),'mandatory source announcer may appear only during original initial admission');
    assert.equal(f.readCount(),4,'two initial passive waits are followed by two fresh complete source samples');
    assert.equal(f.clock(),300);assert.equal(f.guard.spotlightAuxiliary().toastAnnouncerCount,1);await f.guard.close();
  }
  f=fixture();f.dom().state.toast=null;f.guard.sealInitialActions();
  assert.equal(await f.guard(),false,'announcer absence cannot wait after any main action');assert.equal(f.readCount(),1);await f.guard.close();
  f=fixture();assert(await f.guard());f.dom().state.toast=null;
  assert.equal(await f.guard(),false);const afterRemoval=f.readCount();f.dom().state.toast=f.dom().makeToast();
  assert.equal(await f.guard(),false,'a captured announcer cannot be replaced after admission');assert.equal(f.readCount(),afterRemoval);await f.guard.close();
  if(process.argv[3])fs.writeFileSync(process.argv[3],JSON.stringify(rejectionFixtures)+'\n',{mode:0o600});
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
  for(const pending of [false,true]) {
    let original;
    f=fixture({beforeRead:({reads,dom:d})=>{
      if(reads===1&&pending){original={...d.state};Object.assign(d.state,{root:null,motion:null,command:null,input:null,list:null,option:null});}
      if(reads===2){
        const old=d.document.body,replacement=d.node({...old.attributes},d.document.documentElement);
        Object.assign(replacement,{tagName:'BODY',tabIndex:0,isContentEditable:false});
        old.isConnected=false;d.document.body=replacement;
        if(pending)Object.assign(d.state,original);
        d.state.root.parentElement=replacement;
      }
    }});
    assert.equal(await f.guard(),false,'body replacement is terminal even before the lazy root arrives');
    assert.equal(f.guard.spotlightAuxiliary().failure,'scope-changed');assert.equal(f.readCount(),2);
    assert.equal(await f.guard(),false);assert.equal(f.readCount(),2);await f.guard.close();
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
