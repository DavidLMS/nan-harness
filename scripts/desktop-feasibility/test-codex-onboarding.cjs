const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
async function trial(options={}) {
 let now=0,checked=false,absent=false,roleClicks=0,continueClicks=0,samples=0;
 const root={parentElement:null};
 const fieldset={parentElement:root};
 const label={kind:'label',tagName:'LABEL',parentElement:fieldset,innerText:'Engineering'};
 const radio={kind:'radio',parentElement:label,labels:[label],disabled:false};
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
  constructor(kind){this.kind=kind;}
  filter(){return this;}
  locator(s){return new Locator(s==='..'?'fieldset':s.startsWith('xpath=')?'scope':s==='fieldset'?'fieldset':s==='label'?'label':s.includes(':checked')?'checked':s.includes('value=')?'radio':'radios');}
  getByRole(_r,o){return new Locator(o.name==='Continue'?'continue':'login');}
  async count(){return this.kind==='login'?0:this.kind==='legend'?(options.wrongLegend?0:1):this.kind==='radios'?(absent?0:11):this.kind==='checked'?(checked?(options.multipleChecked?2:1):0):this.kind==='continue'?(options.duplicateContinue?2:1):this.kind==='radio'?(options.duplicateRadio?2:1):1;}
  async isEnabled(){return this.kind!=='continue'||!options.disabled;}
  async isChecked(){return checked;}
  element(){return this.kind==='label'?label:this.kind==='continue'?button:this.kind==='radio'?radio:this.kind==='fieldset'?fieldset:root;}
  async evaluate(fn,arg){if(options.remount&&arg instanceof Handle&&samples>0)return false;return evaluate(fn,this.element(),arg);}
  async elementHandle(){return new Handle(this.element());}
 }
 const page={locator:s=>new Locator(s.startsWith('fieldset > legend')?'legend':'radios'),url:()=> 'app://codex/index.html',context:()=>({browser:()=>({contexts:()=>[{pages:()=>options.foreignPage?[page,{}]:[page]}]})})};
 const sandbox={exports:{},process:{platform:'win32',env:{GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',NANH_CODEX_PUBLIC_ONBOARDING:options.noOptin?undefined:'engineering'}},Date:{now:()=>now},setTimeout:f=>{now+=100;f();}};
 vm.runInNewContext(source,sandbox);
 const facts=await sandbox.exports.run(page,()=>!(options.ownerLoss&&roleClicks>0),1200);
 return {facts,roleClicks,continueClicks};
}
(async()=>{
 const good=await trial();assert.equal(good.roleClicks,1);assert.equal(good.continueClicks,1);assert.equal(good.facts.roleScopeAbsent,true);assert.equal(good.facts.stage,'stopped-after-role');assert.equal(good.facts.errorCategory,null);
 for(const opts of [{noOptin:true},{foreignPage:true},{wrongLegend:true},{duplicateRadio:true},{modal:true},{intercepted:'label'},{remount:true}]) {
  const r=await trial(opts);assert.equal(r.roleClicks,0);assert.equal(r.continueClicks,0);
 }
 for(const opts of [{readbackFail:true},{multipleChecked:true},{ownerLoss:true},{disabled:true},{duplicateContinue:true},{intercepted:'continue'},{uncertain:'role'}]) {
  const r=await trial(opts);assert.equal(r.roleClicks,1);assert.equal(r.continueClicks,0);
 }
 for(const opts of [{remain:true},{uncertain:'continue'}]) {const r=await trial(opts);assert.equal(r.roleClicks,1);assert.equal(r.continueClicks,1);assert.equal(r.facts.roleScopeAbsent,false);}
 assert.equal(JSON.stringify(good.facts).includes('Engineering'),false);
 assert.equal(JSON.stringify(good.facts).includes('private'),false);
 console.log('PASS: public onboarding behavioral guards (17 cases)');
})().catch(e=>{console.error(e);process.exitCode=1;});
