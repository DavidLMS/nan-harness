'use strict';
// Frozen Linux bootstrap can choose app after its native workspace lookup times
// out. This passive admission retains the original page and one exact DOM tuple.
const {directCDPPolicy,homeComposerScope}=require('./codex-dom.cjs');
const {codingScope}=require('./codex-onboarding.cjs');
const {isHeldMainGuard}=require('./codex-main-guard.cjs');
const ARTIFACT='ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c';
function policy(platform,env,loan) {
  return platform==='linux'&&directCDPPolicy(platform,env)
    &&env.NANH_CODEX_PUBLIC_ONBOARDING==='engineering'
    &&env.NANH_CODEX_PROJECT_ARTIFACT_SHA256===ARTIFACT
    &&loan?.schemaVersion===1&&loan.platform==='linux';
}
// Standalone browser callback. No application callbacks, stores or text reads.
function tuple(held) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&e.ownerDocument===document&&r.width>0&&r.height>0
      &&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  const nodes=[...document.querySelectorAll('*')];
  if(nodes.length>4096)return held?'changed':{status:'invalid'};
  if(nodes.some(e=>['IFRAME','FRAME'].includes(e.tagName)))return held?'changed':{status:'invalid'};
  const live=nodes.filter(visible);
  const homes=live.filter(e=>e.hasAttribute('data-codex-composer-root')
    &&e.getAttribute('data-composer-placement')==='home');
  if(homes.length===0)return held?'changed':{status:'absent'};
  const editors=live.filter(e=>e.tagName==='TEXTAREA'||e.getAttribute('contenteditable')==='true'
    ||e.tagName==='INPUT'&&!['hidden','radio','checkbox'].includes(e.getAttribute('type')));
  const foreign=live.some(e=>['dialog','alertdialog','menu'].includes(e.getAttribute('role'))
    ||e.getAttribute('aria-modal')==='true'||e.getAttribute('data-thread-find-target')==='conversation'
    ||e.hasAttribute('data-avatar-overlay-composition-autofocus')
    ||e.getAttribute('name')==='conversational-onboarding-inline-role');
  const root=homes[0],editor=editors[0];
  const valid=homes.length===1&&editors.length===1&&!foreign
    &&document.visibilityState==='visible'&&document.hasFocus()
    &&root.contains(editor)&&editor.classList.contains('ProseMirror')
    &&editor.getAttribute('contenteditable')==='true'
    &&editor.getAttribute('aria-disabled')!=='true'&&!editor.disabled&&!editor.readOnly;
  if(!valid)return held?'changed':{status:'invalid'};
  if(held)return held.document===document&&held.root===root&&held.editor===editor?'matched':'changed';
  return {status:'candidate',document,root,editor};
}
function proofReceipt() {
  return {sourceVersion:'26.930.41038',
    bootstrapSourceSha256:'28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d',
    composerSourceSha256:'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
    profileRetained:true,nativeOwnerVerified:true,mainIdentityRetained:true,
    documentRetained:true,documentFocused:true,composerRetained:true,
    uniqueHomeComposer:true,stableSamples:2};
}
function onboardingReceipt(folderTrust) {
  return {schemaVersion:1,mechanism:'codex-public-onboarding',diagnosticsOnly:true,
    stage:'fresh-linux-home',errorCategory:null,conversationalScope:false,engineeringControl:false,
    roleClickAttempted:false,roleClickCompleted:false,engineeringChecked:false,
    continueControl:false,continueClickAttempted:false,continueClickCompleted:false,
    roleScopeAbsent:true,taskScopeProved:false,taskClickAttempted:false,taskClickCompleted:false,
    codingComposerReady:true,roleProofFailure:'unmeasured',sessionProofFailure:'unmeasured',
    ...(folderTrust===undefined?{}:{folderTrust})};
}
function create({held,guard,owner,profile,identity,same,deadline,
  now=Date.now,pause=ms=>new Promise(resolve=>setTimeout(resolve,ms))}) {
  let handle=null,failed=false,closed=false,confirmed=false,lastIdentity;
  const bounded=async work=>{let timer;
    try {return await Promise.race([work,new Promise((_,reject)=>{
      timer=setTimeout(()=>reject(Error('cutoff')),Math.max(1,deadline-now()));
    })]);} finally {clearTimeout(timer);}
  };
  const fresh=async(requireHome=true)=>{
    if(closed||failed||now()>=deadline||profile.verify()!==true||!await guard()
      ||now()>=deadline)throw Error('authority');
    const current=await bounded(identity(held.page,deadline));
    if(!same(held,current)||requireHome&&(!current.scope.focused||current.scope.visibleDocument!==true
      ||current.scope.mainScope||Object.entries({roleLegend:0,roleRadios:0,engineering:0,
        dialog:0,quickChatComposer:0,editable:1}).some(([key,count])=>current.scope.counts[key]!==count))
      ||now()>=deadline||profile.verify()!==true||!await guard()||now()>=deadline)throw Error('identity');
    lastIdentity=current;
  };
  const verify=async()=>{
    try {
      if(!handle)throw Error('tuple');
      await fresh();
      if(await bounded(handle.evaluate(tuple))!=='matched')throw Error('tuple');
      if(await bounded(held.page.evaluate(codingScope))!==false
        ||await bounded(held.page.evaluate(homeComposerScope))!==true)throw Error('source');
      await fresh();return true;
    } catch {failed=true;return false;}
  };
  const close=async()=>{closed=true;const previous=handle;handle=null;
    if(previous)try {await bounded(previous.dispose());}catch {}
  };
  return {
    async tryBind() {
      if(closed||failed||confirmed)return {status:'rejected'};
      try {
        if(!isHeldMainGuard(guard,owner)||typeof profile?.verify!=='function')throw Error('capability');
        await fresh(false);
        const pending=held.page.evaluateHandle(tuple,null).then(async captured=>{
          if(closed||failed){try {await captured.dispose();}catch {}throw Error('closed');}
          return captured;
        });
        handle=await bounded(pending);
        const status=await bounded(handle.evaluate(value=>value.status));
        if(status==='absent') {await fresh(false);await close();closed=false;return {status:'absent'};}
        if(status!=='candidate'||!await verify())throw Error('tuple');
        await pause(Math.min(100,Math.max(0,deadline-now())));
        if(!await verify())throw Error('tuple');
        // The role path seals on its first click. Skipping that click must never
        // leave initial auxiliary/activation admission available to later input.
        guard.sealInitialActions();guard.requireDocumentFocus();
        if(!await verify())throw Error('tuple');
        confirmed=true;
        return {status:'confirmed',identity:lastIdentity,
          binding:{guard,verify,close,proof:proofReceipt()}};
      } catch {failed=true;await close();return {status:'rejected'};}
    },
    close,
  };
}
module.exports={policy,tuple,create,onboardingReceipt};
