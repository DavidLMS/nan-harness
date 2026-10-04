'use strict';
const {parseNative}=require('./codex-point-observation.cjs');
// The frozen ordinary Engineering role label, never a viewport-center target.
function captureSource({scope,group}){
 const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]')};
 const legends=[...document.querySelectorAll('fieldset > legend')].filter(e=>visible(e)&&e.textContent.trim()==='Select the kind of work you do');
 if(legends.length!==1)return null;const fieldset=legends[0].parentElement,root=fieldset.closest(scope);
 if(!root||!visible(root)||[...document.querySelectorAll(scope)].filter(visible).length!==1)return null;
 const radios=[...fieldset.querySelectorAll(group+'[value="engineering"]')];
 if(radios.length!==1||radios[0].disabled||radios[0].labels?.length!==1)return null;
 const radio=radios[0],label=radio.labels[0];if(!fieldset.contains(label)||label.innerText.trim()!=='Engineering'||!visible(label))return null;
 if([...root.querySelectorAll('button')].some(e=>visible(e)&&/^(Log in|Sign in|Continue with Google|Continue with Apple)$/.test(e.innerText.trim())))return null;
 return {root,fieldset,radio,label,scope,group};
}
function sampleSource(){
 const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]')};
 if(!this.root||!this.fieldset||!this.radio||!this.label||![this.root,this.fieldset,this.label].every(visible)||!this.radio.isConnected
  ||this.radio.ownerDocument!==document||this.label.ownerDocument!==document||this.radio.disabled||this.radio.getAttribute('aria-disabled')==='true'
  ||this.label.getAttribute('aria-disabled')==='true'||this.radio.labels?.length!==1||this.radio.labels[0]!==this.label||this.label.innerText.trim()!=='Engineering'
  ||!this.root.contains(this.fieldset)||!this.fieldset.contains(this.radio)||!this.fieldset.contains(this.label))return null;
 const roots=[...document.querySelectorAll(this.scope)].filter(visible),legends=[...document.querySelectorAll('fieldset > legend')].filter(e=>visible(e)&&e.textContent.trim()==='Select the kind of work you do');
 const radios=[...this.fieldset.querySelectorAll(this.group+'[value="engineering"]')];
 if(roots.length!==1||roots[0]!==this.root||legends.length!==1||legends[0].parentElement!==this.fieldset||radios.length!==1||radios[0]!==this.radio)return null;
 if([...document.querySelectorAll('[role="dialog"],[role="alertdialog"],[role="menu"],[aria-modal="true"]')].some(visible))return null;
 for(let e=this.label,n=0;e;e=e.parentElement)if(++n>64||getComputedStyle(e).pointerEvents==='none')return null;
 const r=this.label.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
 if(![r.left,r.top,r.width,r.height,x,y,innerWidth,innerHeight].every(Number.isFinite)||r.width<=0||r.height<=0||x<=0||y<=0||x>=innerWidth||y>=innerHeight||!this.label.contains(document.elementFromPoint(x,y)))return null;
 return {rect:[r.left,r.top,r.width,r.height],css:[innerWidth,innerHeight,x,y]};
}
async function run({session,native,held,deadline,owner,now=Date.now,policy=process.env}){
 const facts={reason:'source-policy-rejected',sourcePointRetained:false,rendererReproved:false,postMappingObserved:false,inputAuthorized:false};
 const allowed=policy.GITHUB_ACTIONS==='true'&&policy.RUNNER_ENVIRONMENT==='github-hosted'&&policy.RUNNER_OS==='macOS'
  &&policy.NANH_CODEX_OWNED_MOVE==='source-point'&&policy.NANH_CODEX_PROJECT_POLICY==='open-project'
  &&policy.NANH_CODEX_PUBLIC_ONBOARDING==='engineering'&&policy.NANH_CODEX_PROJECT_ARTIFACT_SHA256==='f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7';
 if(!allowed)return facts;let retained;const group='nanh-owned-move-source';
 const fresh=()=>{if(now()>=deadline||owner()!==true)throw Error('deadline-or-owner')};
 const send=async(method,params={})=>{fresh();let timer;try{
  const out=await Promise.race([session.send(method,params),new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('deadline-or-owner')),Math.max(1,deadline-now()))})]);
  fresh();if(out.exceptionDetails)throw Error('source-point-unavailable');return out;
 }finally{clearTimeout(timer)}};
 const identity=async()=>{
  const target=(await send('Target.getTargetInfo',{targetId:held.target})).targetInfo,tree=(await send('Page.getFrameTree')).frameTree,frame=tree?.frame;
  if(tree?.childFrames?.length||!target||!frame||target.targetId!==held.target||target.url!==held.url||frame.id!==held.frame||frame.loaderId!==held.loader||frame.url!==held.frameUrl||(frame.urlFragment??'')!==held.fragment)throw Error('renderer-changed');
  const m=await send('Page.getLayoutMetrics'),l=m.cssLayoutViewport,v=m.cssVisualViewport;
  if(!l||!v||![l.clientWidth,l.clientHeight,v.clientWidth,v.clientHeight,l.pageX,l.pageY,v.pageX,v.pageY,v.offsetX,v.offsetY,v.scale].every(Number.isFinite)
    ||l.clientWidth<1||l.clientHeight<1||l.clientWidth>16384||l.clientHeight>16384||l.clientWidth!==v.clientWidth||l.clientHeight!==v.clientHeight
    ||l.pageX!==0||l.pageY!==0||v.pageX!==0||v.pageY!==0||v.offsetX!==0||v.offsetY!==0||v.scale!==1)throw Error('renderer-changed');
  return [l.clientWidth,l.clientHeight];
 };
 const sample=async()=>{
  const size=await identity(),r=await send('Runtime.callFunctionOn',{objectId:retained.objectId,functionDeclaration:sampleSource.toString(),returnByValue:true,silent:true});
  const value=r.result?.value;
  if(!value||!Array.isArray(value.rect)||value.rect.length!==4||!Array.isArray(value.css)||value.css.length!==4||![...value.rect,...value.css].every(Number.isFinite)
    ||value.css[0]!==size[0]||value.css[1]!==size[1])throw Error('source-point-unavailable');return value;
 };
 try{
  if(!held||held.url!==held.frameUrl||held.fragment!=='')throw Error('renderer-changed');await identity();
  const params={scope:require('./codex-onboarding.cjs').scopeFingerprint,group:'input[type="radio"][name="conversational-onboarding-inline-role"]'};
  retained=(await send('Runtime.evaluate',{expression:'('+captureSource.toString()+')('+JSON.stringify(params)+')',objectGroup:group,returnByValue:false,silent:true})).result;
  if(!retained?.objectId)throw Error('source-point-unavailable');
  const a=await sample(),b=await sample();if(JSON.stringify(a)!==JSON.stringify(b))throw Error('renderer-changed');facts.sourcePointRetained=true;
  fresh();const pre=parseNative(native.pointObserve(a.css,held.frameUrl)).facts;fresh();
  if(pre.reason!=='point-occluded'||!pre.webAreaStable||!pre.nativeFocused||!pre.dimensionsMatched||!pre.heldIdentityStable||!pre.webAreaUrlMatched)throw Error('source-point-unavailable');
  if(JSON.stringify(a)!==JSON.stringify(await sample()))throw Error('renderer-changed');
  fresh();facts.native=native.moveOwned(a.css,held.frameUrl,'onboarding-engineering');fresh();
  if(JSON.stringify(a)!==JSON.stringify(await sample()))throw Error('renderer-changed');facts.rendererReproved=true;
  if(facts.native.reason!=='moved-point-observed'){facts.reason='native-move-rejected';return facts;}
  const post=parseNative(native.pointObserve(a.css,held.frameUrl)).facts;fresh();
  if(JSON.stringify(a)!==JSON.stringify(await sample()))throw Error('renderer-changed');
  facts.postMappingObserved=post.mappingObserved===true;facts.reason=facts.postMappingObserved?'moved-source-point-observed':'post-mapping-unproved';return facts;
 }catch(error){facts.reason=['deadline-or-owner','renderer-changed','source-point-unavailable'].includes(error.message)?error.message:'move-unavailable-or-uncertain';return facts;}
 finally{if(retained){let timer;try{await Promise.race([session.send('Runtime.releaseObjectGroup',{objectGroup:group}),new Promise(resolve=>{timer=setTimeout(resolve,Math.max(1,deadline-now()))})])}catch{}finally{clearTimeout(timer)}}}
}
module.exports={run,captureSource,sampleSource};
