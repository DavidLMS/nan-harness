const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
async function trial(options={}) {
 let inventoryOwnerLost=false;
 let now=0,checked=false,absent=false,roleClicks=0,continueClicks=0,samples=0,overlayReads=0,legendReads=0;
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
  getAttribute:()=>null,closest:()=>null,contains:x=>x===e||(e===label&&x===radio)});
 dialog.getAttribute=key=>key==='role'?(options.alertDialog?'alertdialog':'dialog'):null;
 dialog.contains=e=>[root,fieldset,label,radio,button,legend].includes(e);
 dialog.querySelectorAll=selector=>selector.startsWith('input')?(options.ambiguousDialog?[radio,radio]:[radio]):[legend];
 if(options.onboardingDialog||options.alertDialog||options.ambiguousDialog||options.duplicateDialog)
  doc.querySelectorAll=selector=>selector.startsWith('input')?[radio]:options.duplicateDialog?[dialog,foreign]:[dialog];
 const form={classList:{contains:t=>['m-auto','flex','w-full','shrink-0','flex-col','items-center','justify-between','py-4'].includes(t)&&!(options.overlayWrongLayout&&t==='m-auto')}};
 const heading={innerText:options.overlayHeading??(options.overlayLookalike?'All set':"You're all set")};
 const finish={innerText:'Continue',disabled:false,getAttribute:k=>k==='type'?'submit':null};
 const terms={classList:{contains:()=>true},getAttribute:()=>options.overlayWrongLink?'https://example.invalid':'https://openai.com/terms'};
 const privacy={classList:{contains:()=>true},getAttribute:()=> 'https://openai.com/privacy'};
 for(const e of [form,heading,finish,terms,privacy]) Object.assign(e,{isConnected:true,
  getBoundingClientRect:()=>({left:10,top:10,width:80,height:40})});
 form.contains=e=>e===heading;
 form.querySelectorAll=s=>s==='button'?[finish]:s==='a'?[terms,privacy]:[];
 root.contains=e=>[label,radio,button].includes(e);
 root.querySelectorAll=s=>s.startsWith('input')?[radio]:[legend];
 label.closest=s=>s.startsWith('div')?root:null;
 foreign.getAttribute=k=>k==='role'?'dialog':null;
 foreign.querySelectorAll=s=>s.startsWith('input')?[]:s==='form'?[form]:s.startsWith('[class')?(options.overlayWrongHeadingStyle?[]:[heading]):s==='[role="heading"],h1,h2,h3'?(options.overlayDuplicateHeading?[heading,heading]:[heading]):[];
 if(options.overlayDialogReplacement)doc.querySelectorAll=s=>s.startsWith('input')?[radio]:overlayReads?[{...foreign}]:[foreign];
 if(options.overlayMultiple)doc.querySelectorAll=s=>s.startsWith('input')?[radio]:overlayReads?[foreign,dialog]:[foreign];
 const globals={document:doc,innerWidth:800,innerHeight:600,getComputedStyle:()=>({display:'block',visibility:'visible',pointerEvents:'auto'})};
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
   else {continueClicks++;if(options.uncertain==='continue')throw Error('private');absent=!options.remain;}}
  async evaluateHandle(fn){return {value:evaluate(fn,this.element),dispose:async()=>{}};}
  async dispose(){this.disposed=true;}
 }
 class Locator {
  constructor(kind){this.kind=kind;if(kind==='roleFieldsets')this.members=[{legend:true,group:true},...(options.extraFieldset?[{legend:false,group:false}]:[]),...(options.duplicateRoleFieldset?[{legend:true,group:true}]:[])];}
  filter(predicate){if(this.members&&predicate.has)this.members=this.members.filter(e=>predicate.has.kind==='legend'?e.legend:e.group);return this;}
  locator(s){return new Locator(s==='..'?'fieldset':s.startsWith('xpath=')?'scope':s==='fieldset:visible'?'roleFieldsets':s==='fieldset'?'fieldset':s==='label'?'label':s.includes(':checked')?'checked':s.includes('value=')?'radio':'radios');}
  getByRole(_r,o){return new Locator(o.name==='Continue'?'continue':'login');}
  async count(){
   if(this.kind==='legend'){legendReads++;if(options.overlayDeadlineDuringProof&&legendReads>=3)now=1201;if(options.overlayLegendLost&&legendReads>=3)return 0;}
   if(this.kind==='scope'&&options.overlayScopeLost&&legendReads>=3)return 0;
   return this.kind==='label'?(options.duplicateLabel?2:1):this.kind==='scope'?(options.badScope?0:1):this.kind==='fieldset'?(options.extraFieldset?2:1):this.kind==='login'?(options.login?1:0):this.kind==='roleFieldsets'?this.members.length:this.kind==='legend'?(options.wrongLegend?0:options.duplicateRoleFieldset?2:1):this.kind==='radios'?(absent||options.noGroup?0:11):this.kind==='checked'?(checked?(options.multipleChecked?2:1):0):this.kind==='continue'?(options.duplicateContinue?2:1):this.kind==='radio'?(options.duplicateRadio?2:1):1;}
  async isEnabled(){return this.kind==='radio'?!(options.radioDisabled||options.loading&&now<300):this.kind!=='continue'||!options.disabled;}
  async isChecked(){return checked;}
  element(){return this.kind==='label'?label:this.kind==='continue'?button:this.kind==='radio'?radio:this.kind==='fieldset'?fieldset:root;}
  async evaluate(fn,arg){if(options.remount&&arg instanceof Handle&&samples>0)return false;return evaluate(fn,this.element(),arg);}
  async elementHandle(){return new Handle(this.element());}
 }
 const mainFrame={};
 const extraPage={url:()=>options.foreignUrl??'about:blank',evaluate:async()=>{if(options.inventoryOwnerLoss)inventoryOwnerLost=true;if(options.inventoryDeadline)now=1201;return options.visibility??'hidden';}};
 const page={evaluate:async()=> 'visible',mainFrame:()=>options.overlayFrameChange&&overlayReads?{}:mainFrame,locator:s=>new Locator(s.startsWith('fieldset > legend')?'legend':'radios'),
  url:()=>options.urlChange&&roleClicks>0?'app://codex/index.html?PRIVATE_ROUTE':'app://codex/index.html',
  context:()=>({browser:()=>({contexts:()=>[{pages:()=>options.foreignPage?[page,extraPage]:options.replacedPage?[{}]:[page]}]})})};
 const sandbox={exports:{},URL,process:{platform:options.platform??'win32',env:{GITHUB_ACTIONS:options.noHost?'false':'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',NANH_CODEX_PUBLIC_ONBOARDING:options.noOptin?undefined:'engineering'}},Date:{now:()=>now},clearTimeout:()=>{},setTimeout:(f,ms)=>{if(ms<=100){now+=100;f();}}};
 vm.runInNewContext(source,sandbox);
 let guards=0;
 const guard=()=>{guards++;if(options.guardThrows)throw Error('PRIVATE');if(options.guardExhaustsBudget&&guards>=3||options.overlayBudgetExpired&&overlayReads>0)now=1201;return !inventoryOwnerLost&&!(options.overlayOwnerDuringProof&&legendReads>=3)&&!(options.overlayOwnerLoss&&overlayReads>0)&&!options.initialOwnerLoss&&!(options.ownerLossBeforeRole&&guards>=3)&&!(options.ownerLoss&&roleClicks>0)&&!(options.finalLoss&&guards>=2);};
 const budget=options.expired?0:options.invalidDeadline?NaN:options.excessBudget?25001:1200;
 const facts=await sandbox.exports.run(page,options.noGuard?undefined:guard,budget);
 assert(!JSON.stringify(facts).includes('PRIVATE'));
 return {facts,roleClicks,continueClicks};
}
(async()=>{
 for(const [opts,reason] of [[{expired:true},'deadline-expired'],[{invalidDeadline:true},'deadline-invalid'],
  [{excessBudget:true},'deadline-invalid'],[{noGuard:true},'guard-missing'],[{platform:'linux'},'platform'],
  [{noHost:true},'host-policy'],[{noOptin:true},'onboarding-policy']]) {
  const r=await trial(opts);assert.equal(r.facts.errorCategory,'invalid-session');assert.equal(r.facts.sessionProofFailure,reason);
  assert.equal(r.facts.roleProofFailure,'unmeasured');assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const [opts,reason] of [[{guardExhaustsBudget:true},'deadline-expired'],[{initialOwnerLoss:true},'ownership-lost'],[{ownerLossBeforeRole:true},'ownership-lost']]) {
  const r=await trial(opts);assert.equal(r.facts.roleProofFailure,reason);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
  if(opts.guardExhaustsBudget||opts.ownerLossBeforeRole)assert.equal(r.facts.conversationalScope,true);
 }
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
 const good=await trial();assert.equal(good.roleClicks,1);assert.equal(good.continueClicks,1);assert.equal(good.facts.roleScopeAbsent,true);assert.equal(good.facts.stage,'stopped-after-role');assert.equal(good.facts.errorCategory,null);
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
 console.log('PASS: public onboarding behavioral guards (closed proof branches + guarded loading)');
})().catch(e=>{console.error(e);process.exitCode=1;});
