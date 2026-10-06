// Only this factory can issue a retained guard capability. Its complete source
// samples always have fresh native ownership proofs before and after the reads.
const guards=new WeakSet(),owners=new WeakMap();
function createHeldMainGuard(held, browser, owner, deadline, route,
  identity,pause,requireMainScope,allowInitialAppearance,requireDocumentFocus,
  {sameCorrelationIdentity,settleFolderAuxiliary,now,requireVisibleDocument=false,progress=()=>{}}) {
  let actionsStarted=false,appearanceRetried=false,folderSettleTicket=false,folderSettleGranted=false;
  let activationSettleTicket=false,activationSettleGranted=false,mainScopeProved=false;
  let auxiliary=null, auxiliaryIdentity=null, failure='unmeasured', failureDetails=null;
  const started=now();
  let nativeProofCount=0,nativeProofMs=0,identityCount=0,identityMs=0;
  const checkpoint=phase=>{
    const bounded=value=>Math.min(600000,Math.max(0,Math.floor(value)));
    try {progress({phase,elapsedMs:bounded(now()-started),
      nativeProofCount:Math.min(4096,nativeProofCount),nativeProofMs:bounded(nativeProofMs),
      identityCount:Math.min(4096,identityCount),identityMs:bounded(identityMs)});}catch {}
  };
  const reject=reason=>{failure=reason;checkpoint('rejected');return false;};
  const identify=async(page,phase)=>{
    checkpoint(phase);const begin=now();identityCount++;
    try {return await identity(page,deadline);}
    finally {identityMs+=now()-begin;checkpoint(phase);}
  };
  const rejectPageSet=(reason,initial,current)=>{
    const count=pages=>pages.length<=32?pages.length:null;
    failureDetails={reason,initialCount:count(initial),currentCount:count(current),
      heldPresent:current.includes(held.page)};
    return reject('page-set');
  };
  const pages=()=>browser.contexts().flatMap(context=>context.pages());
  const timely=()=>now()<deadline||reject('deadline');
  const valid=()=>{
    if(now()>=deadline)return reject('deadline');
    checkpoint('native-ownership');const begin=now();nativeProofCount++;
    let owned;
    try {owned=owner();}finally {nativeProofMs+=now()-begin;checkpoint('native-ownership');}
    if(owned!==true)return reject('native-ownership');
    return now()<deadline||reject('deadline');
  };
  const measure=async function measure() {
    failure='unmeasured';failureDetails=null;mainScopeProved=false;
    try {
      if(!held)return reject('main-identity');
      if(!timely())return false;
      const initial=pages();
      if(!initial.includes(held.page))return rejectPageSet('held-main-missing',initial,initial);
      if(initial.length<1||initial.length>2)return rejectPageSet('initial-count',initial,initial);
      const extra=initial.find(page=>page!==held.page);
      if(auxiliary&&extra!==auxiliary)return reject('auxiliary-identity');
      if(extra&&route(extra.url())!=='avatarOverlay')return reject('auxiliary-route');
      const samples=extra&&!auxiliary?2:1;
      let candidateAux=null;
      for(let sample=0;sample<samples;sample++) {
        if(!valid())return false;
        const before=pages();
        if(before.length!==initial.length||!before.every(page=>initial.includes(page)))return rejectPageSet('before-sample-changed',initial,before);
        const main=await identify(held.page,'main-identity');
        if(!timely())return false;
        if(!sameCorrelationIdentity(held,main))return reject('main-identity');
        if(requireDocumentFocus&&!main.scope.focused)return reject('main-focus');
        if(requireVisibleDocument&&main.scope.visibleDocument!==true)return reject('main-scope');
        if((requireMainScope||(allowInitialAppearance&&!actionsStarted)||activationSettleTicket)&&!main.scope.mainScope)return reject('main-scope');
        if(extra) {
          const aux=await identify(extra,'auxiliary-identity');
          const expected=auxiliaryIdentity??candidateAux;
          if(!timely())return false;
          if(expected&&!sameCorrelationIdentity(expected,aux))return reject('auxiliary-identity');
          const counts=aux.scope.counts;
          if(aux.scope.focused)return reject('auxiliary-focus');
          if(['roleLegend','roleRadios','engineering','dialog','quickChatComposer','editable']
              .some(key=>counts[key]!==0))return reject('auxiliary-controls');
          candidateAux=aux;
        }
        const after=pages();
        if(!valid())return false;
        if(after.length!==initial.length||!after.every(page=>initial.includes(page)))return rejectPageSet('after-sample-changed',initial,after);
        mainScopeProved=main.scope.mainScope===true;
        if(sample+1<samples)await pause(Math.min(100,Math.max(0,deadline-now())));
      }
      if(extra&&!auxiliary){auxiliary=extra;auxiliaryIdentity=candidateAux;}
      checkpoint('complete');return true;
    } catch {return reject(now()>=deadline?'deadline':'query-failed');}
  };
  const prove=async()=>{
    if(await measure())return true;
    const canSettleBlank=folderSettleTicket||requireVisibleDocument;
    const changed=failureDetails;
    const appeared=(requireVisibleDocument||allowInitialAppearance&&!actionsStarted||folderSettleTicket||activationSettleTicket)
      &&!appearanceRetried&&!auxiliary&&failure==='page-set'
      &&changed&&changed.initialCount===1&&changed.currentCount===2&&changed.heldPresent
      &&['before-sample-changed','after-sample-changed'].includes(changed.reason);
    if(appeared) {
      // One appearance can have two observation phases: target creation followed
      // by its first navigation. They share the original deadline and target.
      appearanceRetried=true;folderSettleTicket=false;
      if(await measure())return true;
    }
    // An appearing blank page grants no input. Keep the main target and wait
    // only for that same page to commit the inspected inert avatar route.
    if(canSettleBlank&&(!appearanceRetried||appeared)&&!auxiliary&&failure==='auxiliary-route') {
      const current=pages(),extra=current.find(page=>page!==held.page);
      if(current.length!==2||!current.includes(held.page)||!extra
        ||!['','about:blank'].includes(extra.url()))return false;
      appearanceRetried=true;folderSettleTicket=false;
      if(!await settleFolderAuxiliary(held,extra,pages,valid,deadline,route,identity,pause,!requireVisibleDocument))return false;
      // Establish two NEW inert auxiliary proofs only after the committed route.
      return measure();
    }
    return false;
  };
  // Issue only after native prepare in the initial focus caller. The latest
  // complete native-bracketed main sample must already prove the role source.
  prove.allowPassiveActivationSettle=()=>{
    if(!timely()||actionsStarted||activationSettleGranted||auxiliary||appearanceRetried||!mainScopeProved)return false;
    activationSettleGranted=true;activationSettleTicket=true;return true;
  };
  prove.finishPassiveActivationSettle=()=>{activationSettleTicket=false;};
  // Caller grants this only after the one folder trust action completed. It
  // authorizes one passive 1-to-2 measurement restart, never another input.
  prove.allowPassiveFolderSettle=()=>{
    if(!actionsStarted||folderSettleGranted||auxiliary||appearanceRetried)return false;
    folderSettleGranted=true;folderSettleTicket=true;return true;
  };
  prove.finishPassiveFolderSettle=()=>{folderSettleTicket=false;};
  prove.requireDocumentFocus=()=>{requireDocumentFocus=true;};
  prove.sealInitialActions=()=>{actionsStarted=true;activationSettleTicket=false;};
  prove.failure=()=>failure;
  prove.failureDetails=()=>failure==='page-set'?failureDetails:null;
  const privateIdentity=value=>value&&Object.fromEntries(['url','target','frame','loader','frameUrl','fragment'].map(key=>[key,value[key]]));
  prove.binding=()=>({schemaVersion:1,main:privateIdentity(held),auxiliary:privateIdentity(auxiliaryIdentity)});
  guards.add(prove);owners.set(prove,owner);
  return prove;
}
function isHeldMainGuard(value,owner) {
  return typeof owner==='function'&&guards.has(value)&&owners.get(value)===owner;
}
module.exports={createHeldMainGuard,isHeldMainGuard};
