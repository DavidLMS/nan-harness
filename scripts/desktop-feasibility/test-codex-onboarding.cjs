const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
async function trial(options={}) {
 let inventoryOwnerLost=false;
 let now=0,checked=false,absent=false,roleClicks=0,continueClicks=0,taskClicks=0,samples=0,overlayReads=0,legendReads=0;
 const root={parentElement:null};
 const fieldset={parentElement:root};
 const label={kind:'label',tagName:'LABEL',parentElement:fieldset,innerText:'Engineering'};
 const radio={kind:'radio',parentElement:label,labels:options.badAssociation?[]:[label],disabled:false};
 const button={kind:'continue',parentElement:root,tagName:'BUTTON',disabled:!!options.disabled};
 const foreign={kind:'foreign'},dialog={kind:'dialog'},legend={innerText:'Select the kind of work you do'};
 const doc={querySelectorAll: selector=>selector.startsWith('input')?[radio]:options.modal?[foreign]:[],
  elementFromPoint:()=>options.intercepted===lastKind?foreign:elements[lastKind]};
 let lastKind='label';const elements={label,continue:button};
 for(const e of [root,fieldset,label,radio,button,foreign,dialog,legend]) Object.assign(e,{ownerDocument:doc,isConnected:true,
  clientLeft:0,clientTop:0,clientWidth:80,clientHeight:40,
  getBoundingClientRect:()=>({left:10,top:10,width:80,height:40}),
  hasAttribute:()=>false,getAttribute:()=>null,closest:()=>null,contains:x=>x===e||(e===label&&x===radio)});
 dialog.getAttribute=key=>key==='role'?(options.alertDialog?'alertdialog':'dialog'):null;
 dialog.contains=e=>[root,fieldset,label,radio,button,legend,startControl].includes(e);
 dialog.querySelectorAll=selector=>selector.startsWith('input')?(options.ambiguousDialog?[radio,radio]:[radio]):[legend];
 if(options.onboardingDialog||options.alertDialog||options.ambiguousDialog||options.duplicateDialog)
  doc.querySelectorAll=selector=>selector.startsWith('input')?[radio]:options.duplicateDialog?[dialog,foreign]:[dialog];
 const form={classList:{contains:t=>(options.computerHistory?['pointer-events-auto','relative','hide-scrollbar','flex','flex-col','gap-6','overflow-y-auto','pb-10']:['m-auto','flex','w-full','shrink-0','flex-col','items-center','justify-between','py-4']).includes(t)&&!options.overlayWrongLayout}};
 const heading={tagName:options.computerHistory?'H2':'DIV',classList:{contains:t=>options.computerHistory&&['heading-dialog','select-none'].includes(t)},innerText:options.overlayHeading??(options.overlayLookalike?'All set':"You're all set")};
 const finish={innerText:'Continue',disabled:false,getAttribute:k=>k==='type'?'submit':null};
 const terms={classList:{contains:()=>true},getAttribute:()=>options.overlayWrongLink?'https://example.invalid':'https://openai.com/terms'};
 const privacy={classList:{contains:()=>true},getAttribute:()=> 'https://openai.com/privacy'};
 for(const e of [form,heading,finish,terms,privacy]) Object.assign(e,{isConnected:true,
  getBoundingClientRect:()=>({left:10,top:10,width:80,height:40})});
 form.contains=e=>e===heading;
 form.querySelectorAll=s=>s==='button'?[finish]:s==='a'?[terms,privacy]:[];
 root.contains=e=>[label,radio,button,startControl].includes(e);
 const acknowledgement={textContent:'Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.',children:[],isConnected:true,closest:()=>null,getBoundingClientRect:()=>({width:80,height:40})};
 const startControl={...button,kind:'task',textContent:options.optionalSkip?'Skip':'Get Started',children:[]};elements.task=startControl;
 const footer={tagName:'DIV',classList:{contains:t=>!options.wrongFooter&&['relative','flex','shrink-0','flex-col','items-center','gap-3','px-10','pt-8','pb-12'].includes(t)}};
 startControl.parentElement={parentElement:footer};
 root.contains=e=>[label,radio,button,startControl,footer].includes(e);
 root.querySelectorAll=s=>absent?(s.startsWith('input')?[]:s==='button'?(options.duplicateTask?[startControl,startControl]:[startControl]):options.noTaskScope?[]:[acknowledgement]):s.startsWith('input')?[radio]:[legend];
 label.closest=s=>s.startsWith('div')?root:null;
 foreign.getAttribute=k=>k==='role'?'dialog':null;
 const setupButtons=(options.importSetupButtons??[]).map(innerText=>({...button,innerText,getAttribute:k=>k==='type'?(innerText.startsWith('Allow')?'submit':'button'):null}));
 foreign.querySelectorAll=s=>s.startsWith('input')?[]:s==='button'?setupButtons:s==='form'?[form]:s.startsWith('[class')?(options.overlayWrongHeadingStyle?[]:[heading]):s==='[role="heading"],h1,h2,h3'?(options.overlayDuplicateHeading?[heading,heading]:[heading]):[];
 if(options.overlayDialogReplacement)doc.querySelectorAll=s=>s.startsWith('input')?[radio]:overlayReads?[{...foreign}]:[foreign];
 if(options.overlayMultiple)doc.querySelectorAll=s=>s.startsWith('input')?[radio]:overlayReads?[foreign,dialog]:[foreign];
 foreign.parentElement=options.overlayAncestor?{ownerDocument:doc,isConnected:true,parentElement:null,
   inert:!!options.overlayAncestorInert,hasAttribute:()=>false,getAttribute:k=>k==='data-state'&&options.overlayClosed?'closed':null}:null;
 foreign.hasAttribute=k=>k==='inert'&&!!options.overlayInert;
 if(options.overlayFractional)label.getBoundingClientRect=()=>({left:10,top:10,width:79.6,height:39.6});
 if(options.overlayOutside)label.getBoundingClientRect=()=>({left:-20,top:10,width:80,height:40});
 if(options.overlayLongAncestors) {let current=foreign;for(let i=0;i<65;i++){current.parentElement={ownerDocument:doc,isConnected:true,parentElement:null,inert:false,hasAttribute:()=>false,getAttribute:()=>null};current=current.parentElement;}}
 let diagnosticPoints=0;if(options.overlayMixedPoints)doc.elementFromPoint=()=>[label,foreign,dialog][diagnosticPoints++%3];
 if(options.overlayPointFront)doc.elementFromPoint=()=>options.overlayPointFront==='dialog'?foreign:options.overlayPointFront==='other'?dialog:label;
 const globals={document:doc,innerWidth:800,innerHeight:600,getComputedStyle:e=>({display:options.hiddenTask&&e===startControl?'none':'block',visibility:'visible',
   opacity:options.overlayBadOpacity?'PRIVATE':e===foreign?(options.overlayZero?'0':'1'):
     e===foreign.parentElement&&options.overlayAncestorZero?'0':'1',
   pointerEvents:e===foreign&&options.overlayPointerNone?'none':e===foreign.parentElement&&options.overlayAncestorPointerNone?'none':'auto'})};
 function evaluate(fn,e,arg) {
  if(fn.name==='sample') {lastKind=e.kind;samples++;}
  if(fn.name==='classifyForeign'){overlayReads++;if(options.overlayHeadingChanges&&overlayReads>1)heading.innerText='Skip setup?';if(options.overlayQueryFail)throw Error('PRIVATE query failed');}
  if(options.overlayReplacement&&fn.name==='classifyForeign'&&overlayReads>1)globals.document={...doc};
  const f=vm.runInNewContext(`(${fn.toString()})`,globals);
  return f(e,arg?.element??arg?.value??arg);
 }
 class Handle {
  constructor(e){this.element=e;this.disposed=false;}
  async evaluate(fn,arg){if(this.disposed)throw Error('PRIVATE disposed handle');return evaluate(fn,this.element,arg);}
  async click(params){assert.equal(params.force,undefined);assert.ok(params.position);if(this.element===label){roleClicks++;if(options.uncertain==='role')throw Error('private');checked=!options.readbackFail;}
   else if(this.element.kind==='task'){taskClicks++;if(options.uncertain==='task')throw Error('private');}
   else {continueClicks++;if(options.uncertain==='continue')throw Error('private');absent=!options.remain;if(options.scopeReplaced)root.isConnected=false;}}
  async evaluateHandle(fn){return {value:evaluate(fn,this.element),dispose:async()=>{}};}
  async dispose(){this.disposed=true;}
 }
 class Locator {
  constructor(kind){this.kind=kind;if(kind==='roleFieldsets')this.members=[{legend:true,group:true},...(options.extraFieldset?[{legend:false,group:false}]:[]),...(options.duplicateRoleFieldset?[{legend:true,group:true}]:[])];}
  filter(predicate){if(this.members&&predicate.has)this.members=this.members.filter(e=>predicate.has.kind==='legend'?e.legend:e.group);return this;}
  locator(s){return new Locator(s==='..'?'fieldset':s.startsWith('xpath=')?'scope':s==='fieldset:visible'?'roleFieldsets':s==='fieldset'?'fieldset':s==='label'?'label':s.includes(':checked')?'checked':s.includes('value=')?'radio':'radios');}
  getByRole(_r,o){return new Locator(o.name==='Continue'?'continue':o.name==='Get Started'?'task':'login');}
  async count(){
   if(this.kind==='legend'){legendReads++;if(options.overlayDeadlineDuringProof&&legendReads>=3)now=1201;if(options.overlayLegendLost&&legendReads>=3)return 0;}
   if(this.kind==='scope'&&options.overlayScopeLost&&legendReads>=3)return 0;
   return this.kind==='label'?(options.duplicateLabel?2:1):this.kind==='scope'?(absent||options.badScope?0:1):this.kind==='fieldset'?(options.extraFieldset?2:1):this.kind==='login'?(options.login?1:0):this.kind==='roleFieldsets'?this.members.length:this.kind==='legend'?(options.wrongLegend?0:options.duplicateRoleFieldset?2:1):this.kind==='radios'?(absent||options.noGroup?0:11):this.kind==='checked'?(checked?(options.multipleChecked?2:1):0):this.kind==='continue'?(options.duplicateContinue?2:1):this.kind==='radio'?(options.duplicateRadio?2:1):this.kind==='task'?(options.duplicateTask?2:1):1;}
  async isEnabled(){return this.kind==='radio'?!(options.radioDisabled||options.loading&&now<300):this.kind!=='continue'||!options.disabled;}
  async isChecked(){return checked;}
  element(){return this.kind==='label'?label:this.kind==='continue'?button:this.kind==='task'?startControl:this.kind==='radio'?radio:this.kind==='fieldset'?fieldset:root;}
  async evaluate(fn,arg){if(options.remount&&arg instanceof Handle&&samples>0)return false;return evaluate(fn,this.element(),arg);}
  async elementHandle(){return new Handle(this.element());}
 }
 const mainFrame={};
 const extraPage={url:()=>options.foreignUrl??'about:blank',evaluate:async()=>{if(options.inventoryOwnerLoss)inventoryOwnerLost=true;if(options.inventoryDeadline)now=1201;return options.visibility??'hidden';}};
 const page={getByRole:(_role,options)=>new Locator(['Get Started','Skip'].includes(options.name)?'task':'login'),evaluate:async fn=>fn.name==='codingScope'?taskClicks===1&&!options.noCodingScope:'visible',mainFrame:()=>options.overlayFrameChange&&overlayReads?{}:mainFrame,locator:s=>new Locator(s.startsWith('fieldset > legend')?'legend':'radios'),
  url:()=>options.urlChange&&roleClicks>0?'app://codex/index.html?PRIVATE_ROUTE':'app://codex/index.html',
  context:()=>({browser:()=>({contexts:()=>[{pages:()=>options.foreignPage?[page,extraPage]:options.replacedPage?[{}]:[page]}]})})};
 const sandbox={exports:{},URL,require,process:{platform:options.platform??'win32',env:{GITHUB_ACTIONS:options.noHost?'false':'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:options.runnerOs??'Windows',NANH_CODEX_PROJECT_ARTIFACT_SHA256:options.skipPin?'ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c':undefined,NANH_CODEX_PUBLIC_ONBOARDING:options.noOptin?undefined:'engineering'}},Date:{now:()=>now},clearTimeout:()=>{},setTimeout:(f,ms)=>{if(ms<=100){now+=100;f();}}};
 vm.runInNewContext(source,sandbox);
 let guards=0,mainProofs=0,sealed=0;
 const guard=()=>{guards++;if(options.guardThrows)throw Error('PRIVATE');if(options.guardExhaustsBudget&&guards>=3||options.overlayBudgetExpired&&overlayReads>0)now=1201;return !inventoryOwnerLost&&!(options.overlayOwnerDuringProof&&legendReads>=3)&&!(options.overlayOwnerLoss&&overlayReads>0)&&!options.initialOwnerLoss&&!(options.ownerLossBeforeRole&&guards>=3)&&!(options.ownerLoss&&roleClicks>0)&&!(options.finalLoss&&guards>=2);};
 const budget=options.expired?0:options.invalidDeadline?NaN:options.excessBudget?60001:options.fullBudget?60000:1200;
 let mainGuard=options.admitAux?async()=>{
  mainProofs++;
  if(options.auxDeadlineAfterProof||options.auxDeadlineFailedProof)now=1201;
  return !options.auxGuardFailure&&!options.auxDeadlineFailedProof
    &&!(options.auxOwnershipLostDuringProof&&legendReads>0)&&(!options.auxOwnershipLostAfterClick||roleClicks===0);
 }:undefined;
 if(mainGuard)mainGuard.sealInitialActions=()=>{sealed++;};
 if(mainGuard)mainGuard.failureDetails=()=>options.pageSetDetails;
 if(mainGuard)mainGuard.failure=()=>options.auxDeadlineFailedProof?'deadline':options.auxGuardFailure??'native-ownership';
 if(options.realMainGuard) {
  const mainIdentity={url:page.url(),target:'main',frame:'frame',loader:'loader',frameUrl:page.url(),fragment:''};
  mainGuard=require('./codex-main-guard.cjs').createHeldMainGuard(
    {...mainIdentity,page},page.context().browser(),options.differentGuardOwner?()=>true:guard,budget,()=> 'avatarOverlay',
    async()=>{mainProofs++;if(options.sampleOwnerLoss)inventoryOwnerLost=true;
      return {...mainIdentity,scope:{focused:true,mainScope:true,counts:{}}};},
    async()=>{},false,false,true,
    {sameCorrelationIdentity:(a,b)=>['url','target','frame','loader','frameUrl','fragment'].every(k=>a[k]===b[k]),
     settleFolderAuxiliary:async()=>false,now:()=>now});
 }
 if(options.forgedMainGuard) {
  mainGuard=async()=>{mainProofs++;return true;};
  mainGuard.includesNativeOwnership=true;
 }
 const facts=await sandbox.exports.run(page,options.noGuard?undefined:guard,budget,mainGuard);
 assert(!JSON.stringify(facts).includes('PRIVATE'));
 return {facts,roleClicks,continueClicks,taskClicks,mainProofs,legendReads,sealed,guards};
}
(async()=>{
 const real=await trial({realMainGuard:true});
 assert(real.roleClicks>0);assert.equal(real.guards,real.mainProofs*2);
 const different=await trial({realMainGuard:true,differentGuardOwner:true,initialOwnerLoss:true});
 assert.equal(different.roleClicks,0);assert.equal(different.mainProofs,0);assert.equal(different.guards,1);
 const lost=await trial({realMainGuard:true,sampleOwnerLoss:true});
 assert.equal(lost.roleClicks,0);assert.equal(lost.continueClicks,0);
 assert.equal(lost.guards,2);assert.equal(lost.mainProofs,1);
 for(const forgedMainGuard of [false,true]) {
  const forged=await trial({forgedMainGuard,admitAux:!forgedMainGuard,initialOwnerLoss:true});
  assert.equal(forged.roleClicks,0);assert.equal(forged.continueClicks,0);
  assert.equal(forged.mainProofs,0);
 }

 const sealedAttempt=await trial({admitAux:true,uncertain:'role'});
 assert.equal(sealedAttempt.sealed,1);
 assert.equal(sealedAttempt.roleClicks,1);
 assert.equal(sealedAttempt.continueClicks,0);
 assert.equal(sealedAttempt.facts.roleClickCompleted,false);
 const noInputSeal=await trial({admitAux:true,initialOwnerLoss:true});
 assert.equal(noInputSeal.sealed,0);
 for(const reason of ['main-focus','auxiliary-controls','query-failed']) {
  const blocked=await trial({admitAux:true,auxGuardFailure:reason});
  assert.equal(blocked.facts.mainGuardFailure,reason);assert.equal(blocked.roleClicks,0);assert.equal(blocked.continueClicks,0);
 }
 const details={reason:'after-sample-changed',initialCount:1,currentCount:2,heldPresent:true};
 const pageSet=await trial({admitAux:true,auxGuardFailure:'page-set',pageSetDetails:details});
 assert.deepEqual(JSON.parse(JSON.stringify(pageSet.facts.pageSetFailure)),details);
 assert.equal(pageSet.roleClicks,0);assert.equal(pageSet.continueClicks,0);
 const latePageSet=await trial({admitAux:true,auxGuardFailure:'page-set',
   pageSetDetails:details,auxDeadlineAfterProof:true});
 assert.equal(latePageSet.facts.roleProofFailure,'deadline-expired');
 assert.equal(latePageSet.facts.mainGuardFailure,'page-set');
 assert.deepEqual(JSON.parse(JSON.stringify(latePageSet.facts.pageSetFailure)),details);

 for(const invalid of [{...details,reason:'PRIVATE'}, {...details,currentCount:33},
   {...details,path:'PRIVATE'}, {...details,heldPresent:1}]) {
  const blocked=await trial({admitAux:true,auxGuardFailure:'page-set',pageSetDetails:invalid});
  assert.equal(blocked.facts.pageSetFailure,undefined);assert.equal(blocked.roleClicks,0);
 }
 const failedLate=await trial({admitAux:true,auxDeadlineFailedProof:true});
 assert.equal(failedLate.facts.roleProofFailure,'deadline-expired');
 assert.equal(failedLate.facts.mainGuardFailure,'deadline');assert.equal(failedLate.roleClicks,0);
 const privateFailure=await trial({admitAux:true,auxGuardFailure:'PRIVATE unsupported'});
 assert.equal(privateFailure.facts.mainGuardFailure,undefined);assert.equal(privateFailure.roleClicks,0);

 const freshAuxLost=await trial({foreignPage:true,admitAux:true,auxOwnershipLostDuringProof:true});
 assert.equal(freshAuxLost.roleClicks,0);
 assert.equal(freshAuxLost.continueClicks,0);
 assert(freshAuxLost.legendReads>0); // A fresh post-DOM proof still runs; no cross-await cache.
 const auxClock=await trial({foreignPage:true,admitAux:true,auxDeadlineAfterProof:true});
 assert.equal(auxClock.facts.roleProofFailure,'deadline-expired');
 assert.equal(auxClock.roleClicks,0);
 assert.equal(auxClock.continueClicks,0);
 const afterClickLost=await trial({foreignPage:true,admitAux:true,auxOwnershipLostAfterClick:true});
 assert.equal(afterClickLost.roleClicks,1);
 assert.equal(afterClickLost.continueClicks,0);

 for(const [opts,reason] of [[{expired:true},'deadline-expired'],[{invalidDeadline:true},'deadline-invalid'],
  [{excessBudget:true},'deadline-invalid'],[{noGuard:true},'guard-missing'],[{platform:'freebsd'},'platform'],[{platform:'linux'},'host-policy'],
  [{noHost:true},'host-policy'],[{noOptin:true},'onboarding-policy']]) {
  const r=await trial(opts);assert.equal(r.facts.errorCategory,'invalid-session');assert.equal(r.facts.sessionProofFailure,reason);
  assert.equal(r.facts.roleProofFailure,'unmeasured');assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const [opts,reason] of [[{guardExhaustsBudget:true},'deadline-expired'],[{initialOwnerLoss:true},'ownership-lost'],[{ownerLossBeforeRole:true},'ownership-lost']]) {
  const r=await trial(opts);assert.equal(r.facts.roleProofFailure,reason);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
  if(opts.guardExhaustsBudget||opts.ownerLossBeforeRole)assert.equal(r.facts.conversationalScope,true);
 }
 const mac=await trial({platform:'darwin',runnerOs:'macOS'});assert.equal(mac.facts.codingComposerReady,true);
 const fullBudget=await trial({fullBudget:true});assert.equal(fullBudget.facts.codingComposerReady,true);
 const linux=await trial({platform:'linux',runnerOs:'Linux'});assert.equal(linux.facts.codingComposerReady,true);
 const loaded=await trial({loading:true});assert.equal(loaded.roleClicks,1);assert.equal(loaded.continueClicks,1);
 for(const [url,kind] of [['about:blank','blank'],['app://codex/PRIVATE','app'],['devtools://PRIVATE','devtools'],['https://PRIVATE','other']]){
  const r=await trial({foreignPage:true,foreignUrl:url});
  assert.equal(r.facts.roleProofFailure,'page-count');assert.equal(r.facts.rejectedPageInventory.total,2);
  assert.equal(r.facts.rejectedPageInventory.held,1);assert.equal(r.facts.rejectedPageInventory[kind],kind==='app'?2:1);
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const [opts,reason] of [[{foreignPage:true},'page-count'],[{replacedPage:true},'page-changed'],[{guardThrows:true},'query-failed']]) {
  const r=await trial(opts);assert.equal(r.facts.roleProofFailure,reason);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const [url,kind] of [['app://-/index.html?initialRoute=/chatgpt/quick-chat-prewarm','quickChatPrewarm'],
  ['app://-/detached-window.html?initialRoute=/detached-window','detachedWindow'],
  ['app://foreign/index.html?initialRoute=/avatar-overlay','unknown'],
  ['app://-/index.html?initialRoute=/debug&initialRoute=/avatar-overlay','unknown']]){
  const r=await trial({foreignPage:true,foreignUrl:url});
  assert.equal(r.facts.rejectedPageInventory.source.routes[kind],1+(kind==='unknown'?1:0));
  assert.equal(r.facts.rejectedPageInventory.source.visibility.hidden,1);
  assert.equal(r.facts.rejectedPageInventory.source.visibility.visible,1);
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const opts of [{inventoryOwnerLoss:true},{inventoryDeadline:true}]){
  const r=await trial({foreignPage:true,...opts});
  assert.equal(r.facts.rejectedPageInventory.source.status,'unavailable');
  assert.equal(Object.keys(r.facts.rejectedPageInventory.source).length,1);
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 const route=await trial({urlChange:true});assert.equal(route.facts.roleProofFailure,'url-changed');
 assert.equal(route.roleClicks,1);assert.equal(route.continueClicks,0);
 const scoped=await trial({extraFieldset:true});assert.equal(scoped.roleClicks,1);assert.equal(scoped.continueClicks,1);assert.equal(scoped.facts.roleScopeAbsent,true);
 const duplicateScope=await trial({duplicateRoleFieldset:true});assert.equal(duplicateScope.facts.roleProofFailure,'legend-count');assert.equal(duplicateScope.roleClicks,0);assert.equal(duplicateScope.continueClicks,0);
 const duplicate=await trial({duplicateRadio:true});assert.equal(duplicate.facts.roleProofFailure,'engineering-count');
 const legend=await trial({wrongLegend:true});assert.equal(legend.facts.roleProofFailure,'legend-count');
 for(const [opts,reason] of [[{badScope:true},'scope-count'],[{login:true},'login-present'],[{noGroup:true},'group-absent'],[{duplicateLabel:true},'label-count'],[{badAssociation:true},'label-association'],[{finalLoss:true},'ownership-lost']]) {const r=await trial(opts);assert.equal(r.facts.roleProofFailure,reason);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);}
 const good=await trial();assert.equal(good.roleClicks,1);assert.equal(good.continueClicks,1);assert.equal(good.facts.roleScopeAbsent,true);assert.equal(good.facts.stage,'coding-readiness');assert.equal(good.facts.errorCategory,null);
 const history=await trial({modal:true,computerHistory:true,overlayHeading:'Connect Computer History',importSetupButtons:['Customize apps','Not now','Allow all apps']});
 assert.equal(history.facts.foreignOverlay,'other');
 assert.equal(history.facts.foreignOverlayFingerprint,'computer-history-consent');
 assert.equal(history.facts.foreignOverlaySourceCounts.computerHistoryTitleCount,1);
 assert.equal(history.roleClicks+history.continueClicks,0);
 for(const change of [{overlayWrongLayout:true},{importSetupButtons:['Not now']},{importSetupButtons:['Customize apps','Not now','Not now']}]) {
  const result=await trial({modal:true,computerHistory:true,overlayHeading:'Connect Computer History',importSetupButtons:['Customize apps','Not now','Allow all apps'],...change});
  assert.notEqual(result.facts.foreignOverlayFingerprint,'computer-history-consent');
  assert.equal(result.roleClicks+result.continueClicks,0);
 }
 const completeOverlay=await trial({modal:true});
 assert.equal(completeOverlay.facts.foreignOverlay,'chatgpt-onboarding-complete');
 assert.equal(completeOverlay.facts.foreignOverlayProof,'classified');
 assert.equal(completeOverlay.facts.foreignOverlaySurface,'separate-dialog');
 assert.equal(completeOverlay.facts.foreignOverlayFingerprint,'matched');
 assert.equal(completeOverlay.facts.foreignOverlayHeading,'all-set');
 for(const [label,expected] of [["You're all set",'all-set'],['Import from other AI apps','external-import'],['Skip setup?','skip-confirmation'],['PRIVATE unknown heading','unknown']]) {
  const r=await trial({modal:true,overlayHeading:label,overlayWrongHeadingStyle:true});
  assert.equal(r.facts.foreignOverlayHeading,expected);
  assert.equal(r.facts.foreignOverlayFingerprint,'heading-mismatch');
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const [buttons,expected] of [[['Continue','Not now'],'imported-setup'],
    [['Continue','Skip'],'imported-setup'],[['Continue'],'unknown'],
    [['Continue','Not now','Skip'],'unknown'],[['Continue','Not now','Not now'],'unknown']]) {
  const r=await trial({modal:true,overlayHeading:'Continue with your existing setup',
    overlayWrongHeadingStyle:true,importSetupButtons:buttons});
  assert.equal(r.facts.foreignOverlayHeading,expected);
  assert.equal(r.facts.foreignOverlayImportSetup.titleCount,1);
  assert.equal(r.facts.foreignOverlayImportSetup.continueCount,1);
  assert.equal(r.facts.foreignOverlayFingerprint,'heading-mismatch');
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 const importDuplicate=await trial({modal:true,overlayHeading:'Continue with your existing setup',
   overlayDuplicateHeading:true,overlayWrongHeadingStyle:true,importSetupButtons:['Continue','Not now']});
 assert.equal(importDuplicate.facts.foreignOverlayHeading,'ambiguous');
 assert.equal(importDuplicate.facts.foreignOverlayImportSetup.titleCount,2);
 const importChanges=await trial({modal:true,overlayHeading:'Continue with your existing setup',
   overlayWrongHeadingStyle:true,overlayHeadingChanges:true,importSetupButtons:['Continue','Not now']});
 assert.equal(importChanges.facts.foreignOverlayProof,'unstable-classification');
 assert.equal(importChanges.facts.foreignOverlayImportSetup,undefined);
 const duplicateHeading=await trial({modal:true,overlayDuplicateHeading:true});
 assert.equal(duplicateHeading.facts.foreignOverlayHeading,'ambiguous');
 assert.equal(duplicateHeading.roleClicks,0);assert.equal(duplicateHeading.continueClicks,0);
 const changedHeading=await trial({modal:true,overlayHeadingChanges:true});
 assert.equal(changedHeading.facts.foreignOverlayProof,'unstable-classification');
 assert.equal(changedHeading.facts.foreignOverlayHeading,undefined);
 assert.equal(changedHeading.roleClicks,0);assert.equal(changedHeading.continueClicks,0);
 for(const [opts,fingerprint] of [[{overlayLookalike:true},'heading-mismatch'],[{overlayWrongLink:true},'legal-links-mismatch'],[{overlayWrongLayout:true},'form-mismatch']]) {
  const r=await trial({modal:true,...opts});
  assert.equal(r.facts.foreignOverlaySurface,'separate-dialog');assert.equal(r.facts.foreignOverlayFingerprint,fingerprint);
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 const alert=await trial({alertDialog:true});assert.equal(alert.facts.foreignOverlaySurface,'enclosing-role-alertdialog');
 assert.equal(alert.facts.foreignOverlayFingerprint,'not-applicable');assert.equal(alert.roleClicks,0);assert.equal(alert.continueClicks,0);
 assert.equal(completeOverlay.roleClicks,0);assert.equal(completeOverlay.continueClicks,0);
 for(const [opts,category] of [[{overlayLookalike:true},'other'],[{overlayWrongLink:true},'other'],[{overlayWrongLayout:true},'other'],
   [{overlayDialogReplacement:true},'guard-rejected'],[{overlayFrameChange:true},'guard-rejected'],
   [{overlayReplacement:true},'guard-rejected'],[{overlayMultiple:true},'ambiguous'],[{overlayOwnerLoss:true},'guard-rejected']]) {
  const r=await trial({modal:true,...opts});assert.equal(r.facts.foreignOverlay,category);
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const [opts,proof] of [[{overlayDialogReplacement:true},'dialog-replaced'],[{overlayFrameChange:true},'frame-replaced'],
   [{overlayReplacement:true},'document-replaced'],[{overlayOwnerLoss:true},'ownership-lost'],[{overlayQueryFail:true},'query-failed'],[{overlayBudgetExpired:true},'deadline-expired']]) {
  const r=await trial({modal:true,...opts});assert.equal(r.facts.foreignOverlay,'guard-rejected');
  assert.equal(r.facts.foreignOverlayProof,proof);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
  assert.equal(r.facts.foreignOverlaySurface,undefined);assert.equal(r.facts.foreignOverlayFingerprint,undefined);
  assert(!JSON.stringify(r.facts).includes('PRIVATE'));
 }
 for(const [opts,roleFailure,overlayProof] of [
  [{overlayLegendLost:true},'legend-count','role-proof-rejected'],
  [{overlayScopeLost:true},'scope-count','role-proof-rejected'],
  [{overlayDeadlineDuringProof:true},'deadline-expired','deadline-expired'],
  [{overlayOwnerDuringProof:true},'ownership-lost','ownership-lost']]) {
  const r=await trial({modal:true,...opts});
  assert.equal(r.facts.roleProofFailure,roleFailure);assert.equal(r.facts.foreignOverlayProof,overlayProof);
  assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
  assert.equal(r.facts.foreignOverlayHeading,undefined);
 }
 assert(!JSON.stringify(completeOverlay.facts).includes("You're all set"));
 assert(!JSON.stringify(completeOverlay.facts).includes('openai.com'));
 // Passive admission of the held main/known auxiliary does not admit a
 // separate modal or waive ordinary control actionability.
 const admittedWithForeignDialog=await trial({foreignPage:true,admitAux:true,modal:true});
 assert.equal(admittedWithForeignDialog.facts.actionabilityFailure,'foreign-overlay');
 assert.equal(admittedWithForeignDialog.roleClicks,0);
 assert.equal(admittedWithForeignDialog.continueClicks,0);
 const knownAux=await trial({foreignPage:true,admitAux:true});
 assert.equal(knownAux.roleClicks,1);assert.equal(knownAux.continueClicks,1);assert.equal(knownAux.facts.taskScopeProved,true);
 const lostAux=await trial({foreignPage:true,admitAux:true,auxOwnershipLostAfterClick:true});
 assert.equal(lostAux.roleClicks,1);assert.equal(lostAux.continueClicks,0);assert.equal(lostAux.facts.roleProofFailure,'page-count');
 const retainedTransition=await trial();
 assert.equal(retainedTransition.facts.taskScopeProved,true);assert.equal(retainedTransition.taskClicks,1);
 for(const options of [{scopeReplaced:true},{duplicateTask:true}]) {
  const rejected=await trial(options);assert.equal(rejected.continueClicks,1);assert.equal(rejected.taskClicks,0);
 }
 const missingTask=await trial({noTaskScope:true});
 assert.equal(missingTask.facts.taskScopeObservation.heldScopeConnected,true);
 assert.equal(missingTask.facts.taskScopeObservation.exactAckLeafCount,0);
 assert.equal(missingTask.facts.taskScopeObservation.exactGetStartedCount,1);
 assert.equal(missingTask.continueClicks,1);assert.equal(missingTask.facts.roleScopeAbsent,true);assert.equal(missingTask.facts.taskScopeProved,false);assert.equal(missingTask.facts.errorCategory,'scope-remained');
 const detachedTask=await trial({scopeReplaced:true});
 assert.equal(detachedTask.facts.taskScopeObservation.heldScopeConnected,false);
 assert.equal(detachedTask.facts.taskScopeObservation.roleRadioCount,null);
 assert.equal(detachedTask.taskClicks,0);
 const duplicateTask=await trial({duplicateTask:true});
 assert.equal(duplicateTask.facts.taskScopeObservation.exactGetStartedCount,2);
 assert.equal(duplicateTask.taskClicks,0);
 assert(!JSON.stringify(missingTask.facts.taskScopeObservation).includes('Engineering'));
 const uncertainTask=await trial({uncertain:'task'});
 assert.equal(uncertainTask.taskClicks,1);assert.equal(uncertainTask.facts.taskClickAttempted,true);
 assert.equal(uncertainTask.facts.taskClickCompleted,false);assert.equal(uncertainTask.facts.codingComposerReady,false);
 const missingComposer=await trial({noCodingScope:true});
 assert.equal(missingComposer.taskClicks,1);assert.equal(missingComposer.facts.taskClickCompleted,true);
 assert.equal(missingComposer.facts.codingComposerReady,false);
 const ownDialog=await trial({onboardingDialog:true});assert.equal(ownDialog.roleClicks,1);assert.equal(ownDialog.continueClicks,1);assert.equal(ownDialog.facts.roleScopeAbsent,true);
 assert.equal((await trial({modal:true})).facts.roleProofFailure,'control-not-actionable');
 assert.equal((await trial({modal:true})).facts.actionabilityFailure,'foreign-overlay');
 assert.equal((await trial({duplicateDialog:true})).facts.actionabilityFailure,'ambiguous-overlays');
 assert.equal((await trial({intercepted:'label'})).facts.actionabilityFailure,'no-owned-point');
 for(const opts of [{noOptin:true},{foreignPage:true},{wrongLegend:true},{duplicateRadio:true},{badScope:true},{duplicateRoleFieldset:true},{login:true},{radioDisabled:true},{modal:true},{alertDialog:true},{ambiguousDialog:true},{duplicateDialog:true},{intercepted:'label'},{remount:true}]) {
  const r=await trial(opts);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const opts of [{readbackFail:true},{multipleChecked:true},{ownerLoss:true},{disabled:true},{duplicateContinue:true},{intercepted:'continue'},{uncertain:'role'}]) {
  const r=await trial(opts);assert.equal(r.roleClicks,1);assert.equal(r.continueClicks,0);
 }
 for(const opts of [{remain:true},{uncertain:'continue'}]) {const r=await trial(opts);assert.equal(r.roleClicks,1);assert.equal(r.continueClicks,1);assert.equal(r.facts.roleScopeAbsent,false);}
 assert.equal(JSON.stringify(good.facts).includes('Engineering'),false);
 assert.equal(JSON.stringify(good.facts).includes('private'),false);
 for(const opts of [{overlayZero:true},{overlayPointerNone:true},{overlayInert:true},
   {overlayAncestor:true,overlayAncestorZero:true,overlayAncestorPointerNone:true,overlayAncestorInert:true,overlayClosed:true},
   {overlayFractional:true},{overlayPointFront:'dialog'},{overlayPointFront:'other'},{overlayMixedPoints:true},{overlayLongAncestors:true},{overlayOutside:true},{overlayBadOpacity:true}]) {
   const r=await trial({modal:true,overlayHeading:'Unknown',...opts});
   assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
   assert.equal(r.facts.actionabilityFailure,'foreign-overlay');
   const m=r.facts.foreignOverlayActionability;assert.ok(m);
   if(opts.overlayOutside||opts.overlayBadOpacity||opts.overlayLongAncestors) {
     assert.equal(m.status,'unavailable');assert.equal(m.targetOwnedPointCount,null);
     assert.equal(m.unavailableReason,opts.overlayOutside?'geometry-outside':opts.overlayBadOpacity?'opacity-invalid':'ancestor-limit');
     assert.equal(m.dialogOwnedPointCount,null);assert.equal(m.otherPointCount,null);
   } else {
     assert.equal(m.status,'observed');assert.equal(m.unavailableReason,null);
     assert.equal(m.targetOwnedPointCount+m.dialogOwnedPointCount+m.otherPointCount,9);
     if(opts.overlayZero)assert.equal(m.dialogOpacityZero,true);
     if(opts.overlayPointerNone)assert.equal(m.dialogPointerEventsNone,true);
     if(opts.overlayInert)assert.equal(m.inert,true);
     if(opts.overlayAncestor) {assert.equal(m.ancestorOpacityZero,true);assert.equal(m.ancestorPointerEventsNone,true);assert.equal(m.inert,true);assert.equal(m.stateClosed,true);}
     if(opts.overlayMixedPoints)assert.deepEqual([m.targetOwnedPointCount,m.dialogOwnedPointCount,m.otherPointCount],[3,3,3]);
     if(opts.overlayPointFront==='dialog')assert.equal(m.dialogOwnedPointCount,9);
     if(opts.overlayPointFront==='other')assert.equal(m.otherPointCount,9);
   }
 }
 const skip=await trial({optionalSkip:true,skipPin:true,platform:'linux',runnerOs:'Linux'});
 assert.equal(skip.taskClicks,1);assert.equal(skip.facts.taskControlKind,'skip-optional-capabilities');assert.equal(skip.facts.codingComposerReady,true);
 for(const options of [{hiddenTask:true},{wrongFooter:true},{duplicateTask:true},{scopeReplaced:true},{noTaskScope:true},{skipPin:false},{platform:'win32',runnerOs:'Windows'}]) {
  const denied=await trial({optionalSkip:true,skipPin:true,platform:'linux',runnerOs:'Linux',...options});
  assert.equal(denied.taskClicks,0);
 }
 console.log('PASS: public onboarding behavioral guards (closed proof branches + guarded loading)');
})().catch(e=>{console.error(e);process.exitCode=1;});
