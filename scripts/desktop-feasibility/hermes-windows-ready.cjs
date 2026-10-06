'use strict';
const { monitor } = require('./hermes-catalog-readiness.cjs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
// Standalone browser callback. Coordinates are returned privately to Node only.
function sample(element, diagnostic=false) {
  const result=(sampleStatus,blocker='unmeasured',point=null)=>
    diagnostic?{sampleStatus,blocker,point}:point;
  if (!element.isConnected || element.ownerDocument !== document || element.closest('[inert]')
      || element.matches(':disabled') || element.getAttribute('aria-disabled') === 'true') return result('guard-rejected');
  const r = element.getBoundingClientRect();
  if (!element.checkVisibility({contentVisibilityAuto:true,opacityProperty:true,visibilityProperty:true})
      || r.width <= 0 || r.height <= 0) return result('hidden');
  if (r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight) return result('outside-viewport');
  let blocker='other';
  for (const y of [.25,.5,.75]) for (const x of [.25,.5,.75]) {
    const px = element.clientWidth*x, py=element.clientHeight*y;
    const front=document.elementFromPoint(r.left+element.clientLeft+px,r.top+element.clientTop+py);
    if (front===element || element.contains(front)) return result('owned','none',
      {x:px,y:py,left:r.left,top:r.top,width:r.width,height:r.height});
    // Fixed source selector only; neither CSS classes nor app text escape.
    if (front?.closest?.('[data-glass-opaque][class~="z-(--z-onboarding)"]')) blocker='onboarding';
    else if (blocker!=='onboarding' && front?.closest?.('[role="dialog"], [role="alertdialog"], [data-slot="dialog-overlay"]')) blocker='modal';
  }
  return result('no-owned-point',blocker);
}
// Frozen model-catalog-menu renders the name and a nested reasoning/tag span
// inside one label. Compare only the name's direct text/HighlightMatches marks;
// no application text leaves this browser callback.
function modelRowLabel(element) {
  if (element.getAttribute('data-slot')!=='dropdown-menu-sub-trigger'
      || element.getAttribute('role')!=='menuitem') return false;
  const labels=element.querySelectorAll(':scope > span[class~="min-w-0"][class~="flex-1"][class~="truncate"]');
  if (labels.length!==1) return false;
  let name='';
  for (const child of labels[0].childNodes) {
    if (child.nodeType===3 || child.nodeType===1 && child.tagName==='MARK') name+=child.textContent;
    else if (child.nodeType!==1 || child.tagName!=='SPAN'
        || !child.classList.contains('text-(--ui-text-tertiary)')) return false;
  }
  return name==='Qwen3.6';
}
exports.run = async function run(page, session, ownedEndpoint, deadline, expectedProfile) {
  const facts={schemaVersion:1,mechanism:'hermes-windows-catalog-readiness',diagnosticsOnly:true,
    stage:'policy',errorCategory:'policy-rejected',menuOpened:false,refreshAttempted:false,
    catalogVerified:false,modelRowVerified:false,menuDismissed:false,composerReverified:false,
    composerObservation:null,guardFailure:'unmeasured'};
  if (process.platform!=='win32' || process.env.GITHUB_ACTIONS!=='true'
      || process.env.RUNNER_ENVIRONMENT!=='github-hosted' || process.env.RUNNER_OS!=='Windows'
      || process.env.FEASIBILITY_HERMES_READINESS_POLICY!=='current-catalog'
      || !Number.isFinite(deadline) || deadline<=Date.now() || deadline-Date.now()>120000) return facts;
  const initialUrl=page.url();
  let boundUrl=initialUrl, startup=true, transitioned=false;
  // Frozen main.ts loads the primary file without a query/fragment. Its
  // HashRouter fresh-draft action then replaces that route with exactly '/'.
  function rootUrl(value) {
    try {
      const url=new URL(value);
      if (value.endsWith('#')) return null;
      if (url.protocol!=='file:' || url.search!=='' || url.username || url.password
          || !['','#/'].includes(url.hash) || url.href!==value) return null;
      return url.hash==='#/' ? value.slice(0,-2) : value;
    } catch { return null; }
  }
  const base=rootUrl(initialUrl);
  function urlValid() {
    const current=page.url();
    if (base===null) return false;
    if (current===boundUrl) return true;
    if (startup && !transitioned && initialUrl===base && current===`${base}#/`) {
      transitioned=true; boundUrl=current; return true;
    }
    return false;
  }
  const guard=()=>{
    let query='ownership';
    try {
      const owned=Date.now()<deadline && ownedEndpoint();
      // The native proof can block; read page identity only after it returns.
      query='page-set';
      const pages=page.context().browser().contexts().flatMap(context=>context.pages());
      query='url';
      const failure = Date.now()>=deadline ? 'deadline-expired'
        : !owned ? 'ownership-lost'
        : !urlValid() ? 'url-changed'
        : pages.length!==1 ? 'page-count'
        : pages[0]!==page ? 'url-changed'
        : Date.now()>=deadline ? 'deadline-expired'
        : null;
      facts.guardFailure=failure;
      return failure===null;
    } catch {
      facts.guardFailure='query-failed';facts.queryFailure=query;
      return false;
    }
  };
  let evidence, documentIdentity;
  async function frame() {
    let pendingIdentity=null;
    for (;;) {
      if (!guard()) throw new Error('guard');
      let timer,query='frame-request';
      try {
        const reply=await Promise.race([session.send('Page.getFrameTree'), new Promise((_,reject)=>{
          timer=setTimeout(()=>reject(new Error('frame deadline')),Math.min(5000,Math.max(1,deadline-Date.now())));
        })]);
        query='frame-shape';
        const current=reply?.frameTree?.frame;
        if (!current) throw new Error('frame');
        query='frame-url';
        if (current.url!==base) throw new Error('frame');
        // CDP separates the URL fragment. The page and frame reads can straddle
        // the known first HashRouter transition; no input is admitted meanwhile.
        query='frame-fragment';
        const fragment=current.urlFragment===undefined?'':current.urlFragment;
        if (!['','#/'].includes(fragment)) throw new Error('frame');
        query='frame-identity';
        if (!['id','loaderId'].every(key=>typeof current[key]==='string'
            && current[key].length>0 && current[key].length<=256)) throw new Error('frame');
        const retained=documentIdentity||pendingIdentity;
        if (retained && (current.id!==retained.id || current.loaderId!==retained.loaderId)) {
          facts.guardFailure='url-changed';throw new Error('document changed');
        }
        if (!guard()) throw new Error('guard');
        query='frame-transition';
        if (current.url+fragment!==page.url()) {
          if (!startup) throw new Error('frame');
          pendingIdentity={id:current.id,loaderId:current.loaderId};
          facts.frameTransitionWaited=true;
          await delay(Math.min(100,Math.max(0,deadline-Date.now())));
          continue;
        }
        return {id:current.id,loaderId:current.loaderId};
      } catch {
        if (facts.guardFailure===null) {
          facts.guardFailure=Date.now()>=deadline?'deadline-expired':'query-failed';
          if (facts.guardFailure==='query-failed') facts.queryFailure=query;
        }
        throw new Error('frame');
      } finally { clearTimeout(timer); }
    }
  }
  try {
    documentIdentity=await frame();
    evidence=monitor(session,guard,'qwen3.6',expectedProfile);
    await session.send('Network.enable');
    facts.stage='composer'; facts.errorCategory='composer-unavailable';
    const roots=page.locator('[data-slot="composer-root"]:visible');
    const editor=roots.locator('[role="textbox"]:visible');
    const pill=roots.getByRole('button',{name:/^Model · [^\n]+: qwen3\.6$/,exact:true});
    const allPills=roots.getByRole('button',{name:/^Model · [^\n]+/,exact:true});
    const picker=roots.getByRole('button',{name:'Open model picker',exact:true});
    const switcher=roots.getByRole('button',{name:'Switch model',exact:true});
    async function observeComposer() {
      const values=await Promise.all([roots.count(),editor.count(),pill.count(),
        allPills.count(),picker.count(),switcher.count()]);
      const readyState=await page.evaluate(()=>document.readyState);
      if (values.some(value=>!Number.isInteger(value) || value<0 || value>64)
          || !['loading','interactive','complete'].includes(readyState)) {
        facts.guardFailure='query-failed';
        throw new Error('observation');
      }
      const [rootCount,editors,expectedModelPills,modelPills,pickerButtons,switchButtons]=values;
      facts.composerObservation={roots:rootCount,editors,expectedModelPills,modelPills,
        pickerButtons,switchButtons,readyState};
      return values.slice(0,3);
    }
    let original=null, originalRoot=null, stableUrl=null;
    while (guard()) {
      if (await page.locator('[role="dialog"]:visible, [role="alertdialog"]:visible').count()!==0) throw new Error('modal');
      const counts=await observeComposer();
      const complete=facts.composerObservation.readyState==='complete';
      if (counts.some(count=>count>1)) throw new Error('ambiguous composer');
      if (complete && counts.every(count=>count===1) && await pill.isEnabled()) {
        await frame();
        if (original && (!await editor.evaluate((element,held)=>element===held,original)
            || !await roots.evaluate((element,held)=>element===held,originalRoot))) throw new Error('composer replaced');
        if (original && stableUrl===page.url()) { startup=false; break; }
        if (!original) { original=await editor.elementHandle(); originalRoot=await roots.elementHandle(); }
        stableUrl=page.url();
      }
      await delay(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    if (!guard() || await roots.count()!==1 || await editor.count()!==1 || await pill.count()!==1) throw new Error('composer');
    if (startup || !original || !originalRoot) throw new Error('unstable composer');
    async function click(locator,action,additionalProof=async()=>true) {
      const observation={action,sampleStatus:'guard-rejected',blocker:'unmeasured'};
      facts.actionObservation=observation;
      await frame();
      if (!guard() || await roots.count()!==1 || await editor.count()!==1
          || !await editor.evaluate((element,held)=>element===held,original)
          || await page.locator('[role="dialog"]:visible, [role="alertdialog"]:visible').count()!==0
          || await locator.count()!==1 || !await locator.isEnabled()) throw new Error('control');
      const handle=await locator.elementHandle();
      const inspect=async()=>{
        if (!guard() || !await additionalProof()) {
          observation.sampleStatus='guard-rejected';return null;
        }
        const value=await handle.evaluate(sample,true);
        observation.sampleStatus=value.sampleStatus;observation.blocker=value.blocker;
        return value.point;
      };
      let last=null, previous=null, stableSamples=0;
      // Menu layout may settle after opening. Wait on the retained control;
      // replacement, loss of custody or an obscured hit point remain terminal.
      while (guard()) {
        await frame();
        if (await locator.count()!==1 || !await locator.evaluate((element,held)=>element===held,handle)) {
          observation.sampleStatus='control-replaced';
          // First-run configuration can dismiss this cover while it is read.
          // No click occurred. The caller must still prove cover absence and
          // the original composer; a replacement button is never adopted.
          if(action==='onboarding' && await locator.count()===0
              && await handle.evaluate(element=>!element.isConnected))return false;
          throw new Error('actionability');
        }
        const current=await inspect();
        if (!current) throw new Error('actionability');
        stableSamples=JSON.stringify(previous)===JSON.stringify(current)?stableSamples+1:1;
        previous=current;
        if (stableSamples===3) {last=current;break;}
        observation.sampleStatus='unstable';
        await delay(Math.min(100,Math.max(0,deadline-Date.now())));
      }
      if (!last || !guard()) {observation.sampleStatus='guard-rejected';throw new Error('guard');}
      try {
        await handle.click({position:{x:last.x,y:last.y},timeout:Math.min(5000,Math.max(1,deadline-Date.now()))});
      } catch {observation.sampleStatus='click-failed';throw new Error('action');}
      return true;
    }
    const pillHandle=await pill.elementHandle();
    const initialSample=await pillHandle.evaluate(sample,true);
    await pillHandle.dispose();
    if (initialSample.sampleStatus==='no-owned-point' && initialSample.blocker==='onboarding') {
      facts.stage='onboarding'; facts.errorCategory='onboarding-unavailable';
      facts.onboardingSkipped=false;
      const cover=page.locator('[data-glass-opaque][class~="z-(--z-onboarding)"]');
      const choice=cover.getByRole('button',{name:"I'll choose a provider later",exact:true});
      // Native custody checks share the original readiness budget; a cold
      // Windows process query must not consume a separate five-second clock.
      const settleDeadline=deadline;
      let choiceReady=false, coverGone=false;
      while (guard() && Date.now()<settleDeadline) {
        const covers=await cover.count(), choices=await choice.count();
        const observation={coverCount:Math.min(64,covers),choiceCount:Math.min(64,choices),
          coverVisible:null,choiceVisible:null,choiceEnabled:null};
        facts.onboardingObservation=observation;
        if (covers===0 && choices===0) {coverGone=true;break;}
        if (covers===0 && choices===1) {
          await delay(Math.min(100,Math.max(0,settleDeadline-Date.now())));
          continue; // The cover appeared between the two read-only queries.
        }
        if (covers!==1 || choices>1) throw new Error('onboarding');
        observation.coverVisible=await cover.isVisible();
        if (choices===1) {
          observation.choiceVisible=await choice.isVisible();
          observation.choiceEnabled=await choice.isEnabled();
        }
        if (choices===1 && observation.coverVisible && observation.choiceVisible && observation.choiceEnabled) {
          choiceReady=true; break;
        }
        await delay(Math.min(100,Math.max(0,settleDeadline-Date.now())));
      }
      if (coverGone) {
        // First-run setup can disappear while its controls are queried. No
        // click occurred; prove the original composer and its hit target afresh.
        await frame();
        if (!guard() || await cover.count()!==0 || await roots.count()!==1 || await editor.count()!==1
            || !await roots.evaluate((element,held)=>element===held,originalRoot)
            || !await editor.evaluate((element,held)=>element===held,original)
            || await pill.count()!==1 || !await pill.isEnabled()) throw new Error('onboarding');
        const current=await pill.elementHandle();
        try {
          if ((await current.evaluate(sample,true)).sampleStatus!=='owned') throw new Error('onboarding');
        } finally {await current.dispose();}
      } else {
        if (!choiceReady || !guard() || Date.now()>=settleDeadline || await cover.count()!==1 || !await cover.isVisible()
            || await choice.count()!==1 || !await choice.isEnabled()) throw new Error('onboarding');
        const heldCover=await cover.elementHandle();
        try {
          const coverProof=async()=>await cover.count()===1 && await cover.isVisible()
            && await cover.evaluate((element,held)=>element===held,heldCover)
            && await choice.count()===1 && await choice.isEnabled();
          // Frozen ChooseLaterLink only dismisses first-run provider selection.
          // It does not connect an account or change the managed provider.
          const dispatched=await click(choice,'onboarding',coverProof);
          while (guard() && Date.now()<settleDeadline && await cover.count()!==0) await delay(20);
          if (!guard() || await cover.count()!==0 || await roots.count()!==1 || await editor.count()!==1
              || !await roots.evaluate((element,held)=>element===held,originalRoot)
              || !await editor.evaluate((element,held)=>element===held,original)) throw new Error('onboarding');
          await frame();
          const counts=await observeComposer();
          if (counts.some(count=>count!==1) || facts.composerObservation.readyState!=='complete') throw new Error('composer');
          facts.onboardingSkipped=dispatched;
        } finally { await heldCover.dispose(); }
      }
    }
    facts.stage='menu'; facts.errorCategory='menu-unavailable';
    await click(pill,'menu'); facts.menuOpened=true;
    const menu=page.getByRole('menu').filter({has:page.getByRole('menuitem',{name:'Refresh models',exact:true})});
    if (!guard() || await menu.count()!==1) throw new Error('menu');
    // An explicit ordinary refresh, armed only after this unique menu opened,
    // avoids treating startup traffic or a cached row as fresh readiness.
    facts.stage='refresh'; facts.errorCategory='refresh-uncertain';
    evidence.arm(); facts.refreshAttempted=true;
    await click(menu.getByRole('menuitem',{name:'Refresh models',exact:true}),'refresh');
    facts.stage='catalog'; facts.errorCategory='catalog-unavailable';
    while (guard() && !evidence.verified()) await delay(Math.min(100,Math.max(0,deadline-Date.now())));
    if (!evidence.verified()) throw new Error('catalog');
    facts.catalogVerified=true;
    const row=menu.getByRole('menuitem').filter({has:page.locator(
      'span[class~="min-w-0"][class~="flex-1"][class~="truncate"]'
    ).filter({hasText:/^Qwen3\.6(?:\s|$)/})});
    // A fresh wire acknowledgement precedes React's query-driven row update.
    // Wait read-only within the original deadline; never refresh or select twice.
    let rowReady=false;
    while (guard()) {
      await frame();
      if (await menu.count()!==1) throw new Error('menu');
      const count=await row.count();
      if (count>1) throw new Error('ambiguous row');
      if (count===1) {
        if (!await row.evaluate(modelRowLabel)) throw new Error('row label');
        if (await row.isVisible() && await row.isEnabled()) {rowReady=true;break;}
      }
      await delay(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    if (!rowReady || !guard()) throw new Error('row');
    facts.modelRowVerified=true;
    facts.stage='dismiss'; facts.errorCategory='dismiss-uncertain';
    if (!guard() || await menu.count()!==1
        || await page.locator('[role="dialog"]:visible, [role="alertdialog"]:visible').count()!==0) throw new Error('menu');
    await frame();
    await page.keyboard.press('Escape');
    while (guard() && await menu.count()!==0) await delay(100);
    if (!guard() || await menu.count()!==0) throw new Error('dismiss');
    facts.menuDismissed=true;
    facts.stage='composer'; facts.errorCategory='composer-changed';
    if (await roots.count()!==1 || await editor.count()!==1 || await pill.count()!==1
        || !await editor.evaluate((element,held)=>element===held,original) || !guard() || !evidence.verified()) throw new Error('composer');
    await frame();
    if (!await roots.evaluate((element,held)=>element===held,originalRoot)) throw new Error('composer');
    const finalCounts=await observeComposer();
    if (!guard() || finalCounts.some(count=>count!==1)
        || facts.composerObservation.readyState!=='complete') throw new Error('composer');
    facts.composerReverified=true; facts.stage='ready'; facts.errorCategory=null;
  } catch { /* Closed stage/category only; never retain raw error or app text. */ }
  finally { evidence?.dispose(); }
  return facts;
};
exports.sample=sample;
exports.modelRowLabel=modelRowLabel;
