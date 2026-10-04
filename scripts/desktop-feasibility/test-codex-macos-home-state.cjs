const assert=require('node:assert/strict');
const {observe}=require('./codex-macos-home-state.cjs');
async function fixture(kind,openMenu=false){
 let reads=0,closed=0,clicks=0,proofs=0,selectedReads=0;
 const sample={matched:true,rect:[0,0,20,20]},menu={dispose:async()=>{}},button={click:async()=>{clicks++},dispose:async()=>{}};
 const held={evaluate:async()=>sample,getProperty:async()=>({asElement:()=>button}),dispose:async()=>{}};
 const identity=()=>({dev:1n,ino:1n,uid:2n,mode:384n,nlink:1n,size:4n,mtimeNs:3n,ctimeNs:3n});
 const pair=()=>{reads++;return kind==='state-missing'?null:{first:{value:{},digest:'same',identity:identity()},second:{value:{},digest:kind==='state-changed'&&reads===2?'changed':'same',identity:{...identity(),ino:kind==='identity-changed'&&reads===2?9n:1n}}}};
 const page={evaluateHandle:async()=>held,locator:()=>({count:async()=>1,elementHandle:async()=>menu}),evaluate:async()=>{selectedReads++;return {status:kind==='wrong-selected'?'blocked':'observed',selectedIdCorrelated:true}}};
 const deps={makeAuthority:()=>({verify:()=>true,snapshotPair:pair,close:()=>closed++}),
  project:()=>kind==='wrong-root'?null:{projectId:'synthetic-id',workspace:'/synthetic'},selected:()=>{},
  makeContext:()=>({prepare:async()=>({verified:kind!=='prepare-failed'}),verifyHeld:async()=>{proofs++;return kind!=='editor-changed'||proofs<3},close:async()=>closed++})};
 const facts=await observe({page,alive:async()=>true,ownerGuard:()=>kind!=='owner-lost',deadline:Date.now()+10000,
  loan:{platform:'macos',directories:[{path:'/synthetic'}]},workspace:'/synthetic',openMenu},deps);
 assert.equal(facts.sendAuthorized,false);assert.equal(facts.inputAuthorized,false);
 return {facts,reads,closed,clicks,selectedReads};
}
(async()=>{
 let r=await fixture('valid');assert.equal(r.facts.status,'observed');assert.equal(r.reads,2);assert.equal(r.clicks,0);assert.equal(r.closed,2);
 r=await fixture('valid',true);assert.equal(r.facts.selectedIdCorrelated,true);assert.equal(r.selectedReads,2);assert.equal(r.clicks,1);
 for(const kind of ['state-missing','wrong-root','prepare-failed','editor-changed','owner-lost','state-changed','identity-changed','wrong-selected']){
  r=await fixture(kind,true);assert.equal(r.facts.status,'blocked',kind);
  if(['state-missing','wrong-root','prepare-failed','owner-lost'].includes(kind))assert.equal(r.clicks,0,kind);
 }
 console.log('Mac passive home/state: 10 fixture cases PASS');
})().catch(e=>{console.error(e);process.exitCode=1});
