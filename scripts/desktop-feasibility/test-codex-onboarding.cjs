const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
async function trial(options={}) {
 let now=0,checked=false,absent=false,roleClicks=0,continueClicks=0,samples=0;
 const root={parentElement:null};
 const fieldset={parentElement:root};
 const label={kind:'label',tagName:'LABEL',parentElement:fieldset,innerText:'Engineering'};
 const radio={kind:'radio',parentElement:label,labels:options.badAssociation?[]:[label],disabled:false};
 const button={kind:'continue',parentElement:root,tagName:'BUTTON',disabled:!!options.disabled};
 const foreign={kind:'foreign'};
 const doc={querySelectorAll: selector=>selector.startsWith('input')?[radio]:options.modal?[foreign]:[],
  elementFromPoint:()=>options.intercepted===lastKind?foreign:elements[lastKind]};
 let lastKind='label';const elements={label,continue:button};
 for(const e of [root,fieldset,label,radio,button,foreign]) Object.assign(e,{ownerDocument:doc,isConnected:true,
  clientLeft:0,clientTop:0,clientWidth:80,clientHeight:40,
  getBoundingClientRect:()=>({left:10,top:10,width:80,height:40}),
  getAttribute:()=>null,closest:()=>null,contains:x=>x===e||(e===label&&x===radio)});
 const globals={document:doc,innerWidth:800,innerHeight:600,getComputedStyle:()=>({display:'block',visibility:'visible',pointerEvents:'auto'})};
 function evaluate(fn,e,arg) {
  if(fn.name==='sample') {lastKind=e.kind;samples++;}
  const f=vm.runInNewContext(`(${fn.toString()})`,globals);
  return f(e,arg?.element??arg);
 }
 class Handle {
  constructor(e){this.element=e;}
  async evaluate(fn,arg){return evaluate(fn,this.element,arg);}
  async click(params){assert.equal(params.force,undefined);assert.ok(params.position);if(this.element===label){roleClicks++;if(options.uncertain==='role')throw Error('private');checked=!options.readbackFail;}
   else {continueClicks++;if(options.uncertain==='continue')throw Error('private');absent=!options.remain;}}
  async dispose(){}
 }
 class Locator {
  constructor(kind){this.kind=kind;if(kind==='roleFieldsets')this.members=[{legend:true,group:true},...(options.extraFieldset?[{legend:false,group:false}]:[]),...(options.duplicateRoleFieldset?[{legend:true,group:true}]:[])];}
  filter(predicate){if(this.members&&predicate.has)this.members=this.members.filter(e=>predicate.has.kind==='legend'?e.legend:e.group);return this;}
  locator(s){return new Locator(s==='..'?'fieldset':s.startsWith('xpath=')?'scope':s==='fieldset:visible'?'roleFieldsets':s==='fieldset'?'fieldset':s==='label'?'label':s.includes(':checked')?'checked':s.includes('value=')?'radio':'radios');}
  getByRole(_r,o){return new Locator(o.name==='Continue'?'continue':'login');}
  async count(){return this.kind==='label'?(options.duplicateLabel?2:1):this.kind==='scope'?(options.badScope?0:1):this.kind==='fieldset'?(options.extraFieldset?2:1):this.kind==='login'?(options.login?1:0):this.kind==='roleFieldsets'?this.members.length:this.kind==='legend'?(options.wrongLegend?0:options.duplicateRoleFieldset?2:1):this.kind==='radios'?(absent||options.noGroup?0:11):this.kind==='checked'?(checked?(options.multipleChecked?2:1):0):this.kind==='continue'?(options.duplicateContinue?2:1):this.kind==='radio'?(options.duplicateRadio?2:1):1;}
  async isEnabled(){return this.kind==='radio'?!(options.radioDisabled||options.loading&&now<300):this.kind!=='continue'||!options.disabled;}
  async isChecked(){return checked;}
  element(){return this.kind==='label'?label:this.kind==='continue'?button:this.kind==='radio'?radio:this.kind==='fieldset'?fieldset:root;}
  async evaluate(fn,arg){if(options.remount&&arg instanceof Handle&&samples>0)return false;return evaluate(fn,this.element(),arg);}
  async elementHandle(){return new Handle(this.element());}
 }
 const page={locator:s=>new Locator(s.startsWith('fieldset > legend')?'legend':'radios'),url:()=> 'app://codex/index.html',context:()=>({browser:()=>({contexts:()=>[{pages:()=>options.foreignPage?[page,{}]:[page]}]})})};
 const sandbox={exports:{},process:{platform:options.platform??'win32',env:{GITHUB_ACTIONS:options.noHost?'false':'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',NANH_CODEX_PUBLIC_ONBOARDING:options.noOptin?undefined:'engineering'}},Date:{now:()=>now},setTimeout:f=>{now+=100;f();}};
 vm.runInNewContext(source,sandbox);
 let guards=0;
 const guard=()=>{guards++;if(options.guardExhaustsBudget&&guards>=3)now=1201;return !options.initialOwnerLoss&&!(options.ownerLossBeforeRole&&guards>=3)&&!(options.ownerLoss&&roleClicks>0)&&!(options.finalLoss&&guards>=2);};
 const budget=options.expired?0:options.invalidDeadline?NaN:options.excessBudget?25001:1200;
 const facts=await sandbox.exports.run(page,options.noGuard?undefined:guard,budget);
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
 const scoped=await trial({extraFieldset:true});assert.equal(scoped.roleClicks,1);assert.equal(scoped.continueClicks,1);assert.equal(scoped.facts.roleScopeAbsent,true);
 const duplicateScope=await trial({duplicateRoleFieldset:true});assert.equal(duplicateScope.facts.roleProofFailure,'legend-count');assert.equal(duplicateScope.roleClicks,0);assert.equal(duplicateScope.continueClicks,0);
 const duplicate=await trial({duplicateRadio:true});assert.equal(duplicate.facts.roleProofFailure,'engineering-count');
 const legend=await trial({wrongLegend:true});assert.equal(legend.facts.roleProofFailure,'legend-count');
 for(const [opts,reason] of [[{badScope:true},'scope-count'],[{login:true},'login-present'],[{noGroup:true},'group-absent'],[{duplicateLabel:true},'label-count'],[{badAssociation:true},'label-association'],[{finalLoss:true},'ownership-lost']]) {const r=await trial(opts);assert.equal(r.facts.roleProofFailure,reason);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);}
 const good=await trial();assert.equal(good.roleClicks,1);assert.equal(good.continueClicks,1);assert.equal(good.facts.roleScopeAbsent,true);assert.equal(good.facts.stage,'stopped-after-role');assert.equal(good.facts.errorCategory,null);
 for(const opts of [{noOptin:true},{foreignPage:true},{wrongLegend:true},{duplicateRadio:true},{badScope:true},{duplicateRoleFieldset:true},{login:true},{radioDisabled:true},{modal:true},{intercepted:'label'},{remount:true}]) {
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
