'use strict';
// Independent original CDP node loan; no cross-session node-identity claim.
function capture(){
 const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]')};
 const homes=[...document.querySelectorAll('[data-codex-composer-root][data-composer-placement="home"]')].filter(visible);
 if(homes.length!==1)return null;const home=homes[0],editors=[...home.querySelectorAll('.ProseMirror[contenteditable="true"]')].filter(visible);
 return editors.length===1?{home,editor:editors[0]}:null;
}
function connected(){
 const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]')};
 const homes=[...document.querySelectorAll('[data-codex-composer-root][data-composer-placement="home"]')].filter(visible);
 const editors=this.home?[...this.home.querySelectorAll('.ProseMirror[contenteditable="true"]')].filter(visible):[];
 return homes.length===1&&homes[0]===this.home&&editors.length===1&&editors[0]===this.editor&&visible(this.editor)&&this.home.contains(this.editor);
}
const blocked=reason=>({verified:false,reason,inputAuthorized:false});
function create({page,alive,deadline,pwProof,makeWitness=require('./codex-send-context-witness.cjs').create,now=Date.now}){
 let session,editor,heldDOM,identity,closed=false,proofFailure=null;const group='nanh-passive-send-context';
 const bounded=async work=>{let timer;try{return await Promise.race([work,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('deadline-or-owner')),Math.max(1,deadline-now()));})]);}finally{clearTimeout(timer);}};
 const fresh=async()=>{if(closed||now()>=deadline||!await alive()||now()>=deadline)throw Error('deadline-or-owner');};
 const send=async(method,params)=>{await fresh();const out=await bounded(session.send(method,{...params,...(method==='Runtime.callFunctionOn'?{objectGroup:group}:{})}));await fresh();if(out.exceptionDetails)throw Error('runtime-unavailable');return out;};
 const current=async()=>{
  const t=await send('Target.getTargetInfo'),f=await send('Page.getFrameTree');const frame=f.frameTree?.frame;
  if(!frame||f.frameTree.childFrames?.length)throw Error('identity-changed');
  const value={target:t.targetInfo?.targetId,frame:frame.id,loader:frame.loaderId,url:frame.url,fragment:frame.urlFragment??''};
  if(Object.values(value).some(v=>typeof v!=='string')||!value.target||!value.frame||!value.loader||page.url()!==value.url+value.fragment)throw Error('identity-changed');return value;
 };
 const same=(a,b)=>['target','frame','loader','url','fragment'].every(k=>a[k]===b[k]);
 const verifyDocument=async()=>{
  await fresh();if(!same(identity,await current()))throw Error('identity-changed');
  const proof=await send('Runtime.callFunctionOn',{objectId:heldDOM.objectId,functionDeclaration:connected.toString(),returnByValue:true,silent:true});
  if(proof.result?.value!==true)throw Error('editor-changed');return true;
 };
 const verify=async held=>{
  await verifyDocument();if(!await bounded(pwProof(held))||!await alive())throw Error('identity-changed');
  return verifyDocument();
 };
 const failure=error=>blocked(['deadline-or-owner','identity-changed','editor-unavailable','editor-changed'].includes(error.message)?error.message:'runtime-unavailable');
 return {
  async prepare(held){
   try{await fresh();session=await bounded(page.context().newCDPSession(page).then(async created=>{
      if(closed){try{await created.detach();}catch{}throw Error('deadline-or-owner');}
      session=created;return created;
    }));await fresh();identity=await current();
    const captured=await send('Runtime.evaluate',{expression:'('+capture.toString()+')()',objectGroup:group,returnByValue:false,silent:true});heldDOM=captured.result;
    if(!heldDOM?.objectId)throw Error('editor-unavailable');
    const props=await send('Runtime.getProperties',{objectId:heldDOM.objectId,ownProperties:true,generatePreview:false});
    const row=props.result?.filter(p=>p.name==='editor');
    if(row?.length!==1||row[0].get||!row[0].value?.objectId)throw Error('editor-unavailable');editor=row[0].value;
    await verify(held);return {verified:true,reason:'verified',inputAuthorized:false};
   }catch(error){return failure(error);}
  },
  // Read-only document/retained-node proof during the one popup-close transition.
  // The complete PW source proof remains mandatory before subsequent input.
  async verifyRetainedDocument(){
   try{if(!editor){proofFailure='editor-unavailable';return false;}await verifyDocument();proofFailure=null;return true;}catch(error){proofFailure=failure(error).reason;return false;}
  },
  retainedDocumentFailure:()=>proofFailure,
  async verifyHeld(held){
   try{if(!editor)return false;return await verify(held);}catch{return false;}
  },
  async observe(held,projectId,cwd){
   if(!editor)return blocked('editor-unavailable');
   try{await verify(held);const witness=makeWitness({session:{send},editor,guard:()=>verify(held),deadline,projectId,cwd,now});
    const result=await witness.observe({requireSelection:false});await verify(held);return result;
   }catch(error){return failure(error);}
  },
  async close(){if(closed)return;closed=true;
   if(session){try{await bounded(session.send('Runtime.releaseObjectGroup',{objectGroup:group}));}catch{}
    try{await bounded(session.detach());}catch{}}
  }
 };
}
module.exports={create,capture,connected};
