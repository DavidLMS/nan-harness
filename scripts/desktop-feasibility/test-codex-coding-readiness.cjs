const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
const start=source.indexOf('function codingScope('),end=source.indexOf('// Exact immutable final-onboarding',start);
function read(options={},diagnostic=true) {
 const node=(text='',extra={})=>({textContent:text,children:[],isConnected:true,getBoundingClientRect:()=>({width:100,height:30}),
   closest:()=>null,getAttribute:()=>null,...extra});
 const composer=node('',{getAttribute:()=>options.disabled?'true':null});
 const ack='Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.';
 const selectors={
  '[data-thread-find-composer] .ProseMirror[contenteditable="true"]':options.noComposer?[]:[composer],
  '[data-thread-find-target="conversation"]':options.noConversation?[]:[node()],
  '[role="dialog"],[role="alertdialog"],[role="menu"],[aria-modal="true"]':options.modal?[node()]:[],
  'input[name="conversational-onboarding-inline-role"]':options.roles?Array.from({length:options.roles},()=>node()):[],
  '*':options.ack?[node(ack)]:[node('PRIVATE user content')],
  'button':[node('PRIVATE button'),...(options.skip?[node('Skip')]:[]),...(options.start?[node('Get Started')]:[])]};
 return vm.runInNewContext(`(()=>{${source.slice(start,end)}return codingScope(${diagnostic});})()`,{
  document:{querySelectorAll:s=>selectors[s]},getComputedStyle:()=>({display:'block',visibility:'visible'})});
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
