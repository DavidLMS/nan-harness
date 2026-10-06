const assert=require('node:assert/strict'),vm=require('node:vm');
const {holdRetryContinuation,retryContinuationObservation}=require('./codex-dom.cjs');
function trial(change={}) {
 const prompt='Check the controlled failure',marker='NAN_CHECK_SYNTHETIC_RECOVERY';
 const visible=e=>Object.assign(e,{isConnected:true,getBoundingClientRect:()=>({width:100,height:20})});
 const bubble=visible({innerText:prompt});
 let later=false,ready=false;
 const conversation=visible({querySelectorAll:selector=>selector==='*'?change.overflow?Array(4097).fill({}):[user]:selector==='[data-local-conversation-user-anchor]'?later?[user,other]:[user]:selector==='[data-content-search-unit-key]'?[old]:ready?headings:[]});
 const user=visible({closest:()=>conversation,querySelectorAll:()=>[bubble],querySelector:()=>bubble,
  compareDocumentPosition:e=>e===other?4:change.disconnected?1:change.before?2:4});
 const other=visible({});
 const unit=visible({getAttribute:()=>change.staleKey?'old':'new-recovery-turn-unit',closest:()=>change.foreign?{}:conversation,
  querySelector:()=>change.userInside?user:null,querySelectorAll:()=>[visible({innerText:change.wrong?'wrong':marker,closest:()=>null})]});
 const old={getAttribute:()=> 'old'};
 const headings=Array.from({length:change.duplicate?2:1},(_,i)=>({closest:()=>i?{...unit,getAttribute:()=> 'another-new-unit'}:unit}));
 const document={querySelectorAll:()=>change.duplicatePrompt?[user,user]:[user]};
 const globals={document,getComputedStyle:()=>({display:'block',visibility:'visible'})};
 const held=vm.runInNewContext(`(${holdRetryContinuation})`,globals)({prompt});
 assert.equal(ready,false);
 if(!held)return {responseVerified:false};
 ready=true;later=!!change.interveningUser;
 if(change.replacedUser)user.isConnected=false;
 if(change.replacedConversation)conversation.isConnected=false;
 if(change.changedDocument)globals.document={};
 return vm.runInNewContext(`(${retryContinuationObservation})`,globals)({held,prompt,marker});
}
assert.equal(trial().responseVerified,true,'new empty source turn follows retained failed user');
for(const key of ['staleKey','foreign','disconnected','before','userInside','wrong','duplicate','duplicatePrompt','overflow','interveningUser','replacedUser','replacedConversation','changedDocument'])assert.equal(trial({[key]:true}).responseVerified,false,key);
assert(!JSON.stringify(trial()).includes('new-recovery-turn-unit'));
console.log('PASS: retained Retry continuation, fresh source unit, exact nonce, no intervening user or foreign conversation');

{
 const {retryTargetOwned}=require('./codex-dom.cjs');
 const original=global.document;global.document={};
 const conversation={contains:()=>true};
 const held={document:global.document,conversation,user:{compareDocumentPosition:()=>4}};
 const button={isConnected:true,closest:selector=>selector.includes('conversation')?conversation:{}};
 assert.equal(retryTargetOwned(button,held),true);
 assert.equal(retryTargetOwned({...button,isConnected:false},held),false);
 assert.equal(retryTargetOwned({...button,closest:()=>({})},held),false);
 for(const order of [1,2,5])assert.equal(retryTargetOwned(button,{...held,user:{compareDocumentPosition:()=>order}}),false);
 assert.equal(retryTargetOwned(button,{...held,document:{}}),false);
 global.document=original;
}
