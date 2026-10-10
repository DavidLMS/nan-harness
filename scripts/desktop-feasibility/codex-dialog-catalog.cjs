// Passive exact public DialogTitle literals; never an input or dismissal policy.
const macCatalog=require('./codex-dialog-title-catalog-macos.json');
const macCatalogSha256='82df6ff119bf98beba8ffe1a593aca671119decdbb8f4f3d39e5378e39b02c48';
const linuxCatalog=require('./codex-dialog-title-catalog-linux.json');
const linuxCatalogSha256='b6566a8d50edd58ed59e29eb2c9ef9de10d72f6e650f3ee0ec0a50a927106ee0';
const catalog=require('./codex-dialog-title-catalog.json');
const windowsCatalog=require('./codex-dialog-title-catalog-windows.json');
const windowsCatalogSha256='617c94534ff816da4c332d7e0ee74addeddae9c3d5412ea7d5102e3fa0a5592c';
const pins={
 win32:{artifact:'36770adda59f71027e94d3d18ff7087da45a41990d1a1575080cd3149dcab30a',wrapper:'a1daa891e89a395b8db399ff6eaa365f12a86880c3d8cf891caf4ccbcb9eebf5',runner:'Windows'},
 linux:{artifact:'ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c',wrapper:'c3c9a86a6d9c3a2a8cecaf0a6a22527c69f89949cb0d8958896bc86131e9c6c9',runner:'Linux'},
 darwin:{artifact:'f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7',wrapper:'0703d0aa97450d6d21346e1c79c887a5bf9062cd0069e8251ec03748a33b6dd0',runner:'macOS'}
};
const catalogSha256='9fa1cdc597524b54f3e7c8ed7477fc565849b3c5989f66be91ff868098188ba3';
function policy(app,platform,env) {
 const pin=pins[platform];
 return !!pin&&app==='chatgpt-desktop'&&env.GITHUB_ACTIONS==='true'
  &&env.RUNNER_ENVIRONMENT==='github-hosted'&&env.RUNNER_OS===pin.runner
  &&env.NANH_CODEX_PUBLIC_ONBOARDING==='engineering'
  &&env.NANH_CODEX_PROJECT_POLICY==='open-project'
  &&env.NANH_CODEX_PROJECT_ARTIFACT_SHA256===pin.artifact;
}
function facts(platform) {
 const pin=pins[platform];
 return {schemaVersion:1,mechanism:'codex-static-dialog-title',diagnosticsOnly:true,
  sourceVersion:platform==='linux'?linuxCatalog.sourceVersion:platform==='darwin'?macCatalog.sourceVersion:windowsCatalog.sourceVersion,platform:platform==='win32'?'windows':platform==='darwin'?'macos':'linux',
  artifactSha256:pin.artifact,wrapperSourceSha256:pin.wrapper,catalogSha256:platform==='win32'?windowsCatalogSha256:platform==='linux'?linuxCatalogSha256:platform==='darwin'?macCatalogSha256:catalogSha256,
  status:'guard-rejected',sourceShape:null,commandMenuShape:null,rejectionStage:'unmeasured',guardFailure:null,titleReferenceCount:null,matchCount:null,sourceTitleEmpty:null,sourceTitleIds:[]};
}
// Standalone callbacks: no closure references, app text or DOM IDs leave the page.
function holdDialog() {
 const visible=e=>e.isConnected&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0
  &&getComputedStyle(e).display!=='none'&&getComputedStyle(e).visibility!=='hidden';
 const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
 if(dialogs.length!==1)return {document,dialog:null,title:null,reference:null};
 const dialog=dialogs[0],reference=dialog.getAttribute('aria-labelledby');
 if(typeof reference!=='string'||reference.length>128||! /^[A-Za-z0-9_:.-]+$/.test(reference))return {document,dialog,title:null,reference};
 const named=[...document.querySelectorAll('[id]')];
 const titles=named.length<=4096?named.filter(e=>e.id===reference):[];
 return {document,dialog,title:titles.length===1?titles[0]:null,reference};
}
function classifyTitle({held,entries}) {
 const reject=(rejectionStage,guardFailure=null)=>({rejectionStage,guardFailure});
 if(!held||held.document!==document)return reject('scope','retained-document');
 if(!document.hasFocus())return reject('scope','document-focus');
 const visible=e=>e.isConnected&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0
  &&getComputedStyle(e).display!=='none'&&getComputedStyle(e).visibility!=='hidden';
 const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
 if(dialogs.length!==1)return reject('dialog-count');
 if(dialogs[0]!==held.dialog||!held.dialog.isConnected||held.dialog.ownerDocument!==document
  ||held.dialog.getAttribute('role')!=='dialog')return reject('changed');
 if(entries.length>256)return reject('scope','catalog-limit');
 let current=held.dialog,depth=0;
 while(current) {
  if(++depth>64||current.inert||current.getAttribute('aria-hidden')==='true'
   ||current.getAttribute('data-state')==='closed')return reject('actionability');
  const style=getComputedStyle(current),raw=style.opacity;
  if(typeof raw!=='string'||! /^(?:0(?:\.\d+)?|1(?:\.0+)?)$/.test(raw)
   ||!(Number(raw)>0)||style.display==='none'||style.visibility==='hidden')return reject('actionability');
  if(current===held.dialog&&style.pointerEvents==='none')return reject('actionability');
  current=current.parentElement;
 }
 const reference=held.dialog.getAttribute('aria-labelledby');
 if(typeof reference!=='string'||reference.length>128||! /^[A-Za-z0-9_:.-]+$/.test(reference))return reject('reference');
 const named=[...document.querySelectorAll('[id]')];
 if(named.length>4096)return reject('title-count');
 const titles=named.filter(e=>e.id===reference);
 if(titles.length!==1)return reject('title-count');
 if(titles[0]!==held.title||reference!==held.reference)return reject('changed');
 const title=titles[0];
 if(title.ownerDocument!==document||!title.isConnected||!held.dialog.contains(title))return reject('changed');
 if(title.tagName!=='H2')return reject('title-tag');
 const text=title.textContent;
 if(typeof text!=='string'||text.length>512)return reject('title-text');
 const legends=[...document.querySelectorAll('fieldset > legend')].filter(e=>visible(e)&&e.textContent?.trim()==='Select the kind of work you do');
 const radios=[...document.querySelectorAll('input[type="radio"][name="conversational-onboarding-inline-role"]')];
 const engineering=radios.filter(e=>e.getAttribute('value')==='engineering');
 const buttons=[...held.dialog.querySelectorAll('button')].filter(visible);
 const lists=[legends,legends.filter(e=>held.dialog.contains(e)),radios,radios.filter(e=>held.dialog.contains(e)),
  engineering,engineering.filter(e=>held.dialog.contains(e)),buttons.filter(e=>e.textContent?.trim()==='Continue'),
  buttons.filter(e=>e.textContent?.trim()==='Get Started')];
 const sourceShape=lists.some(e=>e.length>4096)?null:Object.fromEntries(
  ['pageRoleLegend','dialogRoleLegend','pageRoleRadios','dialogRoleRadios','pageEngineering','dialogEngineering','dialogContinue','dialogGetStarted'].map((key,i)=>[key,lists[i].length]));
 const commandLists=['[cmdk-root]','input[cmdk-input][role="combobox"]','[cmdk-list][role="listbox"]']
  .map(selector=>[...held.dialog.querySelectorAll(selector)]);
 const commandMenuShape=commandLists.some(list=>list.length>4096)?null:{
  dialogMarkerCount:held.dialog.getAttribute('cmdk-dialog')!==null?1:0,
  globalScopeCount:held.dialog.classList?.contains('global-command-menu-dialog')?1:0,
  rootCount:commandLists[0].length,inputCount:commandLists[1].length,listCount:commandLists[2].length};
 const matches=[...new Set(entries.filter(e=>e.text===text.trim()).map(e=>e.id))].sort();
 return {status:matches.length===1?'matched':matches.length?'ambiguous':'unknown',
  sourceShape,commandMenuShape,titleReferenceCount:1,matchCount:matches.length,sourceTitleEmpty:text.trim().length===0,sourceTitleIds:matches,rejectionStage:null,guardFailure:null};
}
async function observe(held,platform,{guard,identity,same,deadline,progress=()=>{}}) {
 const result=facts(platform);let handle;
 const checkpoint=phase=>{try {progress(phase);}catch {}};
 const prove=async final=>{
  if(Date.now()>=deadline){result.rejectionStage='deadline';return false;}
  if(!held||held.url!=='app://-/index.html'){result.rejectionStage='scope';result.guardFailure='held-document';return false;}
  checkpoint(final?'identity-after':'guard-before');
  if(!final&&!await guard()){result.rejectionStage='scope';result.guardFailure=guard.lastFailure??null;return false;}
  checkpoint(final?'identity-after':'identity-before');
  if(!same(held,await identity(held.page))){result.rejectionStage='changed';return false;}
  if(Date.now()>=deadline){result.rejectionStage='deadline';return false;}
  checkpoint(final?'guard-after':'identity-before');
  if(final&&!await guard()){result.rejectionStage='scope';result.guardFailure=guard.lastFailure??null;return false;}
  if(Date.now()>=deadline){result.rejectionStage='deadline';return false;}
  return true;
 };
 try {
  if(!await prove())return result;
  checkpoint('hold-dialog');
  handle=await held.page.evaluateHandle(holdDialog);
  const source=platform==='win32'?windowsCatalog:platform==='linux'?linuxCatalog:platform==='darwin'?macCatalog:catalog;
  const entries=Object.values(Object.fromEntries(source.entries.filter(e=>e.platform===(platform==='win32'?'windows':platform==='darwin'?'mac':'linux')).map(e=>[e.id,e])));
  checkpoint('sample-first');
  const first=await held.page.evaluate(classifyTitle,{held:handle,entries});
  if(!first)return result;
  if(Date.now()>=deadline){result.rejectionStage='deadline';return result;}
  // Two passive retained-node samples form one transaction. Fresh ownership
  // and original CDP identity bracket it; no GUI action occurs inside.
  checkpoint('sample-second');
  const second=await held.page.evaluate(classifyTitle,{held:handle,entries});
  if(!second||!await prove(true))return result;
  if(JSON.stringify(first)!==JSON.stringify(second)){result.rejectionStage='changed';return result;}
  Object.assign(result,second);
 } catch(_) {result.rejectionStage='query';} finally {checkpoint('finished');if(handle)await handle.dispose().catch(()=>{});}
 return result;
}
module.exports={policy,facts,holdDialog,classifyTitle,observe,catalogSha256,pins};
