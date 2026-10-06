const assert=require('node:assert/strict'),vm=require('node:vm');
const {homeComposerScope,runTurn,sampleEditor}=require('./codex-dom.cjs');
function shape(change={}) {
 const node=attrs=>({isConnected:true,tagName:'DIV',getAttribute:k=>attrs[k]??null,
  getBoundingClientRect:()=>({width:100,height:20}),closest:()=>null});
 const home=node({'data-codex-composer-root':'','data-composer-placement':'home'});
 const editor=node({contenteditable:'true'});editor.classList={contains:k=>k==='ProseMirror'};
 home.contains=e=>e===editor;
 let nodes=[home,editor];
 if(change.duplicate)nodes.push(editor);
 if(change.foreign)nodes.push(node({contenteditable:'true'}));
 if(change.modal)nodes.push(node({role:'dialog'}));
 if(change.conversation)nodes.push(node({'data-thread-find-target':'conversation'}));
 if(change.disabled)editor.disabled=true;
 if(change.detached)editor.isConnected=false;
 if(change.wrongHome)home.contains=()=>false;
 if(change.overflow)nodes=Array.from({length:4097},()=>home);
 return vm.runInNewContext(`(${homeComposerScope})()`,{document:{querySelectorAll:()=>nodes},getComputedStyle:()=>({display:'block',visibility:'visible'})});
}
assert.equal(shape(),true);
for(const key of ['duplicate','foreign','modal','conversation','disabled','detached','wrongHome','overflow'])assert.equal(shape({[key]:true}),false,key);
function buttonSample(change={}) {
 const parent={isConnected:true,parentElement:null};
 const button={tagName:'BUTTON',isConnected:true,parentElement:parent,disabled:!!change.disabled,readOnly:false,
  getAttribute:()=>null,closest:()=>null,getBoundingClientRect:()=>({left:10,top:10,width:100,height:40}),
  clientLeft:0,clientTop:0,clientWidth:100,clientHeight:40,contains:()=>false};
 const document={querySelectorAll:()=>[],elementFromPoint:()=>change.covered?{}:button};
 parent.ownerDocument=button.ownerDocument=document;
 return vm.runInNewContext(`(${sampleEditor})(button,'button')`,{button,document,innerWidth:800,innerHeight:600,
  getComputedStyle:e=>({display:'block',visibility:'visible',pointerEvents:e===parent||change.pointerDisabled?'none':'auto'})});
}
assert.equal(buttonSample().points.length,9,'child button opts into parent disabled hit area');
assert.equal(buttonSample({pointerDisabled:true}).blocked,'pointer-disabled');
assert.equal(buttonSample({covered:true}).points.length,0);
assert.equal(buttonSample({disabled:true}).blocked,'disabled');
async function trial(change={}) {
 let text=change.draft?'PRIVATE existing draft':'',sent=false,fills=0,clicks=0,replaced=false,owned=true,homeReads=0,sendReads=0;
 const input={tagName:change.wrongTag?'BUTTON':'DIV',disabled:!!change.disabled,readOnly:!!change.readonly,classList:{contains:k=>k==='ProseMirror'},
  getAttribute:k=>k==='contenteditable'?'true':null,closest:()=>change.inert?{}:null,parentElement:null,
  get isConnected(){return !replaced;},get textContent(){return text;},
  clientLeft:0,clientTop:0,clientWidth:100,clientHeight:40,
  getBoundingClientRect:()=>({left:10,top:10,width:100,height:40}),contains:()=>false};
 const document={querySelectorAll:()=>change.modal?[input]:[],elementFromPoint:()=>change.covered?{}:input};
 input.ownerDocument=change.foreignDoc?{}:document;
 if(change.ancestorPointerDisabled)input.parentElement={isConnected:true,ownerDocument:document,parentElement:null};
 const globals={document,innerWidth:800,innerHeight:600,getComputedStyle:e=>({display:change.hidden?'none':'block',visibility:'visible',pointerEvents:change.pointerDisabled||change.ancestorPointerDisabled&&e!==input?'none':'auto'}),input};
 const held={evaluate:async(fn,arg)=>fn===sampleEditor?vm.runInNewContext(`(${fn})(input)`,globals):fn(input,arg),dispose:async()=>{}};
 const editor={count:async()=>change.duplicate?2:1,elementHandle:async()=>held,
  evaluate:async(fn,arg)=>arg===held?!change.replaced:fn({textContent:text},arg),
  fill:async prompt=>{fills++;text=change.readback?'wrong':prompt;if(change.ownerAfterFill)owned=false;}};
 const handle={evaluate:async()=>({rect:[0,0,10,10],points:[{x:5,y:5}]}),dispose:async()=>{},
  click:async()=>{clicks++;sent=true;replaced=true;if(change.ownerAfterSend)owned=false;}};
 const send={count:async()=>{sendReads++;return change.duplicateSend?2:change.sendMissing?0:change.busySend&&sendReads<3?0:1;},isEnabled:async()=>!change.sendDisabled,elementHandle:async()=>handle,evaluate:async()=>true};
 const publicDOM={home:{status:'observed',homeComposerCount:0,pendingTextareaCount:0,proseMirrorEditableCount:0,workspaceControlCount:0},editable:{status:'observed',editableCount:0,codexThreadCount:0,classicChatGPTCount:0}};
 const page={evaluate:async(fn,arg)=>fn.name==='codingScope'?(arg?{publicDOM}:false):fn.name==='homeComposerScope'?(homeReads++,change.ownerDuringReadiness&&(owned=false),!change.notReady&&homeReads>(change.delayed?2:0)):
  {userCount:sent?1:0,assistantCount:sent?1:0,responseVerified:sent&&!change.wrongMarker},
  locator:selector=>selector.includes('ProseMirror')?editor:{getByRole:()=>send}};
 const facts=await runTurn(page,async()=>owned,{timeoutMs:1000,action:'submit',purpose:'response',prompt:'Check this connection',expectedMarker:'nonce'},Date.now()+(change.notReady||change.sendMissing||change.sendDisabled?35:500));
 return {facts,fills,clicks,homeReads};
}
(async()=>{
 const busy=await trial({busySend:true});assert.equal(busy.facts.responseVerified,true);assert.equal(busy.clicks,1);
 for(const key of ['sendMissing','sendDisabled','duplicateSend']){const blocked=await trial({[key]:true});assert.equal(blocked.fills,1);assert.equal(blocked.clicks,0);assert.equal(blocked.facts.errorCategory,'action-uncertain');}
 const delayed=await trial({delayed:true});assert.equal(delayed.facts.responseVerified,true);assert(delayed.homeReads>=3);assert.equal(delayed.fills,1);
 const waiting=await trial({notReady:true});assert.equal(waiting.facts.composerAdmissionFailure,'scope-not-ready');assert.equal(waiting.facts.composerReadinessObservation.homeComposerCount,0);assert.equal(waiting.fills,0);assert.equal(waiting.clicks,0);
 const ownerWait=await trial({ownerDuringReadiness:true});assert.equal(ownerWait.facts.errorCategory,'ownership-lost');assert.equal(ownerWait.fills,0);assert.equal(ownerWait.facts.composerAdmissionFailure,undefined);
 for(const [key,cause] of [['wrongTag','unsupported-control'],['hidden','hidden'],['disabled','disabled'],['pointerDisabled','pointer-disabled'],['duplicate','nonunique-editor']])assert.equal((await trial({[key]:true})).facts.composerAdmissionFailure,cause,key);
 const inherited=await trial({ancestorPointerDisabled:true});assert.equal(inherited.facts.responseVerified,true);
 const good=await trial();assert.equal(good.facts.responseVerified,true);assert.equal(good.fills,1);assert.equal(good.clicks,1);
 for(const key of ['draft','duplicate','replaced','covered','modal','disabled','readonly','foreignDoc','inert','hidden','pointerDisabled','wrongTag']){const bad=await trial({[key]:true});assert.equal(bad.fills,0,key);assert.equal(bad.clicks,0,key);}
 for(const key of ['readback','ownerAfterFill']){const bad=await trial({[key]:true});assert.equal(bad.fills,1,key);assert.equal(bad.clicks,0,key);}
 const lost=await trial({ownerAfterSend:true});assert.equal(lost.facts.responseVerified,false);assert.equal(lost.clicks,1);
 const mismatch=await trial({wrongMarker:true});assert.equal(mismatch.facts.responseVerified,false);assert.equal(mismatch.clicks,1);
 console.log('PASS: bounded home shape, retained empty editor, fill readback, one Send, declared DOM transition, lost ownership and nonce rejection');
})().catch(e=>{console.error(e);process.exitCode=1});
