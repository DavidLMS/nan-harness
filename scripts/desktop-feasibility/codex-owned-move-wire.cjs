'use strict';
const reasons=new Set(['plan-unavailable','full-workarea-bounds-blocker','no-clear-candidate','position-not-settable','pre-move-identity','write-uncertain','transition-unproved','post-point-occluded','post-hit-unproved','post-mapping-changed','post-focus-unproved','source-point-unavailable','deadline','moved-point-observed']);
function parseMove(output,oldTokens){
 if(typeof output!=='string'||output.length>2048||!output.endsWith('\n'))throw Error('owned move wire rejected');
 const t=output.slice(0,-1).split(' ');if(t[0]!=='owned-move'||!reasons.has(t[1])||![13,21].includes(t.length)||!/^\d+$/.test(t[2]))throw Error('owned move wire rejected');
 const count=Number(t[2]);if(!Number.isSafeInteger(count)||count>9||!t.slice(3,13).every(v=>v==='0'||v==='1'))throw Error('owned move wire rejected');
 const keys=['planMeasured','fullWorkareaBoundsBlocker','candidateFound','moveAttempted','writeAcknowledged','sameIdentityTranslated','nativePointClear','nativeHitWindowMatched','mappingStable','nativeFocused'];
 const facts={reason:t[1],candidateCount:count,...Object.fromEntries(keys.map((k,i)=>[k,t[3+i]==='1'])),inputAuthorized:false};
 if(!facts.planMeasured&&(count||keys.slice(1).some(k=>facts[k]))||facts.fullWorkareaBoundsBlocker&&(facts.candidateFound||count||facts.moveAttempted)
  ||facts.candidateFound&&(!facts.planMeasured||count<1)||facts.moveAttempted&&!facts.candidateFound||facts.writeAcknowledged&&!facts.moveAttempted
  ||facts.sameIdentityTranslated&&!facts.writeAcknowledged||facts.mappingStable&&!facts.sameIdentityTranslated
  ||facts.nativePointClear&&(!facts.mappingStable||!facts.nativeFocused)||facts.nativeHitWindowMatched&&!facts.nativePointClear)throw Error('owned move wire rejected');
 const moved=t[1]==='moved-point-observed';
 if(moved!==(t.length===21)||moved&&(!keys.filter(k=>k!=='fullWorkareaBoundsBlocker').every(k=>facts[k])||facts.fullWorkareaBoundsBlocker))throw Error('owned move wire rejected');
 if(t[1]==='full-workarea-bounds-blocker'&&!facts.fullWorkareaBoundsBlocker||t[1]==='no-clear-candidate'&&(!facts.planMeasured||facts.candidateFound||facts.fullWorkareaBoundsBlocker)
  ||t[1]==='write-uncertain'&&(!facts.moveAttempted||facts.writeAcknowledged))throw Error('owned move wire rejected');
 let tokens=null;
 if(moved){const next=t.slice(13),old=oldTokens.split(' ');
  if(old.length!==8||!next.every(v=>/^-?\d+(?:\.\d+)?(?:e[+-]?\d+)?$/.test(v))||!next.map(Number).every(Number.isFinite)
   ||![0,1,4,5,6,7].every(i=>next[i]===old[i])||(next[2]===old[2]&&next[3]===old[3]))throw Error('owned move transition rejected');
  tokens=next.join(' ');
 }
 return {facts,tokens};
}
module.exports={parseMove,reasons};
