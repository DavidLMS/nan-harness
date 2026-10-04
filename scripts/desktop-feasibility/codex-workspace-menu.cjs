'use strict';
// Open only the frozen source's ordinary workspace selector. Observe, never
// select an item or grant Send; IDs and profile paths remain private.
function capture() {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  const homes=[...document.querySelectorAll('[data-codex-composer-root][data-composer-placement="home"]')].filter(visible);
  if(homes.length!==1)return null;
  const home=homes[0],editors=[...home.querySelectorAll('.ProseMirror[contenteditable="true"]')].filter(visible);
  const controls=[...home.querySelectorAll('[data-composer-navigation-target="workspace-project"]')].filter(visible);
  if(editors.length!==1||controls.length!==1)return null;
  const button=controls[0].closest('button');
  return button&&button===controls[0]&&home.contains(button)?{home,editor:editors[0],control:controls[0],button}:null;
}
function sample(held,{opened,menu}) {
  const blocked={matched:false};
  const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  if(!held||!visible(held.home)||!visible(held.editor)||!visible(held.control)||!visible(held.button)
      ||held.home.getAttribute('data-codex-composer-root')===null||held.home.getAttribute('data-composer-placement')!=='home'
      ||held.editor.getAttribute('contenteditable')!=='true'||!held.editor.classList.contains('ProseMirror')
      ||held.control.getAttribute('data-composer-navigation-target')!=='workspace-project'
      ||held.control!==held.button||held.control.closest('button')!==held.button
      ||held.button.getAttribute('data-slot')!=='popover-trigger'||held.button.getAttribute('aria-haspopup')!=='dialog'||held.button.disabled||held.button.getAttribute('aria-disabled')==='true'
      ||!held.home.contains(held.editor)||!held.home.contains(held.button))return blocked;
  const homes=[...document.querySelectorAll('[data-codex-composer-root][data-composer-placement="home"]')].filter(visible);
  const editors=[...held.home.querySelectorAll('.ProseMirror[contenteditable="true"]')].filter(visible);
  const controls=[...held.home.querySelectorAll('[data-composer-navigation-target="workspace-project"]')].filter(visible);
  if(homes.length!==1||homes[0]!==held.home||editors.length!==1||editors[0]!==held.editor||controls.length!==1||controls[0]!==held.control)return blocked;
  const menus=[...document.querySelectorAll('[cmdk-root]')].filter(visible);
  const overlays=[...document.querySelectorAll('[role="dialog"],[role="alertdialog"],[aria-modal="true"],[role="menu"]')].filter(visible);
  if(!opened) {
    if(menus.length||overlays.length||held.button.getAttribute('aria-expanded')==='true')return blocked;
    const r=held.button.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
    if(![r.left,r.top,r.width,r.height,x,y].every(Number.isFinite)||r.width<=0||r.height<=0
        ||x<0||y<0||x>=innerWidth||y>=innerHeight||!held.button.contains(document.elementFromPoint(x,y)))return blocked;
    return {matched:true,rect:[r.left,r.top,r.width,r.height]};
  }
  if(menus.length!==1||menus[0]!==menu||!visible(menu)||held.button.getAttribute('aria-expanded')!=='true')return blocked;
  const id=held.button.getAttribute('aria-controls'),popup=menu.closest('[role="dialog"][data-slot="popover-content"]');
  if(!id||!popup||popup.id!==id||overlays.some(e=>e!==popup&&!popup.contains(e)))return blocked;
  return {matched:true};
}
async function run(page,guard,ownerGuard,deadline,loan,workspace) {
  const facts={status:'blocked',diagnosticsOnly:true,clickAttempted:false,clickCompleted:false,sendAuthorized:false};
  const alive=async()=>Date.now()<deadline&&ownerGuard()===true&&await guard()===true&&Date.now()<deadline;
  let held,button,menu,context,prepared;
  try {
    if(!await alive())return {...facts,reason:'guard'};
    held=await page.evaluateHandle(capture);
    if(!await alive())return {...facts,reason:'guard'};
    const first=await held.evaluate(sample,{opened:false,menu:null});
    if(!first.matched||!await alive())return {...facts,reason:'control'};
    const second=await held.evaluate(sample,{opened:false,menu:null});
    if(!second.matched||JSON.stringify(first.rect)!==JSON.stringify(second.rect)||!await alive())return {...facts,reason:'control'};
    button=(await held.getProperty('button')).asElement();
    if(!button)return {...facts,reason:'control'};
    const final=await held.evaluate(sample,{opened:false,menu:null});
    if(!final.matched||JSON.stringify(first.rect)!==JSON.stringify(final.rect)||!await alive())return {...facts,reason:'control'};
    context=require('./codex-context-session.cjs').create({page,alive,deadline,
      pwProof:async()=>!!held&&await alive()&&(await held.evaluate(sample,{opened:!!menu,menu:menu??null})).matched===true&&await alive()});
    prepared=await context.prepare(held);
    if(!await alive())return {...facts,reason:'guard'};
    const afterPreparation=await held.evaluate(sample,{opened:false,menu:null});
    if(!afterPreparation.matched||JSON.stringify(first.rect)!==JSON.stringify(afterPreparation.rect)||!await alive())return {...facts,reason:'control'};
    facts.clickAttempted=true;
    await button.click({position:{x:first.rect[2]/2,y:first.rect[3]/2},timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
    facts.clickCompleted=true;
    if(!await alive())return {...facts,reason:'guard'};
    const menus=page.locator('[cmdk-root]:visible');
    if(await menus.count()!==1)return {...facts,reason:'menu'};
    menu=await menus.elementHandle();
    if(!menu||!await alive()||!(await held.evaluate(sample,{opened:true,menu})).matched||!await alive())return {...facts,reason:'menu'};
    const observation=await require('./codex-profile-state.cjs').observe(page,ownerGuard,deadline,loan,workspace,menu,alive,async selected=>prepared.verified?context.observe(held,selected.projectId,selected.workspace):prepared);
    if(!await alive()||!(await held.evaluate(sample,{opened:true,menu})).matched||!await alive())return {...facts,reason:'guard'};
    return {...facts,status:'observed',profileStateObservation:observation};
  } catch {return {...facts,reason:Date.now()>=deadline?'deadline':facts.clickAttempted?'action-uncertain':'query'};}
  finally {if(context)await context.close();for(const handle of [menu,button,held])if(handle)try{await handle.dispose();}catch{}}
}
exports.capture=capture;exports.sample=sample;exports.run=run;
