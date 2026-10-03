// Passive exact public DialogTitle literals; never an input or dismissal policy.
const macCatalog=require('./codex-dialog-title-catalog-macos.json');
const macCatalogSha256='cb0dad04840297918677c21b87182b4845117e90c93b414a428dabcd4d332a93';
const linuxCatalog=require('./codex-dialog-title-catalog-linux.json');
const linuxCatalogSha256='1922e550abd0c95c9190ae82478f07f1485cdc07e6c9132a16c11a3314229816';
const catalog=require('./codex-dialog-title-catalog.json');
const windowsCatalog=require('./codex-dialog-title-catalog-windows.json');
const windowsCatalogSha256='dff2a1184ab65c0ad8497ea90984ccb19be01c09f6a025a8e1e9d96b3bc4f467';
const pins={
 win32:{artifact:'f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87',wrapper:'5e3a36d643393af861d2009584f64289f2247928e793f1985fe12cfec803a40b',runner:'Windows'},
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
  sourceVersion:platform==='linux'?linuxCatalog.sourceVersion:platform==='darwin'?macCatalog.sourceVersion:catalog.sourceVersion,platform:platform==='win32'?'windows':platform==='darwin'?'macos':'linux',
  artifactSha256:pin.artifact,wrapperSourceSha256:pin.wrapper,catalogSha256:platform==='win32'?windowsCatalogSha256:platform==='linux'?linuxCatalogSha256:platform==='darwin'?macCatalogSha256:catalogSha256,
  status:'guard-rejected',rejectionStage:'unmeasured',titleReferenceCount:null,matchCount:null,sourceTitleEmpty:null,sourceTitleIds:[]};
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
 const reject=rejectionStage=>({rejectionStage});
 if(!held||held.document!==document||!document.hasFocus())return reject('scope');
 const visible=e=>e.isConnected&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0
  &&getComputedStyle(e).display!=='none'&&getComputedStyle(e).visibility!=='hidden';
 const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
 if(dialogs.length!==1)return reject('dialog-count');
 if(dialogs[0]!==held.dialog||!held.dialog.isConnected||held.dialog.ownerDocument!==document
  ||held.dialog.getAttribute('role')!=='dialog')return reject('changed');
 if(entries.length>256)return reject('scope');
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
 const matches=[...new Set(entries.filter(e=>e.text===text.trim()).map(e=>e.id))].sort();
 return {status:matches.length===1?'matched':matches.length?'ambiguous':'unknown',
  titleReferenceCount:1,matchCount:matches.length,sourceTitleEmpty:text.trim().length===0,sourceTitleIds:matches,rejectionStage:null};
}
async function observe(held,platform,{guard,identity,same,deadline}) {
 const result=facts(platform);let handle;
 const prove=async()=>{
  if(Date.now()>=deadline){result.rejectionStage='deadline';return false;}
  if(!held||held.url!=='app://-/index.html'||!await guard()){result.rejectionStage='scope';return false;}
  if(!same(held,await identity(held.page))){result.rejectionStage='changed';return false;}
  if(Date.now()>=deadline){result.rejectionStage='deadline';return false;}
  if(!await guard()){result.rejectionStage='scope';return false;}
  return true;
 };
 try {
  if(!await prove())return result;
  handle=await held.page.evaluateHandle(holdDialog);
  const source=platform==='win32'?windowsCatalog:platform==='linux'?linuxCatalog:platform==='darwin'?macCatalog:catalog;
  const entries=Object.values(Object.fromEntries(source.entries.filter(e=>e.platform===(platform==='win32'?'windows':platform==='darwin'?'mac':'linux')).map(e=>[e.id,e])));
  if(!await prove())return result;
  const first=await held.page.evaluate(classifyTitle,{held:handle,entries});
  if(!first||!await prove())return result;
  const second=await held.page.evaluate(classifyTitle,{held:handle,entries});
  if(!second||!await prove())return result;
  if(JSON.stringify(first)!==JSON.stringify(second)){result.rejectionStage='changed';return result;}
  Object.assign(result,second);
 } catch(_) {result.rejectionStage='query';} finally {if(handle)await handle.dispose().catch(()=>{});}
 return result;
}
module.exports={policy,facts,holdDialog,classifyTitle,observe,catalogSha256,pins};
