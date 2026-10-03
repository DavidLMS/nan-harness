// Passive exact public DialogTitle literals; never an input or dismissal policy.
const catalog=require('./codex-dialog-title-catalog.json');
const windowsCatalog=require('./codex-dialog-title-catalog-windows.json');
const windowsCatalogSha256='27524df1c017bf62e2db2b5578ae60d4e102dcb2df0a0d7fe1faea0f410dc377';
const pins={
 win32:{artifact:'f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87',wrapper:'5e3a36d643393af861d2009584f64289f2247928e793f1985fe12cfec803a40b',runner:'Windows'},
 linux:{artifact:'e0174d8d0a5f4141145458c814f3c2d863dd67e942b868785a1f5dac9cba3e16',wrapper:'1d2a61d9ad9d603c46eb04df3c140a8e5ae6789393429881d3d8a6319fcf39ba',runner:'Linux'},
 darwin:{artifact:'bfda661a7c9ca44dac3168134058dd6007947cde318ade37d570c484329f6d41',wrapper:'df6152796a7762d3956cbf2030bd8b17de90a4554513d786f11cd41f88b892d4',runner:'macOS'}
};
const catalogSha256='6875f72d89a61cab995daccf0bf8075e78324c8e9981e653dcb46fd377e73ee8';
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
  sourceVersion:catalog.sourceVersion,platform:platform==='win32'?'windows':platform==='darwin'?'macos':'linux',
  artifactSha256:pin.artifact,wrapperSourceSha256:pin.wrapper,catalogSha256:platform==='win32'?windowsCatalogSha256:catalogSha256,
  status:'guard-rejected',titleReferenceCount:null,matchCount:null,sourceTitleIds:[]};
}
// Standalone callbacks: no closure references, app text or DOM IDs leave the page.
function holdDialog() {
 const visible=e=>e.isConnected&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0
  &&getComputedStyle(e).display!=='none'&&getComputedStyle(e).visibility!=='hidden';
 const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
 if(dialogs.length!==1)return null;
 const dialog=dialogs[0],reference=dialog.getAttribute('aria-labelledby');
 if(typeof reference!=='string'||reference.length>128||! /^[A-Za-z0-9_:.-]+$/.test(reference))return null;
 const named=[...document.querySelectorAll('[id]')];if(named.length>4096)return null;
 const titles=named.filter(e=>e.id===reference);if(titles.length!==1)return null;
 return {document,dialog,title:titles[0],reference};
}
function classifyTitle({held,entries}) {
 if(!held||held.document!==document||!document.hasFocus()||!held.dialog.isConnected
  ||held.dialog.ownerDocument!==document||held.dialog.getAttribute('role')!=='dialog')return null;
 const visible=e=>e.isConnected&&e.getBoundingClientRect().width>0&&e.getBoundingClientRect().height>0
  &&getComputedStyle(e).display!=='none'&&getComputedStyle(e).visibility!=='hidden';
 const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
 if(dialogs.length!==1||dialogs[0]!==held.dialog||entries.length>63)return null;
 let current=held.dialog,depth=0;
 while(current) {
  if(++depth>64||current.inert||current.getAttribute('aria-hidden')==='true'
   ||current.getAttribute('data-state')==='closed')return null;
  const style=getComputedStyle(current),raw=style.opacity;
  if(typeof raw!=='string'||! /^(?:0(?:\.\d+)?|1(?:\.0+)?)$/.test(raw)
   ||!(Number(raw)>0)||style.display==='none'||style.visibility==='hidden')return null;
  if(current===held.dialog&&style.pointerEvents==='none')return null;
  current=current.parentElement;
 }
 const reference=held.dialog.getAttribute('aria-labelledby');
 if(typeof reference!=='string'||reference.length>128||! /^[A-Za-z0-9_:.-]+$/.test(reference))return null;
 const named=[...document.querySelectorAll('[id]')];
 if(named.length>4096)return null;
 const titles=named.filter(e=>e.id===reference);
 if(titles.length!==1||titles[0]!==held.title||reference!==held.reference)return null;
 const title=titles[0];
 if(title.ownerDocument!==document||!title.isConnected||!held.dialog.contains(title)||title.tagName!=='H2')return null;
 const text=title.textContent;
 if(typeof text!=='string'||text.length>512)return null;
 const matches=[...new Set(entries.filter(e=>e.text===text.trim()).map(e=>e.id))].sort();
 return {status:matches.length===1?'matched':matches.length?'ambiguous':'unknown',
  titleReferenceCount:1,matchCount:matches.length,sourceTitleIds:matches};
}
async function observe(held,platform,{guard,identity,same,deadline}) {
 const result=facts(platform);let handle;
 const prove=async()=>held&&held.url==='app://-/index.html'&&Date.now()<deadline&&await guard()
  &&same(held,await identity(held.page))&&Date.now()<deadline&&await guard();
 try {
  if(!await prove())return result;
  handle=await held.page.evaluateHandle(holdDialog);
  const source=platform==='win32'?windowsCatalog:catalog;
  const entries=Object.values(Object.fromEntries(source.entries.filter(e=>e.platform===(platform==='win32'?'windows':platform==='darwin'?'mac':'linux')).map(e=>[e.id,e])));
  if(!await prove())return result;
  const first=await held.page.evaluate(classifyTitle,{held:handle,entries});
  if(!first||!await prove())return result;
  const second=await held.page.evaluate(classifyTitle,{held:handle,entries});
  if(!second||JSON.stringify(first)!==JSON.stringify(second)||!await prove())return result;
  Object.assign(result,second);
 } catch(_) {} finally {if(handle)await handle.dispose().catch(()=>{});}
 return result;
}
module.exports={policy,facts,holdDialog,classifyTitle,observe,catalogSha256,pins};
