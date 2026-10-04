const assert=require('node:assert/strict'),s=require('./codex-owned-move-schema.cjs'),{parseMove}=require('./codex-owned-move-wire.cjs');
const native=parseMove('owned-move moved-point-observed 1 1 0 1 1 1 1 1 1 1 1 42 100 30 40 600 400 500 0\n','42 100 10 20 600 400 500 0').facts;
const valid={reason:'moved-source-point-observed',sourcePointRetained:true,rendererReproved:true,postMappingObserved:true,inputAuthorized:false,native};assert(s.observation(valid));
for(const v of [{...valid,rawPath:'/private'},{...valid,inputAuthorized:true},{...valid,native:{...native,rawPID:100}},{...valid,native:{...native,reason:'unknown'}},{...valid,native:{...native,candidateCount:10}},{...valid,native:{...native,nativeHitWindowMatched:null}},{...valid,rendererReproved:false},{...valid,native:null}])assert.equal(s.observation(v),false);
console.log('9 strict closed reducer schema cases passed');
