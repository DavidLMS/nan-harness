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
 const roots={count:async()=>scenario==='composer-pending' && clock<200?0:1,locator:()=>editor,getByRole:()=>control};
 const page={evaluate:async()=>scenario!=='document-pending' || clock>=200,url:()=> 'file:///synthetic/resources/app.asar/dist/index.html',locator(selector){return selector.includes('composer-root')?roots:selector.includes('dialog')?{count:async()=>scenario==='modal'?1:0}:{filter(){return this;}};},getByRole:()=>menu,
 keyboard:{press:async key=>{assert.equal(key,'Escape');escapes++;if(scenario!=='menu-remains')opened=false;}},
 context:()=>({browser:()=>({contexts:()=>[{pages:()=>[page]}]})})};
 const exports={};
 const context={exports,require:p=>require(p==='./hermes-catalog-readiness.cjs'?__dirname+'/hermes-catalog-readiness.cjs':p),
 process:{platform:'win32',env:{GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',FEASIBILITY_HERMES_READINESS_POLICY:'current-catalog'}},
 Date:{now:()=>clock},setTimeout:fn=>{clock+=100;fn();},JSON,Number,Error};
 vm.runInNewContext(fs.readFileSync(__dirname+'/hermes-windows-ready.cjs','utf8'),context);
 const facts=await exports.run(page,session,()=>owner,3000,'default');
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
 console.log('PASS Windows ordinary catalog UI behavioral guards');
})().catch(()=>process.exitCode=1);
