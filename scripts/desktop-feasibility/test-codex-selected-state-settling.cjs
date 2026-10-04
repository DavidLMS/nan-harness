'use strict';
const assert=require('node:assert/strict');
const {settle}=require('./codex-selected-state-settling.cjs');
const state=(selected=true,record='original')=>({selected,record});
const pair=(ino=1n,digest='sealed',value=state())=>({first:{identity:{dev:1n,ino,uid:2n,mode:384n,nlink:1n,size:20n,mtimeNs:3n,ctimeNs:4n},digest,value},second:{identity:{dev:1n,ino,uid:2n,mode:384n,nlink:1n,size:20n,mtimeNs:3n,ctimeNs:4n},digest,value}});
async function run(kind){
 let now=0,reads=0,owner=true,proofs=0;
 const baseline=pair(1n,'baseline',state(false));
 const read=()=>{
  reads++;
  if(kind==='owner-during-read')owner=false;
  if(kind==='null')return null;
  if(kind==='pending'&&now<200||kind==='replay'&&now===200)return baseline;
  if(kind==='wrong-record'&&now>=200)return pair(2n,'other',state(true,'foreign'));
  if(kind==='wrong-selected'&&now>=200)return pair(2n,'other',state(false));
  if(kind==='identity-rewrite'&&now>=200)return pair(2n);
  if(kind==='digest-rewrite'&&now>=200)return pair(1n,'updated');
  if(kind==='continuous')return pair(BigInt(reads));
  return pair();
 };
 const result=await settle({baseline,read,valid:v=>v.selected&&v.record==='original',deadline:kind==='short'?400:1200,
  prove:async()=>{proofs++;return kind!=='pre-guard'&&owner},now:()=>now,wait:async ms=>{assert(ms<=100);now+=ms}});
 return {result,reads,proofs,now};
}
(async()=>{
 for(const kind of ['stable','identity-rewrite','digest-rewrite','pending']){
  const r=await run(kind);assert(r.result.pair,kind);assert.equal(r.now,kind==='stable'?500:700);assert.equal(r.proofs,2*r.reads);
 }
 for(const kind of ['wrong-record','wrong-selected','replay'])assert.equal((await run(kind)).result.reason,'selection-state',kind);
 assert.equal((await run('null')).result.reason,'state-changed');
 for(const kind of ['continuous','short']){const r=await run(kind);assert.equal(r.result.reason,'deadline');assert.equal(r.now,kind==='short'?400:1200);}
 let r=await run('pre-guard');assert.equal(r.result.reason,'guard');assert.equal(r.reads,0);
 r=await run('owner-during-read');assert.equal(r.result.reason,'guard');assert.equal(r.reads,1);
 console.log('Linux passive settling: 12 meaningful cases PASS');
})().catch(e=>{console.error(e);process.exitCode=1});
