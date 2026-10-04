'use strict';
// Frozen Linux writer: atom-state debounce500ms followed by queued full-map
// temp-file rename. This is passive observed stability, never a flush call.
const same=(a,b)=>['dev','ino','uid','mode','nlink','size','mtimeNs','ctimeNs'].every(k=>a[k]===b[k]);
const equal=(a,b)=>same(a.identity,b.identity)&&a.digest===b.digest;
async function settle({read,prove,valid,baseline=null,deadline,now=Date.now,
  wait=require('node:timers/promises').setTimeout}) {
  let last=null,quietSince=null,selectionSeen=false;
  while(now()<deadline) {
    if(await prove()!==true||now()>=deadline)return {reason:'guard'};
    const pair=read();
    if(await prove()!==true||now()>=deadline)return {reason:'guard'};
    if(!pair)return {reason:'state-changed'};
    if(!valid(pair.first.value)||!valid(pair.second.value)) {
      // Only the exact pre-action sealed bytes/inode may still be pending.
      // Once exact selected state is seen, even that old state is a replay.
      if(selectionSeen||!baseline||!equal(baseline.first,pair.first)||!equal(baseline.second,pair.second))return {reason:'selection-state'};
      last=null;quietSince=null;
    } else {
      selectionSeen=true;
      if(!last||!equal(last.first,pair.first)||!equal(last.second,pair.second))quietSince=now();
      last=pair;
      if(now()-quietSince>=500)return {pair};
    }
    const remaining=deadline-now();
    if(remaining<=0)break;
    await wait(Math.min(100,remaining));
  }
  return {reason:'deadline'};
}
exports.settle=settle;
