const assert=require('node:assert/strict'),{create}=require('./codex-context-session.cjs');
async function fixture(change){let detached=0,released=0,clock=0,loader='L',connected=true,owner=true,pw=true,called=0;const methods=[];
 const session={async send(m,p){methods.push(m);if(m==='Target.getTargetInfo')return {targetInfo:{targetId:'T'}};
 if(m==='Page.getFrameTree')return {frameTree:{frame:{id:'F',loaderId:loader,url:'app://private'}}};
 if(m==='Runtime.evaluate')return {result:{objectId:'HELD'}};
 if(m==='Runtime.getProperties')return {result:[{name:'editor',value:{objectId:'EDITOR'}}]};
 if(m==='Runtime.callFunctionOn'){assert.equal(p.objectId,'HELD');return {result:{value:connected}};}
 if(m==='Runtime.releaseObjectGroup'){released++;return {}};throw Error(m);},async detach(){detached++}};
 const page={url:()=> 'app://private',context:()=>({newCDPSession:async()=>session})};
 const loan=create({page,alive:async()=>owner,deadline:1000,pwProof:async()=>pw,now:()=>clock,
 makeWitness:({editor,guard})=>({async observe(o){called++;assert.equal(editor.objectId,'EDITOR');assert.deepEqual(o,{requireSelection:false});await guard();return {verified:true,reason:'verified',inputAuthorized:false}}})});
 const prepared=await loan.prepare({});assert.equal(prepared.verified,true);change?.({reload:()=>loader='NEW',replace:()=>connected=false,expire:()=>clock=1000,foreign:()=>owner=false,pwChange:()=>pw=false});
 const result=await loan.observe({},'privateID','privateRoot');await loan.close();await loan.close();assert.equal(detached,1);assert.equal(released,1);assert(!JSON.stringify(result).includes('private'));return {result,called};}
(async()=>{assert.equal((await fixture()).result.verified,true);
 for(const [change,reason] of [[x=>x.reload(),'identity-changed'],[x=>x.replace(),'editor-changed'],[x=>x.expire(),'deadline-or-owner'],[x=>x.foreign(),'deadline-or-owner'],[x=>x.pwChange(),'identity-changed']]){const x=await fixture(change);assert.equal(x.result.reason,reason);assert.equal(x.result.inputAuthorized,false);assert.equal(x.called,0)}
 console.log('6 identity, deadline, retained-node and cleanup cases passed');})();
