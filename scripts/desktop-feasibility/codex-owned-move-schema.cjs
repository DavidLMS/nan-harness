'use strict';
const {reasons}=require('./codex-owned-move-wire.cjs');
const bits=['planMeasured','fullWorkareaBoundsBlocker','candidateFound','moveAttempted','writeAcknowledged','sameIdentityTranslated','nativePointClear','nativeHitWindowMatched','mappingStable','nativeFocused'];
function exact(v,keys){return !!v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join('\0')===keys.slice().sort().join('\0')}
function nativeFacts(v){
 if(!exact(v,['reason','candidateCount',...bits,'inputAuthorized'])||!reasons.has(v.reason)||v.inputAuthorized!==false||!bits.every(k=>typeof v[k]==='boolean')||!Number.isSafeInteger(v.candidateCount)||v.candidateCount<0||v.candidateCount>9)return false;
 if(!v.planMeasured&&(v.candidateCount||bits.slice(1).some(k=>v[k]))||v.fullWorkareaBoundsBlocker&&(v.candidateFound||v.candidateCount||v.moveAttempted)
  ||v.candidateFound&&(!v.planMeasured||v.candidateCount<1)||v.moveAttempted&&!v.candidateFound||v.writeAcknowledged&&!v.moveAttempted
  ||v.sameIdentityTranslated&&!v.writeAcknowledged||v.mappingStable&&!v.sameIdentityTranslated
  ||v.nativePointClear&&(!v.mappingStable||!v.nativeFocused)||v.nativeHitWindowMatched&&!v.nativePointClear)return false;
 if(v.reason==='moved-point-observed'&&(!bits.filter(k=>k!=='fullWorkareaBoundsBlocker').every(k=>v[k])||v.fullWorkareaBoundsBlocker)
  ||v.reason==='full-workarea-bounds-blocker'&&!v.fullWorkareaBoundsBlocker
  ||v.reason==='no-clear-candidate'&&(!v.planMeasured||v.candidateFound||v.fullWorkareaBoundsBlocker)
  ||v.reason==='write-uncertain'&&(!v.moveAttempted||v.writeAcknowledged))return false;return true;
}
const outerReasons=new Set(['source-policy-rejected','source-point-unavailable','deadline-or-owner','renderer-changed','native-move-rejected','move-unavailable-or-uncertain','post-mapping-unproved','moved-source-point-observed']);
function observation(v){const keys=['reason','sourcePointRetained','rendererReproved','postMappingObserved','inputAuthorized'];
 if(!exact(v,[...keys,...(v&&Object.hasOwn(v,'native')?['native']:[])])||!outerReasons.has(v.reason)||v.inputAuthorized!==false||!keys.slice(1,4).every(k=>typeof v[k]==='boolean')||Object.hasOwn(v,'native')&&!nativeFacts(v.native))return false;
 if(Object.hasOwn(v,'native')&&!v.sourcePointRetained||v.postMappingObserved!==(v.reason==='moved-source-point-observed')||v.rendererReproved&&!v.sourcePointRetained||v.postMappingObserved&&(!v.rendererReproved||v.native?.reason!=='moved-point-observed'))return false;
 if(v.reason==='moved-source-point-observed'&&!(v.sourcePointRetained&&v.rendererReproved&&v.postMappingObserved&&v.native?.reason==='moved-point-observed'))return false;
 if(v.reason==='source-policy-rejected'&&(v.sourcePointRetained||v.rendererReproved||v.postMappingObserved||Object.hasOwn(v,'native')))return false;
 if(v.reason==='post-mapping-unproved'&&(!v.sourcePointRetained||!v.rendererReproved||v.native?.reason!=='moved-point-observed'))return false;
 if(v.reason==='native-move-rejected'&&(!v.native||v.native.reason==='moved-point-observed'||!v.rendererReproved))return false;return true;
}
module.exports={nativeFacts,observation};
