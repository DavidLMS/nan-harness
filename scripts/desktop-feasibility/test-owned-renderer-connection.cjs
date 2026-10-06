'use strict';
const assert=require('node:assert/strict');
const {connect}=require('./owned-renderer-connection.cjs');
(async()=>{
 for(const scenario of ['ready','transient','persistent','owner-lost','expired','late','guard-exhausts']) {
  let now=0,opens=0,guards=0;const browser={};const timeouts=[];
  const owned=()=>{guards++;if(scenario==='guard-exhausts')now=1000;return scenario!=='owner-lost'||guards===1;};
  const open=async timeout=>{opens++;timeouts.push(timeout);if(scenario==='late')now=1001;
   if(scenario==='persistent'||scenario==='owner-lost'||scenario==='transient'&&opens===1)throw Error('PRIVATE');return browser;};
  const operation=()=>connect(open,owned,1000,async ms=>{now+=ms;},()=>now);
  if(scenario==='expired')now=1000;
  if(['ready','transient'].includes(scenario))assert.equal(await operation(),browser);
  else await assert.rejects(operation(),error=>!error.message.includes('PRIVATE'));
  assert.equal(opens,{ready:1,transient:2,persistent:3,'owner-lost':1,expired:0,late:1,'guard-exhausts':0}[scenario]);
  if(scenario==='transient')assert.deepEqual(timeouts,[1000,900]);
 }
 console.log('Owned renderer connection: bounded read-only handshake retries, fresh custody and original deadline passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
