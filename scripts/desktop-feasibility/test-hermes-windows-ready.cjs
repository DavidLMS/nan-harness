'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {EventEmitter}=require('node:events');
async function trial(scenario) {
 let clock=0,opened=false,owner=true,refreshClicks=0,pillClicks=0,escapes=0;
 const session=new EventEmitter(); session.send=async()=>{};
 const point={x:5,y:5,left:0,top:0,width:20,height:20};
 const editorHandle={};
 const handle={evaluate:async()=>scenario==='covered'?null:point,click:async()=>{if(opened){refreshClicks++; if(scenario==='uncertain')throw new Error('PRIVATE');
 const send=(dir,obj)=>session.emit('Network.webSocketFrame'+dir,{requestId:'socket',response:{opcode:1,payloadData:JSON.stringify(obj)}});
 send('Sent',{jsonrpc:'2.0',id:1,method:'model.options',params:{profile:'default',explicit_only:true,refresh:true}});
 send('Received',{jsonrpc:'2.0',id:1,result:{providers:[{models:['qwen3.6']}]}});
 }else{pillClicks++;opened=true;if(scenario==='owner-loss')owner=false;}}};
 const control={count:async()=>scenario==='duplicate'?2:1,isEnabled:async()=>scenario!=='disabled',elementHandle:async()=>handle,evaluate:async()=>true};
 const editor={count:async()=>1,elementHandle:async()=>editorHandle,evaluate:async()=>scenario!=='composer-changed'};
 const row={...control,filter(){return this;},count:async()=>scenario==='wrong-row'?0:1};
 const menu={filter(){return this;},count:async()=>opened?1:0,getByRole(_role,options){return options?.name?control:row;}};
 const roots={count:async()=>scenario==='missing-root' || scenario==='composer-pending' && clock<200?0:1,
 locator:()=>scenario==='missing-editor'?{...editor,count:async()=>0}:editor,
 getByRole(_role,options){
   if(options.name==='Open model picker')return {...control,count:async()=>scenario==='picker-only'?1:0};
   if(options.name==='Switch model')return {...control,count:async()=>0};
   if(options.name.source==='^Model · [^\\n]+')return {...control,count:async()=>scenario==='picker-only'?0:1};
   return {...control,count:async()=>scenario==='picker-only' || scenario==='label-variant'?0:scenario==='duplicate'?2:1};
 }};
 const page={evaluate:async()=>['startup-owner-loss','startup-url-change','startup-page-count'].includes(scenario) || scenario==='loading' || scenario==='document-pending' && clock<200?'loading':'complete',url:()=>scenario==='startup-url-change' && clock>=200?'file:///synthetic/other.html':'file:///synthetic/resources/app.asar/dist/index.html',locator(selector){return selector.includes('composer-root')?roots:selector.includes('dialog')?{count:async()=>scenario==='modal'?1:0}:{filter(){return this;}};},getByRole:()=>menu,
 keyboard:{press:async key=>{assert.equal(key,'Escape');escapes++;if(scenario!=='menu-remains')opened=false;}},
 context:()=>({browser:()=>({contexts:()=>[{pages:()=>scenario==='startup-page-count' && clock>=200?[page,page]:[page]}]})})};
 const exports={};
 const context={exports,require:p=>require(p==='./hermes-catalog-readiness.cjs'?__dirname+'/hermes-catalog-readiness.cjs':p),
 process:{platform:'win32',env:{GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',FEASIBILITY_HERMES_READINESS_POLICY:'current-catalog'}},
 Date:{now:()=>clock},setTimeout:fn=>{clock+=100;fn();},JSON,Number,Error};
 vm.runInNewContext(fs.readFileSync(__dirname+'/hermes-windows-ready.cjs','utf8'),context);
 const facts=await exports.run(page,session,()=>{
   if(scenario==='owner-query-exhausts-budget')clock=3001;
   return owner && !(scenario==='startup-owner-loss' && clock>=200);
 },3000,'default');
 assert(!JSON.stringify(facts).includes('PRIVATE'));
 return {facts,pillClicks,refreshClicks,escapes};
}
(async()=>{
 for(const scenario of ['composer-pending','document-pending']) assert.equal((await trial(scenario)).facts.stage,'ready');
 const good=await trial('good');assert.equal(good.facts.stage,'ready');assert.equal(good.pillClicks,1);assert.equal(good.refreshClicks,1);assert.equal(good.escapes,1);
 for(const s of ['covered','duplicate','disabled','modal']){const r=await trial(s);assert.equal(r.pillClicks,0);assert.equal(r.refreshClicks,0);assert.notEqual(r.facts.stage,'ready');}
 const lost=await trial('owner-loss');assert.equal(lost.refreshClicks,0);assert.equal(lost.escapes,0);
 const uncertain=await trial('uncertain');assert.equal(uncertain.refreshClicks,1);assert.equal(uncertain.escapes,0);assert.notEqual(uncertain.facts.stage,'ready');
 for(const s of ['wrong-row','menu-remains','composer-changed'])assert.notEqual((await trial(s)).facts.stage,'ready');
 for(const scenario of ['missing-root','missing-editor','picker-only','label-variant','loading']) {
   const result=await trial(scenario);
   assert.equal(result.pillClicks,0);assert.equal(result.refreshClicks,0);
   assert.equal(result.facts.guardFailure,'deadline-expired');
   const sample=result.facts.composerObservation;
   assert(sample);assert.equal(sample.roots,scenario==='missing-root'?0:1);
   if(scenario==='picker-only') {
     assert.equal(sample.pickerButtons,1);assert.equal(sample.expectedModelPills,0);
     assert.equal(sample.modelPills,0);
   }
   if(scenario==='label-variant') {
     assert.equal(sample.modelPills,1);assert.equal(sample.expectedModelPills,0);
   }
   if(scenario==='loading')assert.equal(sample.readyState,'loading');
 }
 const ownerLost=await trial('startup-owner-loss');
 assert.equal(ownerLost.facts.guardFailure,'ownership-lost');assert.equal(ownerLost.pillClicks,0);
 assert.equal((await trial('owner-loss')).facts.guardFailure,'ownership-lost');
 const exhausted=await trial('owner-query-exhausts-budget');
 assert.equal(exhausted.facts.guardFailure,'deadline-expired');
 assert.equal(exhausted.facts.composerObservation,null);
 assert.equal(exhausted.pillClicks,0);assert.equal(exhausted.refreshClicks,0);
 for(const [scenario,reason] of [['startup-url-change','url-changed'],['startup-page-count','page-count']]) {
   const result=await trial(scenario);assert.equal(result.facts.guardFailure,reason);
   assert.equal(result.pillClicks,0);assert.equal(result.refreshClicks,0);
 }
 console.log('PASS Windows ordinary catalog UI behavioral guards');
})().catch(()=>process.exitCode=1);
