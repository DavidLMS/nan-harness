'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
async function trial(options={}){
 let selected=false,reopened=false,reopens=0,itemClicks=0,originalClicks=0,contextClosed=0,checks=0;
 const rect=[10,10,30,20],menu={dispose:async()=>{}},newMenu={dispose:async()=>{}};
 const button={click:async()=>{originalClicks++;},dispose:async()=>{}},nextButton={click:async()=>{reopens++;reopened=true;},dispose:async()=>{}};
 const next={evaluate:async()=>({matched:!options.invalidSelectedStructure,rect}),getProperty:async()=>({asElement:()=>nextButton}),dispose:async()=>{}};
 const held={evaluate:async fn=>fn.name==='prepare'?true:({matched:!selected,rect}),
  evaluateHandle:async()=>next,getProperty:async()=>({asElement:()=>button}),dispose:async()=>{}};
 // The candidate's null test and source sample have separate wire results.
 next.evaluate=async fn=>fn.name==='sample'?{matched:!options.invalidSelectedStructure,rect}:true;
 const item={click:async()=>{itemClicks++;selected=true;},dispose:async()=>{}};
 const choice={evaluate:async()=>({matched:true,rect}),getProperty:async()=>({asElement:()=>item}),dispose:async()=>{}};
 const context={prepare:async()=>({verified:true}),verifyHeld:async()=>true,
  verifyRetainedDocument:async()=>!options.changedOriginalDocument,retainedDocumentFailure:()=> 'editor-changed',
  observe:async()=>({verified:true,inputAuthorized:false}),close:async()=>{contextClosed++;}};
 const page={evaluateHandle:async fn=>fn.name==='capture'?held:choice,
  evaluate:async()=>({reason:'selected-id',selectedItemCount:reopened?1:0,matchingItemCount:1}),
  locator:()=>({count:async()=>1,elementHandle:async()=>reopened?newMenu:menu})};
 // Project-choice capture is dispatched only inside selection.
 let captures=0;page.evaluateHandle=async()=>++captures===1?held:choice;
 const scope={exports:{},require:name=>{
  if(name.includes('context-session'))return {create:()=>context};
  if(name.includes('project-choice'))return {capture:()=>{},sample:()=>{}};
  if(name.includes('selected-project'))return {sample:()=>{}};
  if(name.includes('control-transition'))return {prepare:function prepare(){},capture:()=>{}};
  if(name.includes('profile-state'))return {observe:async(_page,_owner,_deadline,_loan,_workspace,_menu,_alive,_observe,transition)=>{
   const result=await transition({projectId:'synthetic-id',projectName:'synthetic'},()=>true);
   if(!result)return {status:'blocked',sendAuthorized:false};
   const actual=await page.evaluate();checks++;assert.equal(actual.selectedItemCount,1);
   return {status:'observed',statePairStable:true,selectedIdCorrelated:true,sendAuthorized:false};
  }};
  throw Error('unexpected dependency');
 },setTimeout,clearTimeout,Date};
 vm.runInNewContext(fs.readFileSync(__dirname+'/codex-workspace-menu.cjs','utf8'),scope);
 const facts=await scope.exports.run(page,async()=>true,()=>true,Date.now()+1000,{},'/synthetic',{frozenLinuxTrial:true});
 return {facts,reopens,itemClicks,originalClicks,contextClosed,checks};
}
(async()=>{
 const ok=await trial();assert.equal(ok.itemClicks,1);assert.equal(ok.reopens,1);assert.equal(ok.originalClicks,1);assert.equal(ok.checks,1);assert.equal(ok.contextClosed,1);assert.equal(ok.facts.sendAuthorized,false);assert.equal(ok.facts.selectionClickCompleted,true);
 for(const options of [{changedOriginalDocument:true},{invalidSelectedStructure:true}]){
  const denied=await trial(options);assert.equal(denied.reopens,0);assert.equal(denied.checks,0);assert.equal(denied.facts.sendAuthorized,false);assert.notEqual(denied.facts.selectionClickCompleted,true);
 }
 console.log('PASS one declared trigger transition + source menu check; changed document/structure never reopen');
})().catch(error=>{console.error(error);process.exitCode=1;});
