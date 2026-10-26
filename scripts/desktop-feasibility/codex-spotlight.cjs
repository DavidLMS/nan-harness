'use strict';
const ARTIFACT='36770adda59f71027e94d3d18ff7087da45a41990d1a1575080cd3149dcab30a';
const VERSION='26.1007.21434';
const RENDERER='2b78662247051c8b0905e1afbe0839187b8267e9181e179bc422322e4010433b';
// Inspected spotlight-page-7a8f7a62bd89.js and its 56b82c671210 CSS in the
// immutable Windows package. Neither a route nor this public receipt grants
// input; callers separately retain the original main and native profile owner.
function policy(platform=process.platform,env=process.env) {
  return platform==='win32'&&env.GITHUB_ACTIONS==='true'&&env.RUNNER_ENVIRONMENT==='github-hosted'
    &&env.RUNNER_OS==='Windows'&&env.NANH_DESKTOP_RENDERER_APP==='chatgpt-desktop'
    &&env.NANH_CODEX_PUBLIC_ONBOARDING==='engineering'&&env.NANH_CODEX_INPUT_CHANNEL==='cdp-dom'
    &&env.NANH_CODEX_PROJECT_POLICY==='open-project'&&env.NANH_DESKTOP_QUALIFICATION_MODE==='renderer'
    &&env.NANH_CODEX_PROJECT_ARTIFACT_SHA256===ARTIFACT;
}
function route(raw) {
  // Frozen packaged URL builder: app://-/index.html, one initialRoute parameter.
  return raw==='app://-/index.html?initialRoute=%2Fspotlight';
}
// One atomic DOM read includes honest generic counts and the private source
// tuple. No text, value, URL, node or computed style leaves the private handle.
function sample(held) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const all=s=>[...document.querySelectorAll(s)];
  const live=s=>all(s).filter(visible),cap=a=>Math.min(4096,a.length);
  const editables=live('textarea,[contenteditable="true"],input:not([type="radio"]):not([type="checkbox"]):not([type="hidden"])');
  const inputs=all('input[cmdk-input][role="combobox"]'),lists=all('[cmdk-list][role="listbox"]');
  const scope={counts:{roleLegend:cap(live('fieldset > legend').filter(e=>e.innerText.trim()==='Select the kind of work you do')),
    roleRadios:cap(live('input[type="radio"][name="conversational-onboarding-inline-role"]')),
    engineering:cap(live('input[type="radio"][name="conversational-onboarding-inline-role"]').filter(e=>e.value==='engineering')),
    dialog:cap(live('[role="dialog"],[role="alertdialog"]')),
    quickChatComposer:cap(live('textarea[data-avatar-overlay-composition-autofocus]')),editable:cap(editables)},
    mainScope:false,visibleDocument:document.visibilityState==='visible',focused:document.hasFocus(),
    spotlight:{failure:null,pending:false,inputCount:cap(inputs),listCount:cap(lists),editableCount:cap(editables)}};
  const body=document.body;
  const result={document,body,root:null,motion:null,command:null,input:null,list:null,scope};
  const fail=reason=>{scope.spotlight.failure=reason;return result;};
  if(held&&held.document!==document)return fail('scope-changed');
  if(held&&held.body!==body)return fail('scope-changed');
  if(scope.focused)return fail('auxiliary-focus');
  // Frozen webview/index.html (e5a9c64da5705de75d475147d59420902a293c4ec875f91507ba5bb8d6467525,
  // byte 10673) supplies BODY tabindex=0, outline:none as its document focus
  // anchor, before the lazy route mounts. It is an ancestor, not a palette
  // widget. gVs/LHi also set these finite font/theme variables on this body.
  // CSSOM expands outline into three longhands; validate its source semantics.
  // Any other tabindex node or editable/widget/overlay body still refuses.
  const sourceStyleNames=['outline-color','outline-style','outline-width',
    '--font-code-family','--font-ui-family','--codex-content-font-family','--font-ui-size',
    '--text-4xl','--text-3xl','--text-2xl','--text-xl','--text-lg','--text-base','--text-sm','--text-xs',
    '--text-dynamic-isle-row','--text-heading-lg','--text-heading-app','--text-heading-md','--text-heading-sm',
    '--cursor-interaction','--text-document'];
  if(!body||body.tagName!=='BODY'||!body.isConnected||body.parentElement!==document.documentElement
    ||body.getAttribute('tabindex')!=='0'||body.tabIndex!==0||body.isContentEditable
    ||body.getAttributeNames().length!==2||body.getAttributeNames().some(name=>!['tabindex','style'].includes(name)))return fail('foreign-controls');
  const inline=body.style;
  if(inline.getPropertyValue('outline')!=='none'
    ||[...inline].some(name=>!sourceStyleNames.includes(name)||inline.getPropertyPriority(name)!==''))return fail('foreign-controls');
  const controls=all('input,textarea,[contenteditable="true"],button,a[href],select,[tabindex],[role="button"],[role="option"],[role="combobox"],[role="listbox"]');
  const forbidden=all('iframe,frame,object,embed,textarea,[contenteditable="true"],[role="dialog"],[role="alertdialog"],[aria-modal="true"],[role="menu"]');
  const roots=all('.command-menu-dialog.global-command-menu-dialog');
  if(roots.length===0) {
    if(held?.root)return fail('scope-changed');
    if(controls.some(e=>e!==body)||forbidden.length)return fail('foreign-controls');
    scope.spotlight.pending=true;return result;
  }
  if(roots.length!==1)return fail('initial-scope');
  const root=roots[0],motions=[...root.children].filter(e=>e.hasAttribute('data-open'));
  const commands=all('[cmdk-root][data-command-menu-variant="standalone"]');
  if(motions.length!==1||commands.length!==1||inputs.length!==1||lists.length!==1)return fail('initial-scope');
  const motion=motions[0],command=commands[0],input=inputs[0],list=lists[0];
  Object.assign(result,{root,motion,command,input,list});
  if(![root,motion,command,input,list].every(e=>e.isConnected)||!body.contains(root)||!motion.contains(command)
    ||!command.contains(input)||!command.contains(list)||input.type!=='text')return fail('initial-scope');
  if(held?.root&&['root','motion','command','input','list'].some(key=>held[key]!==result[key]))return fail('scope-changed');
  // Frozen spotlight-page CSS closes this still-mounted interactive palette.
  // It can remain a visible native HWND for 500ms: this proves source closure,
  // never native hiding or affinity between Spotlight's commands and the main.
  if(motion.getAttribute('data-open')!=='false')return fail('source-open');
  const style=getComputedStyle(motion);
  if(Number(style.opacity)!==0||style.pointerEvents!=='none'
    ||motion.getAnimations().some(a=>a.playState==='running'||a.pending))return fail('source-transition');
  if(forbidden.length
    ||controls.some(e=>e!==body&&(!command.contains(e)||getComputedStyle(e).pointerEvents!=='none'))||editables.length!==1||editables[0]!==input
    ||['roleLegend','roleRadios','engineering','dialog','quickChatComposer'].some(key=>scope.counts[key]!==0))return fail('foreign-controls');
  // Hydration grants no exception to an observed open/transitioning palette.
  // Once its closed root exists, retain that tuple even while readiness waits.
  scope.spotlight.pending=document.readyState!=='complete';
  return result;
}
function create({deadline,now=Date.now}) {
  let held=null,identity=null,failed=null,closed=false,stableSamples=0;
  let counts={inputCount:null,listCount:null,editableCount:null};
  const fail=reason=>{failed??=reason;return false;};
  const timely=()=>!closed&&!failed&&(now()<deadline||fail('deadline'));
  const dispose=async handle=>{if(handle)await handle.dispose().catch(()=>{});};
  const bounded=async promise=>{
    let timer;
    try {return await Promise.race([promise,new Promise((_,reject)=>{
      timer=setTimeout(()=>{fail('deadline');reject(Error('deadline'));},Math.max(1,deadline-now()));
    })]);}finally{clearTimeout(timer);}
  };
  const read=async(page,current)=>{
    if(!timely())throw Error('spotlight');
    if(!route(current.url)||!route(current.frameUrl)||current.fragment!=='') {
      fail('identity');throw Error('spotlight');
    }
    if(identity&&(identity.page!==page||['url','target','frame','loader','frameUrl','fragment'].some(k=>identity[k]!==current[k]))) {
      fail('identity');throw Error('spotlight');
    }
    identity??={...current,page};
    let next;
    try {
      const pending=page.evaluateHandle(sample,held);
      pending.then(handle=>{if(closed||failed||now()>=deadline)void dispose(handle);},()=>{});
      next=await bounded(pending);
      if(!timely())throw Error('spotlight');
      const scope=await bounded(next.evaluate(value=>value.scope));
      if(!timely())throw Error('spotlight');
      counts=Object.fromEntries(Object.keys(counts).map(k=>[k,scope.spotlight[k]]));
      if(scope.spotlight.failure){fail(scope.spotlight.failure);throw Error('spotlight');}
      await dispose(held);held=next;next=null;
      return scope;
    } catch {fail(now()>=deadline?'deadline':'query');throw Error('spotlight');}
    finally {await dispose(next);}
  };
  return {read,fail,within:bounded,failed:()=>failed,commit:()=>{if(timely())stableSamples=Math.min(2,stableSamples+1);},
    receipt:()=>({sourceVersion:VERSION,rendererSha256:RENDERER,status:failed?'rejected':stableSamples===2?'verified':'rejected',
      failure:failed??(stableSamples===2?null:'initial-scope'),stableSamples,...counts}),
    close:async()=>{closed=true;await dispose(held);held=null;}};
}
module.exports={policy,route,sample,create,ARTIFACT,VERSION,RENDERER};
