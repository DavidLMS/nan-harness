const assert=require('node:assert/strict');
const {observe}=require('./codex-macos-home-state.cjs');
async function fixture(kind,openMenu=false){
 let reads=0,closed=0,clicks=0,proofs=0,selectedReads=0,owner=true,localGuard,ownerCalls=0;
 const sample={matched:true,rect:[0,0,20,20]},menu={dispose:async()=>{}},button={click:async()=>{clicks++},dispose:async()=>{}};
 const held={evaluate:async()=>sample,getProperty:async()=>({asElement:()=>button}),dispose:async()=>{}};
 const identity=()=>({dev:1n,ino:1n,uid:2n,mode:384n,nlink:1n,size:4n,mtimeNs:3n,ctimeNs:3n});
 const pair=()=>{reads++;if(kind==='owner-during-pair'||kind==='owner-during-final-pair'&&reads===2)owner=false;const beforeCalls=ownerCalls;for(let n=0;n<30;n++)assert.equal(localGuard(),true);assert.equal(ownerCalls,beforeCalls);return kind==='state-missing'?null:{first:{value:{},digest:'same',identity:identity()},second:{value:{},digest:kind==='state-changed'&&reads===2?'changed':'same',identity:{...identity(),ino:kind==='identity-changed'&&reads===2?9n:1n}}}};
 const page={evaluateHandle:async()=>held,locator:()=>({count:async()=>1,elementHandle:async()=>menu}),evaluate:async()=>{selectedReads++;return {status:kind==='wrong-selected'?'blocked':'observed',selectedIdCorrelated:true}}};
 const deps={makeAuthority:(_loan,_deadline,guard)=>{localGuard=guard;return {verify:()=>true,snapshotPair:pair,close:()=>closed++}},
  project:()=>kind==='wrong-root'?null:{projectId:'synthetic-id',workspace:'/synthetic'},selected:()=>{},
  makeContext:()=>({prepare:async()=>({verified:kind!=='prepare-failed'}),verifyHeld:async()=>{proofs++;return kind!=='editor-changed'||proofs<3},close:async()=>closed++})};
 const facts=await observe({page,alive:async()=>true,ownerGuard:()=>{ownerCalls++;return owner&&kind!=='owner-lost'},deadline:Date.now()+10000,
  loan:{platform:'macos',directories:[{path:'/synthetic'}]},workspace:'/synthetic',openMenu},deps);
 assert.equal(facts.sendAuthorized,false);assert.equal(facts.inputAuthorized,false);
 return {facts,reads,closed,clicks,selectedReads};
}
(async()=>{
 let r=await fixture('valid');assert.equal(r.facts.status,'observed');assert.equal(r.reads,2);assert.equal(r.clicks,0);assert.equal(r.closed,2);
 r=await fixture('valid',true);assert.equal(r.facts.selectedIdCorrelated,true);assert.equal(r.selectedReads,2);assert.equal(r.clicks,1);
 for(const kind of ['state-missing','wrong-root','prepare-failed','editor-changed','owner-lost','state-changed','identity-changed','wrong-selected','owner-during-pair','owner-during-final-pair']){
  r=await fixture(kind,true);assert.equal(r.facts.status,'blocked',kind);
  if(['state-missing','wrong-root','prepare-failed','owner-lost','owner-during-pair'].includes(kind))assert.equal(r.clicks,0,kind);
 }
 console.log('Mac passive home/state: 12 fixture cases PASS');
})().catch(e=>{console.error(e);process.exitCode=1});

const {custodyPair}=require('./codex-macos-home-state.cjs');
(async()=>{
 let owner=true,events=[];
 const prove=async()=>{events.push('proof');return owner};
 let read={snapshotPair(){events.push('read');owner=false;return {first:{},second:{}}}};
 let result=await custodyPair(read,prove);assert.equal(result.queried,true);assert.equal(result.pair,null);assert.deepEqual(events,['proof','read','proof']);
 events=[];result=await custodyPair(read,prove);assert.equal(result.queried,false);assert.deepEqual(events,['proof']);
 owner=true;events=[];read={snapshotPair(){events.push('read');throw Error('synthetic replaced root')}};
 result=await custodyPair(read,prove);assert.equal(result.pair,null);assert.deepEqual(events,['proof','read','proof']);
 owner=true;events=[];read={snapshotPair(){events.push('read');return Promise.resolve({})}};
 result=await custodyPair(read,prove);assert.equal(result.pair,null);assert.deepEqual(events,['proof','read','proof']);
 let proofCalls=0;result=await custodyPair({snapshotPair(){return {}}},async()=>{if(++proofCalls===2)throw Error('synthetic final guard');return true});assert.equal(result.queried,true);assert.equal(result.pair,null);
 result=await custodyPair({snapshotPair(){throw Error('must not execute')}},async()=>{throw Error('synthetic before guard')});assert.equal(result.queried,false);
 let now=0;result=await custodyPair({snapshotPair(){now=10;return {}}},async()=>now<10);assert.equal(result.pair,null);
 let rootHeld=true;result=await custodyPair({snapshotPair(){rootHeld=false;return {}}},async()=>rootHeld);assert.equal(result.pair,null);
 console.log('Mac synchronous custody: original owner mutation, replaced root, failed preproof and async transport rejected');
})().catch(e=>{console.error(e);process.exitCode=1});
