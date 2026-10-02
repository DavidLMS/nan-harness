'use strict';
const { monitor } = require('./hermes-catalog-readiness.cjs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
// Standalone browser callback. Coordinates are returned privately to Node only.
function sample(element) {
  if (!element.isConnected || element.ownerDocument !== document || element.closest('[inert]')
      || element.matches(':disabled') || element.getAttribute('aria-disabled') === 'true') return null;
  const r = element.getBoundingClientRect();
  if (!element.checkVisibility({contentVisibilityAuto:true,opacityProperty:true,visibilityProperty:true})
      || r.width <= 0 || r.height <= 0 || r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight) return null;
  for (const y of [.25,.5,.75]) for (const x of [.25,.5,.75]) {
    const px = element.clientWidth*x, py=element.clientHeight*y;
    const front=document.elementFromPoint(r.left+element.clientLeft+px,r.top+element.clientTop+py);
    if (front===element || element.contains(front)) return {x:px,y:py,left:r.left,top:r.top,width:r.width,height:r.height};
  }
  return null;
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
  const guard=()=>{
    try {
      const failure = Date.now()>=deadline ? 'deadline-expired'
        : !ownedEndpoint() ? 'ownership-lost'
        : page.url()!==initialUrl ? 'url-changed'
        : page.context().browser().contexts().flatMap(context=>context.pages()).length!==1 ? 'page-count'
        : Date.now()>=deadline ? 'deadline-expired'
        : null;
      facts.guardFailure=failure;
      return failure===null;
    } catch {
      facts.guardFailure='query-failed';
      return false;
    }
  };
  let evidence;
  try {
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
    while (guard()) {
      if (await page.locator('[role="dialog"]:visible, [role="alertdialog"]:visible').count()!==0) throw new Error('modal');
      const counts=await observeComposer();
      const complete=facts.composerObservation.readyState==='complete';
      if (counts.some(count=>count>1)) throw new Error('ambiguous composer');
      if (complete && counts.every(count=>count===1) && await pill.isEnabled()) break;
      await delay(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    if (!guard() || await roots.count()!==1 || await editor.count()!==1 || await pill.count()!==1) throw new Error('composer');
    const original=await editor.elementHandle();
    async function click(locator) {
      if (!guard() || await roots.count()!==1 || await editor.count()!==1
          || !await editor.evaluate((element,held)=>element===held,original)
          || await page.locator('[role="dialog"]:visible, [role="alertdialog"]:visible').count()!==0
          || await locator.count()!==1 || !await locator.isEnabled()) throw new Error('control');
      const handle=await locator.elementHandle();
      const first=await handle.evaluate(sample); await delay(Math.min(100,Math.max(0,deadline-Date.now())));
      const second=await handle.evaluate(sample);
      if (!guard() || !first || !second || JSON.stringify(first)!==JSON.stringify(second)
          || await locator.count()!==1 || !await locator.evaluate((element,held)=>element===held,handle)) throw new Error('actionability');
      const last=await handle.evaluate(sample);
      if (!last || JSON.stringify(second)!==JSON.stringify(last) || !guard()) throw new Error('actionability');
      await handle.click({position:{x:last.x,y:last.y},timeout:Math.min(5000,Math.max(1,deadline-Date.now()))});
    }
    facts.stage='menu'; facts.errorCategory='menu-unavailable';
    await click(pill); facts.menuOpened=true;
    const menu=page.getByRole('menu').filter({has:page.getByRole('menuitem',{name:'Refresh models',exact:true})});
    if (!guard() || await menu.count()!==1) throw new Error('menu');
    // An explicit ordinary refresh, armed only after this unique menu opened,
    // avoids treating startup traffic or a cached row as fresh readiness.
    facts.stage='refresh'; facts.errorCategory='refresh-uncertain';
    evidence.arm(); facts.refreshAttempted=true;
    await click(menu.getByRole('menuitem',{name:'Refresh models',exact:true}));
    facts.stage='catalog'; facts.errorCategory='catalog-unavailable';
    while (guard() && !evidence.verified()) await delay(Math.min(100,Math.max(0,deadline-Date.now())));
    if (!evidence.verified()) throw new Error('catalog');
    facts.catalogVerified=true;
    const row=menu.getByRole('menuitem').filter({has:page.locator('span').filter({hasText:/^Qwen3\.6$/})});
    if (!guard() || await row.count()!==1 || !await row.isEnabled()) throw new Error('row');
    facts.modelRowVerified=true;
    facts.stage='dismiss'; facts.errorCategory='dismiss-uncertain';
    if (!guard() || await menu.count()!==1
        || await page.locator('[role="dialog"]:visible, [role="alertdialog"]:visible').count()!==0) throw new Error('menu');
    await page.keyboard.press('Escape');
    while (guard() && await menu.count()!==0) await delay(100);
    if (!guard() || await menu.count()!==0) throw new Error('dismiss');
    facts.menuDismissed=true;
    facts.stage='composer'; facts.errorCategory='composer-changed';
    if (await roots.count()!==1 || await editor.count()!==1 || await pill.count()!==1
        || !await editor.evaluate((element,held)=>element===held,original) || !guard() || !evidence.verified()) throw new Error('composer');
    const finalCounts=await observeComposer();
    if (!guard() || finalCounts.some(count=>count!==1)
        || facts.composerObservation.readyState!=='complete') throw new Error('composer');
    facts.composerReverified=true; facts.stage='ready'; facts.errorCategory=null;
  } catch { /* Closed stage/category only; never retain raw error or app text. */ }
  finally { evidence?.dispose(); }
  return facts;
};
exports.sample=sample;
