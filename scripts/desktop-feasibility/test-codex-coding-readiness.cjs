const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
const start=source.indexOf('function codingScope('),end=source.indexOf('// Exact immutable final-onboarding',start);
function read(options={},diagnostic=true) {
 const node=(text='',extra={})=>({textContent:text,children:[],isConnected:true,getBoundingClientRect:()=>({width:100,height:30}),
   closest:()=>null,getAttribute:()=>null,...extra});
 const composer=node('',{getAttribute:()=>options.disabled?'true':null});
 const ack='Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.';
 const homeRoot=node('',{getAttribute:k=>k==='data-testid'?'chatgpt-work-home-page':null,
   contains:e=>e!==homeRoot});
 const localHome=node('',{getAttribute:k=>k==='data-codex-composer-root'?'':k==='data-composer-placement'?'home':null,
   contains:e=>e===homeEditor});
 const homeEditor=node('PRIVATE owned draft',{getAttribute:k=>k==='contenteditable'?'true':null,
   classList:{contains:k=>k==='ProseMirror'&&!options.nonProseMirror}});
 const workspace=node('PRIVATE project name',{tagName:'BUTTON',getAttribute:k=>k==='data-composer-navigation-target'?'workspace-project':null});
 const sourceNodes=options.sourceHome?[homeRoot,localHome,homeEditor,workspace]:[];
 if(options.sourceHome)homeEditor.parentElement=localHome;
 if(options.ancestryMarker) {
  const attrs=options.ancestryMarker;const parent=node('',{getAttribute:k=>attrs[k]??null});
  homeEditor.parentElement=parent;sourceNodes.push(homeEditor,parent);
  if(options.cycle)parent.parentElement=parent;
  if(options.depth) {let current=parent;for(let i=0;i<64;i++){current.parentElement=node();current=current.parentElement;}}
  if(options.hiddenEditable)homeEditor.isConnected=false;
 }
 if(options.nodeOverflow)for(let i=0;i<4097;i++)sourceNodes.push(node());
 if(options.sidebar) {
  const sidebar=node('New chat',{tagName:'BUTTON',classList:{contains:k=>k==='sidebar-item'},
   getAttribute:k=>k==='type'?'button':null,getBoundingClientRect:()=>({left:1,top:1,width:100,height:30}),contains:()=>false});
  sourceNodes.push(sidebar);if(options.duplicateSidebar)sourceNodes.push(sidebar);
 }
 if(options.hiddenHome)homeRoot.isConnected=false;
 if(options.detachedHome)localHome.getAttribute=k=>k==='data-codex-composer-root'?'':k==='data-composer-placement'?'thread':null;
 const selectors={
  '[data-thread-find-composer] .ProseMirror[contenteditable="true"]':options.noComposer?[]:[composer],
  '[data-thread-find-target="conversation"]':options.noConversation?[]:[node()],
  '[role="dialog"],[role="alertdialog"],[role="menu"],[aria-modal="true"]':options.modal?[node()]:[],
  'input[name="conversational-onboarding-inline-role"]':options.roles?Array.from({length:options.roles},()=>node()):[],
  '*':[...(options.ack?[node(ack)]:[node('PRIVATE user content')]),...sourceNodes],
  'button':[node('PRIVATE button'),...(options.skip?[node('Skip')]:[]),...(options.start?[node('Get Started')]:[])]};
 return vm.runInNewContext(`(()=>{${source.slice(start,end)}return codingScope(${diagnostic});})()`,{
  document:{querySelectorAll:s=>selectors[s],documentElement:{clientWidth:800,clientHeight:600},
   elementFromPoint:()=>options.covered?node():sourceNodes.find(e=>e.textContent==='New chat')},getComputedStyle:()=>({display:'block',visibility:'visible'})});
}
assert.equal(read({},false),true);
for(const options of [{noComposer:true},{noConversation:true},{modal:true},{disabled:true}]) {
 const result=read(options);assert.equal(result.ready,false);assert.equal(result.observation.status,'observed');
}
const home=read({noConversation:true,skip:true,ack:true});
assert.equal(home.observation.composerCount,1);assert.equal(home.observation.conversationCount,0);
assert.equal(home.observation.exactSkipCount,1);assert.equal(home.observation.exactAckLeafCount,1);assert.equal(home.ready,false);
assert(!JSON.stringify(home).includes('PRIVATE'));
const overflow=read({roles:33});assert.equal(overflow.observation.status,'overflow');
for(const [key,value] of Object.entries(overflow.observation))if(key!=='status')assert.equal(value,null);
assert.equal(overflow.ready,true); // Advisory counters never change established admission.
console.log('PASS: coding readiness source counts, unchanged admission, overflow and privacy');

const codingHome=read({sourceHome:true,noComposer:true,noConversation:true});
assert.equal(codingHome.ready,false);
assert.deepEqual(JSON.parse(JSON.stringify(codingHome.home)),{
 status:'observed',sourcePlatform:'linux',sourceVersion:'26.930.41038',
 composerSourceSha256:'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
 pageSourceSha256:'9c9d0d9247226d43edeb4606a539b518e3be06fb65aa37bd014984fbe3998ba9',
 homeRootCount:1,localHomeComposerCount:1,homeEditableCount:1,homeProseMirrorCount:1,workspaceControlCount:1});
assert.equal(read({sourceHome:true,nonProseMirror:true}).home.homeProseMirrorCount,0);
assert.equal(read({sourceHome:true,hiddenHome:true}).home.localHomeComposerCount,0);
assert.equal(read({sourceHome:true,detachedHome:true}).home.homeEditableCount,0);
assert(!JSON.stringify(codingHome).includes('PRIVATE'));
console.log('PASS: exact Work home source counts do not admit input or expose draft/project data');

for(const [attrs,category] of [
 [{'data-codex-composer-root':'','data-composer-placement':'home'},'codexHomeCount'],
 [{'data-codex-composer-root':'','data-composer-placement':'thread'},'codexThreadCount'],
 [{'data-codex-composer-root':'','data-composer-placement':'PRIVATE unknown'},'codexOtherCount'],
 [{'data-chatgpt-composer':'','data-composer-input':''},'classicChatGPTCount'],
 [{'data-composer-input':'','data-composer-body':''},'genericInputCount'],
 [{'data-composer-body':''},'genericBodyCount'],[{'data-wrong-marker':''},'unboundCount']]) {
 const result=read({ancestryMarker:attrs,nonProseMirror:true});
 assert.equal(result.ancestry.editableCount,1);assert.equal(result.ancestry[category],1);
 assert(!JSON.stringify(result).includes('PRIVATE'));
}
assert.equal(read({ancestryMarker:{'data-composer-input':''},hiddenEditable:true}).ancestry.editableCount,0);
for(const options of [{cycle:true},{depth:true},{nodeOverflow:true}]) {
 const result=read({ancestryMarker:{'data-composer-input':''},...options}).ancestry;
 assert.equal(result.status,'overflow');
 for(const [key,value] of Object.entries(result))if(key.endsWith('Count')||key==='sidebarNewChatHitActionable')assert.equal(value,null);
}
assert.equal(read({sidebar:true}).ancestry.sidebarNewChatHitActionable,true);
assert.equal(read({sidebar:true,covered:true}).ancestry.sidebarNewChatHitActionable,false);
assert.equal(read({sidebar:true,duplicateSidebar:true}).ancestry.sidebarNewChatCount,2);
assert.equal(read({sidebar:true,duplicateSidebar:true}).ancestry.sidebarNewChatHitActionable,false);
console.log('PASS: source editable partition, bounded ancestry, sidebar diagnostic and privacy');
