'use strict';
// Use the frozen ordinary workspace selector; selection is a bounded opt-in
// transition. This observation never grants Send; project IDs remain private.
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
async function run(page,guard,ownerGuard,deadline,loan,workspace,selectionPolicy=null) {
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
    let selectionConsumed=false;
    const transition=selectionPolicy?.frozenLinuxTrial===true&&Object.keys(selectionPolicy).length===1?async(selected,custody)=>{
      if(selectionConsumed||!custody()||!await alive()||!(await held.evaluate(sample,{opened:true,menu})).matched)return null;
      if(prepared.verified!==true||!await context.verifyHeld(held))return null;
      const retainedGuard=async()=>custody()&&await alive()&&await context.verifyHeld(held)&&custody()&&await alive();
      selectionConsumed=true;
      const choice=require('./codex-project-choice.cjs');let retained,item;
      try{
        retained=await page.evaluateHandle(choice.capture,{menu,projectId:selected.projectId});
        if(!await retainedGuard())return null;
        const a=await retained.evaluate(choice.sample),b=await retained.evaluate(choice.sample);
        if(!a.matched||!b.matched||JSON.stringify(a.rect)!==JSON.stringify(b.rect)||!await retainedGuard())return null;
        item=(await retained.getProperty('item')).asElement();if(!item)return null;
        const c=await retained.evaluate(choice.sample);
        if(!c.matched||JSON.stringify(a.rect)!==JSON.stringify(c.rect)||!await retainedGuard()
          ||!(await held.evaluate(sample,{opened:true,menu})).matched||!await retainedGuard())return null;
        const selection=await page.evaluate(require('./codex-selected-project.cjs').sample,{menu,projectId:selected.projectId});
        if(selection.reason!=='selected-id'||selection.selectedItemCount!==0||selection.matchingItemCount!==1||!await retainedGuard())return null;
        facts.selectionClickAttempted=true;facts.selectionStage='item-click';
        await item.click({position:{x:a.rect[2]/2,y:a.rect[3]/2},timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
        facts.selectionStage='original-popup-close';
        if(!custody()||!await alive())return null;
        // No popup substitute: the original popup must close in this document.
        const closeCutoff=Math.min(deadline,Date.now()+2000);
        let closed=false;
        while(Date.now()<closeCutoff){
          if(!custody()||!await alive()){facts.selectionFailure='deadline-or-owner';return null;}
          if(!await context.verifyRetainedDocument()){facts.selectionFailure=context.retainedDocumentFailure();return null;}
          const state=await held.evaluate(sample,{opened:false,menu:null});
          if(!custody()||!await alive()){facts.selectionFailure='deadline-or-owner';return null;}
          if(!await context.verifyRetainedDocument()){facts.selectionFailure=context.retainedDocumentFailure();return null;}
          if(state.matched){closed=true;break;}
          await new Promise(resolve=>setTimeout(resolve,Math.min(50,Math.max(0,closeCutoff-Date.now()))));
        }
        if(!closed){facts.selectionFailure='source-close-unproved';return null;}
        await menu.dispose();menu=null;
        facts.selectionStage='closed-source';
        const closedA=await held.evaluate(sample,{opened:false,menu:null}),closedB=await held.evaluate(sample,{opened:false,menu:null});
        if(!closedA.matched||!closedB.matched||JSON.stringify(closedA.rect)!==JSON.stringify(closedB.rect)||!await retainedGuard())return null;
        const final=await held.evaluate(sample,{opened:false,menu:null});
        if(!final.matched||JSON.stringify(closedA.rect)!==JSON.stringify(final.rect)||!await retainedGuard())return null;
        facts.selectionStage='reopen-click';
        await button.click({position:{x:final.rect[2]/2,y:final.rect[3]/2},timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
        if(!custody()||!await alive())return null;
        facts.selectionStage='reopened-popup';
        const reopened=page.locator('[cmdk-root]:visible');if(await reopened.count()!==1)return null;
        menu=await reopened.elementHandle();
        if(!menu||!custody()||!await alive()||!(await held.evaluate(sample,{opened:true,menu})).matched||!await retainedGuard())return null;
        facts.selectionClickCompleted=true;facts.selectionStage='completed';
        return {completed:true,menu};
      }finally{for(const h of [item,retained])if(h)try{await h.dispose();}catch{}}
    }:null;
    const observation=await require('./codex-profile-state.cjs').observe(page,ownerGuard,deadline,loan,workspace,menu,alive,async selected=>prepared.verified?context.observe(held,selected.projectId,selected.workspace):prepared,transition);
    if(!await alive()||!(await held.evaluate(sample,{opened:true,menu})).matched||!await alive())return {...facts,reason:'guard'};
    return {...facts,status:'observed',profileStateObservation:observation};
  } catch {return {...facts,reason:Date.now()>=deadline?'deadline':facts.clickAttempted?'action-uncertain':'query'};}
  finally {if(context)await context.close();for(const handle of [menu,button,held])if(handle)try{await handle.dispose();}catch{}}
}
exports.capture=capture;exports.sample=sample;exports.run=run;
