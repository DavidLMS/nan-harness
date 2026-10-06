'use strict';
// Frozen Mac 26.930.41038; passive home/state/menu witness, never input.
function capture() {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  const homes=[...document.querySelectorAll('[data-codex-composer-root][data-composer-placement="home"]')].filter(visible);
  if(homes.length!==1)return null;
  const pages=[...document.querySelectorAll('[data-testid="chatgpt-work-home-page"]')].filter(visible);
  if(pages.length!==1||!pages[0].contains(homes[0]))return null;
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
  const pages=[...document.querySelectorAll('[data-testid="chatgpt-work-home-page"]')].filter(visible);
  if(pages.length!==1||!pages[0].contains(held.home))return blocked;
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

// Only the retained-root filesystem read is synchronous inside this boundary.
// Native ownership and original document/editor proofs stay outside the read.
async function custodyPair(authority,prove) {
  let before=false;try{before=await prove();}catch{}
  if(before!==true)return {queried:false,pair:null};
  let pair=null;
  try{pair=authority.snapshotPair();}catch{}
  let after=false;try{after=await prove();}catch{}
  return {queried:true,pair:after===true&&pair&&typeof pair.then!=='function'?pair:null};
}

async function observe({page,alive,deadline,loan,workspace,ownerGuard,openMenu=false},
  {makeAuthority=require('./codex-profile-state.cjs').authority,
   project=require('./codex-profile-state.cjs').project,
   makeContext=require('./codex-context-session.cjs').create,
   selected=require('./codex-macos-selected-project.cjs').sample}={}) {
  const facts={status:'blocked',diagnosticsOnly:true,homeRetained:false,stateQueried:false,
    statePairStable:false,ordinaryLocalProjectObserved:false,selectedIdCorrelated:false,
    menuClickAttempted:false,menuClickCompleted:false,inputAuthorized:false,sendAuthorized:false};
  let held,button,menu,context,a;
  const live=async()=>Date.now()<deadline&&await alive()===true&&Date.now()<deadline;
  const ownedCustody=()=>Date.now()<deadline&&ownerGuard()===true&&a.verify()
    &&ownerGuard()===true&&Date.now()<deadline;
  try {
    if(!loan||loan.platform!=='macos'||workspace!==loan.directories?.[0]?.path||!await live())return {...facts,reason:'custody'};
    if(typeof ownerGuard!=='function'||ownerGuard()!==true)return {...facts,reason:'custody'};
    // The original prelaunch root handles prove filesystem custody independently.
    // Never run listener subprocesses once per root or filesystem verification.
    a=makeAuthority(loan,deadline,()=>Date.now()<deadline);
    if(ownerGuard()!==true)return {...facts,reason:'custody'};
    if(!a||!ownedCustody())return {...facts,reason:'custody'};
    held=await page.evaluateHandle(capture);
    const first=await held.evaluate(sample,{opened:false,menu:null});
    const second=await held.evaluate(sample,{opened:false,menu:null});
    if(!first.matched||!second.matched||JSON.stringify(first.rect)!==JSON.stringify(second.rect)||!await live()||!ownedCustody())return {...facts,reason:'home'};
    context=makeContext({page,alive:live,deadline,pwProof:async()=>!!held&&await live()&&ownedCustody()
      &&(await held.evaluate(sample,{opened:!!menu,menu:menu??null})).matched===true});
    const prepared=await context.prepare(held);
    if(prepared.verified!==true||!await context.verifyHeld(held)||!ownedCustody())return {...facts,reason:'document'};
    facts.homeRetained=true;
    const proveStateBoundary=async()=>Date.now()<deadline&&ownerGuard()===true&&a.verify()
      &&await context.verifyHeld(held)&&a.verify()&&ownerGuard()===true&&Date.now()<deadline;
    const firstRead=await custodyPair(a,proveStateBoundary);
    facts.stateQueried=firstRead.queried;
    const before=firstRead.pair;
    if(!before)return {...facts,reason:'state'};
    facts.statePairStable=true;
    const candidate=project(before.first.value,workspace);
    if(!candidate)return {...facts,reason:'project'};
    facts.ordinaryLocalProjectObserved=true;
    if(openMenu) {
      button=(await held.getProperty('button')).asElement();
      const last=await held.evaluate(sample,{opened:false,menu:null});
      if(!button||!last.matched||JSON.stringify(first.rect)!==JSON.stringify(last.rect)||!await context.verifyHeld(held)||!ownedCustody())return {...facts,reason:'control'};
      facts.menuClickAttempted=true;
      await button.click({position:{x:first.rect[2]/2,y:first.rect[3]/2},timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
      facts.menuClickCompleted=true;
      const menus=page.locator('[cmdk-root]:visible');
      if(!await live()||!ownedCustody()||await menus.count()!==1)return {...facts,reason:'menu'};
      menu=await menus.elementHandle();
      if(!menu||!await context.verifyHeld(held))return {...facts,reason:'document'};
      for(let n=0;n<2;n++) {
        if(!ownedCustody()||!await context.verifyHeld(held))return {...facts,reason:'document'};
        const observed=await page.evaluate(selected,{menu,projectId:candidate.projectId});
        if(observed.status!=='observed'||observed.selectedIdCorrelated!==true)return {...facts,reason:'selected-id'};
        if(!ownedCustody()||!await context.verifyHeld(held))return {...facts,reason:'document'};
      }
      facts.selectedIdCorrelated=true;
    }
    const after=(await custodyPair(a,proveStateBoundary)).pair;
    if(!after||before.first.digest!==after.second.digest||!['dev','ino','uid','mode','nlink','size','mtimeNs','ctimeNs'].every(key=>typeof before.first.identity[key]==='bigint'&&before.first.identity[key]===after.second.identity[key])
      ||!ownedCustody()||!await context.verifyHeld(held))return {...facts,statePairStable:false,selectedIdCorrelated:false,reason:'state-changed'};
    return {...facts,status:'observed',reason:openMenu?'menu-correlated':'state-observed'};
  }catch{return {...facts,reason:Date.now()>=deadline?'deadline':'query'};}
  finally {
    if(context)await context.close();
    for(const handle of [menu,button,held])if(handle)try{await handle.dispose();}catch{}
    if(a)a.close();
  }
}
exports.observe=observe;

exports.custodyPair=custodyPair;
