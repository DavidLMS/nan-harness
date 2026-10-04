// Read-only owned renderer inventory. No UI text, HTML, URLs or paths are emitted.
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(path.resolve(__dirname, '../../.github/web-check/node_modules/playwright'));
const request = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const connection = JSON.parse(fs.readFileSync(request.connectionPath, 'utf8'));
const output = process.argv[4];
const app = process.env.NANH_DESKTOP_RENDERER_APP;
const facts = { schemaVersion: 1, mechanism: 'renderer-inventory', diagnosticsOnly: true,
  app, endpointOwned: false, launcherOwned: false, attached: false, pageCount: 0,
  textareaCount: 0, editableCount: 0, sendCount: 0, retryCount: 0,
  newThreadCount: 0, loginCount: 0, dialogCount: 0, documentState: { readyState: 'unobserved', targetKind: 'unobserved', bodyPresent: false, elementCount: 0, visibleElementCount: 0, inputCount: 0, frameCount: 0, pageErrorCount: 0 }, errorCategory: 'unclassified' };
facts.startupScreen = 'unmeasured';
function save() {
  fs.writeFileSync(`${output}.tmp`, JSON.stringify(facts) + '\n', { mode: 0o600 });
  fs.renameSync(`${output}.tmp`, output);
}
function onboardingTrial(appName, platform, env) {
  return appName === 'chatgpt-desktop' && ['win32','linux','darwin'].includes(platform)
    && env.GITHUB_ACTIONS === 'true' && env.RUNNER_ENVIRONMENT === 'github-hosted'
    && env.RUNNER_OS === ({win32:'Windows',linux:'Linux',darwin:'macOS'}[platform]) && env.NANH_CODEX_PUBLIC_ONBOARDING === 'engineering';
}
function onboardingDeadline(trial, startupDeadline, totalDeadline) {
  // Trust, binding and onboarding share the original clock; actions never
  // allocate another 25 seconds or truncate unused trial time.
  return trial ? totalDeadline : startupDeadline;
}
function onboardingBudget(trial, platform) {
  return trial ? platform==='win32'?120000:60000 : 25000;
}
// This passive receipt never relaxes the page-count guard or sends input.
function correlationFacts() {
  return {schemaVersion:1,mechanism:'codex-main-aux-correlation',diagnosticsOnly:true,
    status:'initial-main-unavailable',totalPages:null,stableSamples:0,
    heldMainUnchanged:false,auxRouteMatched:false,mainScopeUnique:false,
    auxMainControlsAbsent:false,auxComposerAbsent:false,guarded:false,
    mainDocumentFocused:null,auxDocumentFocused:null,main:null,aux:null};
}
function correlationScope() {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const all=selector=>[...document.querySelectorAll(selector)].filter(visible);
  const radios=all('input[type="radio"][name="conversational-onboarding-inline-role"]');
  const legends=all('fieldset > legend').filter(e=>e.innerText.trim()==='Select the kind of work you do');
  const engineering=radios.filter(e=>e.value==='engineering');
  const dialogs=all('[role="dialog"],[role="alertdialog"]');
  const tokens=['relative','flex','h-full','min-h-0','w-full','flex-col','bg-transparent','tracking-normal','text-default','select-text'];
  const fieldset=legends.length===1?legends[0].parentElement:null;
  const roots=all('div'+tokens.map(t=>`[class~="${t}"]`).join(''));
  const scope=roots.length===1?roots[0]:null;
  // The source role page is an ordinary div. Foreign dialogs govern input
  // separately; their presence or containment cannot establish page identity.
  const mainScope=!!scope&&radios.length===11&&engineering.length===1
    &&scope.contains(fieldset)&&radios.every(e=>fieldset.contains(e))
    &&engineering[0].labels?.length===1
    &&engineering[0].labels[0].innerText.trim()==='Engineering';
  const cap=items=>Math.min(4096,items.length);
  return {counts:{roleLegend:cap(legends),roleRadios:cap(radios),engineering:cap(engineering),
      dialog:cap(dialogs),quickChatComposer:cap(all('textarea[data-avatar-overlay-composition-autofocus]')),
      editable:cap(all('textarea,[contenteditable="true"],input:not([type="radio"]):not([type="checkbox"]):not([type="hidden"])'))},
    mainScope,focused:document.hasFocus()};
}
async function correlationIdentity(page, deadline, includeScope=true, diagnostic=null) {
  let session;
  const bounded=async promise=>{
    const remaining=deadline-Date.now();if(remaining<=0)throw new Error('deadline');
    let timer;try{return await Promise.race([promise,new Promise((_,reject)=>{
      timer=setTimeout(()=>reject(new Error('deadline')),remaining);})]);}finally{clearTimeout(timer);}
  };
  try {
    session=await bounded(page.context().newCDPSession(page));
    const target=await bounded(session.send('Target.getTargetInfo'));
    const tree=await bounded(session.send('Page.getFrameTree'));
    const frame=tree.frameTree?.frame;
    if(diagnostic) {
      diagnostic.targetPresent=typeof target.targetInfo?.targetId==='string'&&target.targetInfo.targetId.length>0;
      diagnostic.framePresent=typeof frame?.id==='string'&&frame.id.length>0;
      diagnostic.loaderPresent=typeof frame?.loaderId==='string'&&frame.loaderId.length>0;
    }
    if(typeof target.targetInfo?.targetId!=='string'||!target.targetInfo.targetId
      ||!frame||typeof frame.id!=='string'||!frame.id||typeof frame.url!=='string'
      ||typeof frame.loaderId!=='string'||!frame.loaderId)throw new Error('identity');
    const scope=includeScope?await bounded(page.evaluate(correlationScope)):null;
    return {page,url:page.url(),target:target.targetInfo.targetId,frame:frame.id,
      loader:frame.loaderId,frameUrl:frame.url,fragment:frame.urlFragment??'',scope};
  } finally {if(session)await session.detach().catch(()=>{});}
}
function sameCorrelationIdentity(a,b) {
  return a.page===b.page&&a.url===b.url&&a.target===b.target&&a.frame===b.frame
    &&a.loader===b.loader&&a.frameUrl===b.frameUrl&&a.fragment===b.fragment;
}
function officialInitialMain(identity) {
  return typeof identity.target==='string'&&identity.target.length>0
    &&typeof identity.frame==='string'&&identity.frame.length>0
    &&typeof identity.loader==='string'&&identity.loader.length>0
    &&identity.url==='app://-/index.html'&&identity.frameUrl===identity.url&&identity.fragment==='';
}
function initialMainFacts() {
  return {status:'unmeasured',route:'unmeasured',targetPresent:false,framePresent:false,loaderPresent:false};
}
function initialMainRoute(url) {
  if(url===''||url==='about:blank')return 'blank';
  try {
    const parsed=new URL(url);
    if(parsed.protocol!=='app:')return 'other';
    if(parsed.host!=='-'||parsed.pathname!=='/index.html')return 'other-app';
    if(parsed.hash)return 'primary-fragment';
    return parsed.search?'primary-query':'primary';
  } catch{return 'other';}
}
async function captureCorrelationMain(page,browser,guard,deadline,identity=correlationIdentity,
  pause=ms=>new Promise(resolve=>setTimeout(resolve,ms)),diagnostic=null) {
  const stop=status=>{if(diagnostic)diagnostic.status=status;return null;};
  try {
    const commitDeadline=Math.min(deadline,Date.now()+10000);
    let held=null;
    // The owned sole target can exist before Electron commits its first loader.
    // Wait only before binding; once bound, a reload never grants a new identity.
    while(Date.now()<commitDeadline&&guard()) {
      const pages=browser.contexts().flatMap(c=>c.pages());
      if(pages.length!==1||pages[0]!==page)return stop('page-count');
      const url=page.url();
      if(diagnostic)diagnostic.route=initialMainRoute(url);
      if(url!==''&&url!=='about:blank'&&url!=='app://-/index.html')return stop('route-rejected');
      if(url==='app://-/index.html') {
        try { held=await identity(page,commitDeadline,false,diagnostic); } catch { held=null;if(diagnostic)diagnostic.status='identity-query-failed'; }
        if(held) {
          if(!officialInitialMain(held))return stop('identity-rejected');
          break;
        }
      }
      await pause(Math.min(50,Math.max(0,commitDeadline-Date.now())));
    }
    if(!guard())return stop('ownership-lost');
    if(!held||Date.now()>=commitDeadline)return stop('deadline');
    const between=browser.contexts().flatMap(c=>c.pages());
    if(between.length!==1||between[0]!==page)return stop('page-count');
    const fresh=await identity(page,deadline,false);
    const after=browser.contexts().flatMap(c=>c.pages());
    if(Date.now()>=deadline)return stop('deadline');
    if(!guard())return stop('ownership-lost');
    if(after.length!==1||after[0]!==page)return stop('page-count');
    if(!sameCorrelationIdentity(held,fresh)||!officialInitialMain(fresh))return stop('identity-changed');
    if(diagnostic)diagnostic.status='captured';
    return held;
  } catch{return stop('query-failed');}
}
function mainConfirmationFacts() {
  return {status:'unmeasured',identityUnchanged:null,mainScopeUnique:null,documentFocused:null,counts:null};
}
async function bindCorrelationMain(held,browser,guard,deadline,route,
  identity=correlationIdentity,pause=ms=>new Promise(resolve=>setTimeout(resolve,ms)),diagnostic=null,settleGuard=null) {
  const stop=status=>{if(diagnostic)diagnostic.status=status;return null;};
  try {
    if(!held)return stop('initial-missing');
    if(Date.now()>=deadline)return stop('deadline');
    if(!guard())return stop('ownership-lost');
    let fresh;
    do {
      if(settleGuard&&(!await settleGuard()||Date.now()>=deadline))return stop('guard-rejected');
      fresh=await identity(held.page,deadline);
      if(Date.now()>=deadline)return stop('deadline');
      if(!guard())return stop('ownership-lost');
      if(settleGuard&&!sameCorrelationIdentity(held,fresh))return stop('identity-changed');
      if(settleGuard&&!fresh.scope.focused)return stop('document-unfocused');
      if(settleGuard&&!await settleGuard())return stop('guard-rejected');
      if(fresh.scope.mainScope||!settleGuard)break;
      // Completed trust may still be hydrating the same source page. Only
      // passive proof repeats; zero controls never authorize an action.
      await pause(Math.min(100,Math.max(0,deadline-Date.now())));
    } while(Date.now()<deadline);
    if(diagnostic) {
      diagnostic.identityUnchanged=sameCorrelationIdentity(held,fresh);
      diagnostic.mainScopeUnique=fresh.scope.mainScope;
      diagnostic.documentFocused=fresh.scope.focused;
      diagnostic.counts=fresh.scope.counts;
    }
    if(Date.now()>=deadline)return stop('deadline');
    if(!guard())return stop('ownership-lost');
    if(!sameCorrelationIdentity(held,fresh))return stop('identity-changed');
    if(!fresh.scope.mainScope)return stop('source-scope');
    if(!fresh.scope.focused)return stop('document-unfocused');
    const proof=heldMainGuard(held,browser,guard,deadline,route,identity,pause,true);
    if(!await proof())return stop(Date.now()>=deadline?'deadline':'guard-rejected');
    if(diagnostic)diagnostic.status='confirmed';
    return fresh;
  } catch{return stop(Date.now()>=deadline?'deadline':'query-failed');}
}
async function observeMainAux(held,browser,guard,deadline,auxRoute,
  identity=correlationIdentity,pause=ms=>new Promise(r=>setTimeout(r,ms))) {
  const facts=correlationFacts();
  const pages=()=>browser.contexts().flatMap(c=>c.pages());
  const stop=status=>{facts.status=status;return facts;};
  try {
    const first=pages();facts.totalPages=first.length<=32?first.length:null;
    if(!held)return facts;
    if(first.length!==2)return stop('page-count');
    if(!first.includes(held.page))return stop('identity-changed');
    const auxiliary=first.find(p=>p!==held.page);
    if(auxRoute(auxiliary.url())!=='avatarOverlay')return stop('source-scope');
    const initialAux=await identity(auxiliary,deadline);
    for(let sample=0;sample<2;sample++) {
      if(Date.now()>=deadline)return stop('deadline');
      if(!guard())return stop('ownership-lost');
      const current=pages();
      if(current.length!==2||!current.includes(held.page)||!current.includes(auxiliary))return stop('identity-changed');
      const main=await identity(held.page,deadline),aux=await identity(auxiliary,deadline);
      if(Date.now()>=deadline)return stop('deadline');
      if(!guard())return stop('ownership-lost');
      if(!sameCorrelationIdentity(held,main)||!sameCorrelationIdentity(initialAux,aux))return stop('identity-changed');
      const finalPages=pages();
      if(finalPages.length!==2||!finalPages.includes(held.page)||!finalPages.includes(auxiliary))return stop('identity-changed');
      facts.heldMainUnchanged=true;facts.auxRouteMatched=true;facts.mainScopeUnique=main.scope.mainScope;
      facts.main=main.scope.counts;facts.aux=aux.scope.counts;
      facts.mainDocumentFocused=main.scope.focused;facts.auxDocumentFocused=aux.scope.focused;
      facts.auxMainControlsAbsent=['roleLegend','roleRadios','engineering','dialog'].every(k=>facts.aux[k]===0);
      facts.auxComposerAbsent=facts.aux.quickChatComposer===0&&facts.aux.editable===0;
      if(!facts.mainScopeUnique||!facts.auxMainControlsAbsent||!facts.auxComposerAbsent
        ||!facts.mainDocumentFocused||facts.auxDocumentFocused)return stop('source-scope');
      facts.guarded=true;facts.stableSamples++;
      if(sample===0)await pause(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    return stop('observed');
  } catch{return stop(Date.now()>=deadline?'deadline':'query-failed');}
}
// A completed trust action can precede the source auxiliary's first loader.
// A blank page is only a pending observation, never an input capability.
async function settleFolderAuxiliary(held,extra,pages,valid,deadline,route,identity,pause) {
  while(Date.now()<deadline) {
    if(!valid())return false;
    const before=pages();
    if(before.length!==2||!before.includes(held.page)||!before.includes(extra))return false;
    const main=await identity(held.page,deadline);
    if(!valid()||!sameCorrelationIdentity(held,main)||!main.scope.mainScope||!main.scope.focused)return false;
    const after=pages();
    if(after.length!==2||!after.includes(held.page)||!after.includes(extra))return false;
    const url=extra.url();
    if(route(url)==='avatarOverlay')return valid();
    if(url!==''&&url!=='about:blank')return false;
    await pause(Math.min(100,Math.max(0,deadline-Date.now())));
  }
  return false;
}
// Identity is captured while the official primary route is the sole page.
// Source confirmation and focused/inert proofs happen before any input.
// Later source-known inert avatar pages never become selectable input targets.
function heldMainGuard(held, browser, owner, deadline, route,
  identity=correlationIdentity,pause=ms=>new Promise(resolve=>setTimeout(resolve,ms)),
  requireMainScope=false,allowInitialAppearance=false,requireDocumentFocus=true) {
  return require('./codex-main-guard.cjs').createHeldMainGuard(held,browser,owner,deadline,route,
    identity,pause,requireMainScope,allowInitialAppearance,requireDocumentFocus,
    {sameCorrelationIdentity,settleFolderAuxiliary,now:()=>Date.now()});
}
// One public page activation; it never substitutes for fresh focus/owner proof.
async function focusCapturedMain(held,proof,deadline,identity=correlationIdentity,
  same=sameCorrelationIdentity,pause=ms=>new Promise(resolve=>setTimeout(resolve,ms)),diagnostic=null,nativeActivation=null,observePoint=null) {
  const phase=value=>{if(diagnostic)diagnostic.phase=value;};
  const stop=status=>{if(diagnostic)diagnostic.status=status;return status==='focused';};
  const rejected=()=>stop(Date.now()>=deadline?'deadline':'rejected');
  const proved=async()=>{
    const result=await proof();
    if(!result&&diagnostic&&typeof proof.failure==='function') {
      const reason=proof.failure();
      if(['deadline','native-ownership','page-set','main-identity','main-focus','main-scope',
        'auxiliary-route','auxiliary-identity','auxiliary-focus','auxiliary-controls','query-failed','unmeasured'].includes(reason))
        diagnostic.guardFailure=reason;
    }
    return result;
  };
  try {
    phase('pre-proof');
    if(!held||Date.now()>=deadline||!await proved())return rejected();
    phase('pre-identity');
    let before;
    while(Date.now()<deadline) {
      before=await identity(held.page,deadline);
      if(Date.now()>=deadline||!same(held,before)||!await proved())return rejected();
      if(before.scope.mainScope)break;
      // Source controls may mount after DOM readiness. This is passive only;
      // the original document and all ownership/auxiliary proofs stay held.
      await pause(Math.min(100,Math.max(0,deadline-Date.now())));
      if(Date.now()>=deadline||!await proved())return rejected();
    }
    if(!before?.scope.mainScope)return rejected();
    let nativeActivated=false;
    if(!before.scope.focused) {
      if(nativeActivation) {
        nativeActivation.prepare();
        proof.allowPassiveActivationSettle?.();
        const second=await identity(held.page,deadline);
        if(Date.now()>=deadline||!same(held,second)||!second.scope.mainScope||!await proved())return rejected();
      }
      // This is the same one consumed activation; no diagnostic retries it.
      phase('activation');
      if(Date.now()>=deadline)return rejected();
      if(diagnostic)diagnostic.activationAttempted=true;
      if(nativeActivation) {
        nativeActivation.activate();
        nativeActivated=true;
      } else await held.page.bringToFront();
    }
    phase('polling');
    let focusedSamples=0,pointObserved=false;
    while(Date.now()<deadline) {
      if(!await proved())return rejected();
      const fresh=await identity(held.page,deadline);
      if(Date.now()>=deadline||!same(held,fresh)||!fresh.scope.mainScope||!await proved())return rejected();
      if(nativeActivated&&!nativeActivation.verify()) {
        if(typeof nativeActivation.pending!=='function'||nativeActivation.pending()!==true)return rejected();
        const pendingStack=typeof nativeActivation.pendingStack==='function'?nativeActivation.pendingStack():null;
        if(diagnostic&&pendingStack)diagnostic.nativePendingStack=pendingStack;
        if(diagnostic&&observePoint&&!pointObserved) {
          pointObserved=true;
          diagnostic.nativePointObservation=await observePoint();
          if(Date.now()>=deadline||!await proved())return rejected();
        }
        focusedSamples=0;
        await pause(Math.min(100,Math.max(0,deadline-Date.now())));
        continue;
      }
      focusedSamples=fresh.scope.focused?focusedSamples+1:0;
      if(focusedSamples===2) {
        proof.requireDocumentFocus();
        phase('final-proof');
        return Date.now()<deadline&&await proved()&&Date.now()<deadline?stop('focused'):rejected();
      }
      await pause(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    return stop('deadline');
  } catch {
    const boundary=typeof nativeActivation?.failure==='function'?nativeActivation.failure():null;
    if(diagnostic&&['request','cg-inventory-before','ax-main-before','cg-inventory-after',
       'ax-main-after','identity','trust'].includes(boundary)) {
      diagnostic.nativeBoundary=boundary;
      const inventory=typeof nativeActivation?.inventoryFailure==='function'?nativeActivation.inventoryFailure():null;
      if(inventory)diagnostic.nativeInventoryFailure=inventory;
    }
    const actionFailure=typeof nativeActivation?.actionFailure==='function'?nativeActivation.actionFailure():null;
    if(diagnostic&&actionFailure)diagnostic.nativeActivationFailure=actionFailure;
    return stop(Date.now()>=deadline?'deadline':'query-failed');
  } finally {proof?.finishPassiveActivationSettle?.();}
}

function publishCodexBinding(output,owner,connection,guard) {
  const root=path.dirname(output),bindingPath=path.join(root,`main-binding-${owner}.private`);
  const checkpoint={...guard.binding(),ownerPid:owner,launcherPid:connection.launcherPid,port:connection.port};
  const metadata=fs.lstatSync(root);
  if(!metadata.isDirectory()||metadata.isSymbolicLink()
      ||process.platform!=='win32'&&(metadata.mode&0o077)!==0)throw new Error('private-root');
  fs.writeFileSync(bindingPath,JSON.stringify(checkpoint)+'\n',{mode:0o600,flag:'wx'});
}
function passiveCatalogGuard(browser,page,ownerGuard) {
  const guard=()=>{
    guard.lastFailure=null;
    const pages=browser.contexts().flatMap(context=>context.pages());
    if(pages.length!==1||pages[0]!==page){guard.lastFailure='page-set';return false;}
    if(!ownerGuard()){guard.lastFailure='native-ownership';return false;}
    return true;
  };
  guard.lastFailure=null;
  return guard;
}
function recordStaticDialog(value) {
  const destination=`${output}.dialog-title.json`;
  try {
    fs.writeFileSync(`${destination}.tmp`,JSON.stringify(value)+'\n',{mode:0o600,flag:'wx'});
    fs.renameSync(`${destination}.tmp`,destination);
  } catch(_) {} // Advisory only; never changes the original inventory verdict.
}
function linuxDialogPolicy(appName,platform,env) {
  return onboardingTrial(appName,platform,env)&&platform==='linux'
    &&env.NANH_CODEX_PROJECT_POLICY==='open-project'
    &&env.NANH_CODEX_PROJECT_ARTIFACT_SHA256==='e0174d8d0a5f4141145458c814f3c2d863dd67e942b868785a1f5dac9cba3e16';
}
function linuxDialogFacts() {
  return {schemaVersion:1,mechanism:'codex-linux-startup-dialog',diagnosticsOnly:true,
    sourceVersion:'26.930.31730',
    completeSourceSha256:'16b6c59e36aa19da0c4ec1560b6cedec43fabffeca2601710cb6f25f22c593cc',
    onboardingSourceSha256:'b8dff84333a6cfb62341d43642087ba8d72dd31225ed2b3b8e29ad7da31372c6',
    projectSourceSha256:'802041599f534cdc852760bcc3eb18bc4bdc2fda523b8983098c5946476504a9',
    status:'guard-rejected',candidate:'unknown',
    sourceCount:{dialogCount:null,allSetTitleCount:null,importedSetupTitleCount:null,
      computerHistoryTitleCount:null,projectImportTitleCount:null,
      allSetMatchCount:null,importedSetupMatchCount:null,computerHistoryMatchCount:null,projectImportMatchCount:null}};
}
function holdLinuxDialog() {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
  return {document,dialog:dialogs.length===1?dialogs[0]:null};
}
// Standalone browser callback: static source predicates, no application callbacks.
function matchLinuxDialog(held) {
  const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  if(held.document!==document||!document.hasFocus())return null;
  const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"]')].filter(visible);
  if(dialogs.length!==1||dialogs[0]!==held.dialog||held.dialog.ownerDocument!==document
      ||held.dialog.getAttribute('role')!=='dialog')return null;
  const dialog=held.dialog;
  const titles=[...dialog.querySelectorAll('h1,h2,h3,[role="heading"],[class~="text-3xl"][class~="leading-9"][class~="font-normal"]')].filter(visible);
  const buttons=[...dialog.querySelectorAll('button')].filter(visible);
  const forms=[...dialog.querySelectorAll('form')].filter(visible);
  const divs=[dialog,...dialog.querySelectorAll('div')].filter(visible);
  if([titles,buttons,forms,divs].some(nodes=>nodes.length>512))return null;
  const text=e=>e.innerText.trim();
  const titleCount=label=>titles.filter(e=>text(e)===label).length;
  const buttonCount=label=>buttons.filter(e=>text(e)===label).length;
  const styled=(element,tokens)=>tokens.every(token=>element.classList.contains(token));
  const oneTitle=label=>titles.find(e=>text(e)===label);
  const counts={dialogCount:1,allSetTitleCount:titleCount("You're all set"),
    importedSetupTitleCount:titleCount('Continue with your existing setup'),
    computerHistoryTitleCount:titleCount('Connect Computer History'),
    projectImportTitleCount:titleCount('Select settings to import'),
    allSetMatchCount:0,importedSetupMatchCount:0,computerHistoryMatchCount:0,projectImportMatchCount:0};
  if(Object.values(counts).some(count=>count>32))return null;
  if(counts.allSetTitleCount===1) {
    const title=oneTitle("You're all set");
    const sourceForms=forms.filter(e=>styled(e,['m-auto','flex','w-full','shrink-0','flex-col','items-center','justify-between','py-4'])&&e.contains(title));
    if(sourceForms.length===1&&styled(title,['text-3xl','leading-9','font-normal'])) {
      const form=sourceForms[0],controls=[...form.querySelectorAll('button')].filter(visible),links=[...form.querySelectorAll('a')].filter(visible);
      if(controls.length===1&&text(controls[0])==='Continue'&&controls[0].getAttribute('type')==='submit'
          &&links.filter(e=>e.classList.contains('underline')&&e.getAttribute('href')==='https://openai.com/terms').length===1
          &&links.filter(e=>e.classList.contains('underline')&&e.getAttribute('href')==='https://openai.com/privacy').length===1)counts.allSetMatchCount=1;
    }
  }
  if(counts.importedSetupTitleCount===1) {
    const title=oneTitle('Continue with your existing setup');
    const roots=divs.filter(e=>e.contains(title)&&(
      styled(e,['flex','w-full','max-w-3xl','flex-col','items-center','overflow-hidden','px-10','pb-10'])
      ||styled(e,['flex','w-full','max-w-xl','flex-col','gap-6'])));
    if(roots.length===1&&roots[0].contains(buttons.find(e=>text(e)==='Continue'))&&buttonCount('Continue')===1
        &&((buttonCount('Not now')===1&&buttonCount('Skip')===0)||(buttonCount('Not now')===0&&buttonCount('Skip')===1)))counts.importedSetupMatchCount=1;
  }
  if(counts.computerHistoryTitleCount===1) {
    const title=oneTitle('Connect Computer History');
    const sourceForms=forms.filter(e=>e.contains(title)&&styled(e,['pointer-events-auto','relative','hide-scrollbar','flex','flex-col','gap-6','overflow-y-auto','pb-10']));
    const typed=(label,type)=>buttons.filter(e=>text(e)===label&&e.getAttribute('type')===type).length;
    if(title.tagName==='H2'&&styled(title,['heading-dialog','select-none'])&&sourceForms.length===1
        &&typed('Customize apps','button')===1&&typed('Allow access','submit')+typed('Allow all apps','submit')===1
        &&typed('Not now','button')<=1&&buttons.every(e=>sourceForms[0].contains(e)))counts.computerHistoryMatchCount=1;
  }
  if(counts.projectImportTitleCount===1) {
    const roots=divs.filter(e=>styled(e,['max-h-[min(720px,calc(100vh-64px))]','overflow-hidden'])&&e.contains(oneTitle('Select settings to import')));
    if(roots.length===1&&buttonCount('Continue')===1&&buttonCount('Not now')===1
        &&buttons.every(e=>roots[0].contains(e))&&roots[0].querySelectorAll('[role="checkbox"],input[type="checkbox"]').length>0)counts.projectImportMatchCount=1;
  }
  const matches=[['all-set',counts.allSetMatchCount],['imported-setup',counts.importedSetupMatchCount],
    ['computer-history',counts.computerHistoryMatchCount],['project-import',counts.projectImportMatchCount]].filter(([,count])=>count===1);
  return {status:matches.length===1?'matched':matches.length>1?'ambiguous':'other',
    candidate:matches.length===1?matches[0][0]:matches.length>1?'ambiguous':'unknown',sourceCount:counts};
}
async function observeLinuxDialog(held,browser,guard,deadline,identity=correlationIdentity) {
  const facts=linuxDialogFacts();let handle;
  const reprove=async()=>{
    if(!held||Date.now()>=deadline||!guard())return false;
    const pages=browser.contexts().flatMap(c=>c.pages());
    if(pages.length!==1||pages[0]!==held.page||!officialInitialMain(held))return false;
    const fresh=await identity(held.page,deadline,false);
    return Date.now()<deadline&&guard()&&sameCorrelationIdentity(held,fresh)
      &&browser.contexts().flatMap(c=>c.pages()).length===1
      &&browser.contexts().flatMap(c=>c.pages())[0]===held.page;
  };
  try {
    if(!await reprove())return facts;
    handle=await held.page.evaluateHandle(holdLinuxDialog);
    if(!await reprove())return facts;
    const first=await held.page.evaluate(matchLinuxDialog,handle);
    if(!first||!await reprove())return facts;
    const second=await held.page.evaluate(matchLinuxDialog,handle);
    if(!second||JSON.stringify(first)!==JSON.stringify(second)||!await reprove())return facts;
    return {...facts,...second};
  } catch{return facts;}finally{if(handle)await handle.dispose().catch(()=>{});}
}
function recordLinuxDialog(value) {
  const destination=`${output}.linux-dialog.json`;
  try {
    fs.writeFileSync(`${destination}.tmp`,JSON.stringify(value)+'\n',{mode:0o600,flag:'wx'});
    fs.renameSync(`${destination}.tmp`,destination);
  } catch { /* Advisory-only recording cannot change the inventory verdict. */ }
}

async function run() {
  if (!['chatgpt-desktop', 'claude-desktop', 'pen-desktop'].includes(app)
      || !Number.isSafeInteger(request.ownerPid) || request.ownerPid <= 1
      || !Number.isSafeInteger(connection.launcherPid) || connection.launcherPid <= 1
      || !Number.isSafeInteger(connection.port) || connection.port <= 1024 || connection.port > 65535) {
    facts.errorCategory = 'invalid-request'; save(); return;
  }
  const trial = onboardingTrial(app, process.platform, process.env);
  const started = Date.now();
  const deadline = started + (trial ? 35000 : 25000);
  const totalDeadline = started + onboardingBudget(trial,process.platform);
  const rootProof = require('./endpoint-ownership.cjs').proof(String(request.ownerPid), String(connection.port));
  facts.launcherOwned = rootProof.descendant(connection.launcherPid, deadline);
  if (!facts.launcherOwned) { facts.errorCategory = 'launcher-unowned'; save(); return; }
  const ownership = require('./endpoint-ownership.cjs').proof(String(connection.launcherPid), String(connection.port));
  while (Date.now() < deadline && !ownership.ownedEndpoint(deadline)) await new Promise(r => setTimeout(r, 250));
  facts.endpointOwned = ownership.ownedEndpoint(deadline);
  if (!facts.endpointOwned) { facts.errorCategory = 'endpoint-unowned'; save(); return; }
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${connection.port}`, { timeout: 2000, noDefaults: true });
  try {
    facts.attached = true;
    let pages = browser.contexts().flatMap(context => context.pages());
    // The debugger can listen before the application creates its first page.
    // Wait for that page, but never choose among multiple application targets.
    while (pages.length === 0 && Date.now() < deadline && ownership.ownedEndpoint(deadline)) {
      await new Promise(r => setTimeout(r, 250));
      pages = browser.contexts().flatMap(context => context.pages());
    }
    facts.pageCount = Math.min(4096, pages.length);
    if (pages.length !== 1) {
      if (trial) { facts.mainAuxCorrelation=correlationFacts(); facts.mainAuxCorrelation.totalPages=pages.length<=32?pages.length:null; }
      facts.errorCategory = 'target-ambiguous'; save(); return;
    }
    const page = pages[0];
    const ownerGuard=()=>{
      const record=proof=>{
        const reason=typeof proof.failure==='function'?proof.failure():'unmeasured';
        if(['ancestor-unowned','ancestor-query','listener-unavailable','listener-shape',
          'listener-unowned','listener-query','unmeasured'].includes(reason))facts.nativeOwnershipFailure=reason;
        delete facts.nativeListenerShape;
        const details=typeof proof.failureDetails==='function'?proof.failureDetails():null;
        if(reason==='listener-shape'&&details)facts.nativeListenerShape=details;
      };
      let proof=rootProof;
      try {
        if(!proof.descendant(connection.launcherPid,totalDeadline)){record(proof);return false;}
        proof=ownership;
        if(!proof.ownedEndpoint(totalDeadline)){record(proof);return false;}
        return true;
      } catch(error){record(proof);throw error;}
    };
    if(trial)facts.initialMainBinding=initialMainFacts();
    const initialMain=trial?await captureCorrelationMain(page,browser,ownerGuard,deadline,
      correlationIdentity,ms=>new Promise(resolve=>setTimeout(resolve,ms)),facts.initialMainBinding):null;
    let pageErrorCount = 0;
    page.on('pageerror', () => { pageErrorCount = Math.min(4096, pageErrorCount + 1); });
    const documentDeadline = Math.min(deadline, Date.now() + 10000);
    while (Date.now() < documentDeadline && ownership.ownedEndpoint(documentDeadline)) {
      const loaded = await page.evaluate(() => document.readyState === 'complete'
        && document.body !== null && document.querySelectorAll('button,input,textarea,[contenteditable="true"]').length > 0);
      if (loaded) break;
      await new Promise(r => setTimeout(r, 250));
    }
    if (!ownership.ownedEndpoint(documentDeadline)) { facts.endpointOwned = false; facts.errorCategory = 'endpoint-unowned'; save(); return; }
    let focusGuard;
    if(trial&&process.platform==='darwin') {
      focusGuard=heldMainGuard(initialMain,browser,ownerGuard,totalDeadline,
        require('./codex-onboarding.cjs').sourceRoute,correlationIdentity,
        ms=>new Promise(resolve=>setTimeout(resolve,ms)),false,false,false);
      const nativeActivation=require('./codex-native-activation.cjs').controller(
        request.nativeActivation,request.ownerPid,connection.launcherPid,deadline);
      facts.initialMainActivation={phase:'pre-proof',status:'unmeasured',activationAttempted:false,guardFailure:null};
      if(!await focusCapturedMain(initialMain,focusGuard,deadline,correlationIdentity,
        sameCorrelationIdentity,ms=>new Promise(resolve=>setTimeout(resolve,ms)),facts.initialMainActivation,nativeActivation,async()=>{
          let session;
          try {
            if(Date.now()>=deadline||!ownerGuard())throw Error('expired');
            session=await initialMain.page.context().newCDPSession(initialMain.page);
            return await require('./codex-point-observation.cjs').observe({session,native:nativeActivation,
              held:initialMain,deadline,owner:ownerGuard});
          } catch {return {reason:'observation-unavailable',mappingObserved:false,inputAuthorized:false};}
          finally {if(session)await session.detach().catch(()=>{});}
        })) {
        facts.initialMainConfirmation=mainConfirmationFacts();
        await bindCorrelationMain(initialMain,browser,ownerGuard,deadline,
          require('./codex-onboarding.cjs').sourceRoute,correlationIdentity,
          ms=>new Promise(resolve=>setTimeout(resolve,ms)),facts.initialMainConfirmation);
        facts.errorCategory='attachment-or-action-failed';save();return;
      }
    }
    if(linuxDialogPolicy(app,process.platform,process.env)) {
      recordLinuxDialog(await observeLinuxDialog(initialMain,browser,ownerGuard,deadline));
    }
    const titleCatalog=require('./codex-dialog-catalog.cjs');
    if(['linux','darwin','win32'].includes(process.platform)&&titleCatalog.policy(app,process.platform,process.env)) {
      // Passive title evidence uses the captured sole main document, independently
      // of role-onboarding controls hidden behind an active startup dialog.
      const soleGuard=passiveCatalogGuard(browser,page,ownerGuard);
      recordStaticDialog(await titleCatalog.observe(initialMain,process.platform,
        {guard:soleGuard,identity:p=>correlationIdentity(p,deadline,false),same:sameCorrelationIdentity,deadline}));
    }
    if (process.env.NANH_CODEX_PUBLIC_ONBOARDING !== undefined) {
      const targetReady = app === 'chatgpt-desktop'
        && await page.evaluate(() => location.protocol === 'app:' && document.readyState === 'complete');
      if (!targetReady) { facts.errorCategory = 'invalid-request'; save(); return; }
      const correlationDeadline=onboardingDeadline(trial,deadline,totalDeadline);
      if(trial)facts.initialMainConfirmation=mainConfirmationFacts();
      const profileAuthority=request.codexProfileLoan===undefined?null:
        require('./codex-profile-state.cjs').authority(request.codexProfileLoan,correlationDeadline,ownerGuard);
      if(request.codexProfileLoan!==undefined&&(!profileAuthority||request.codexProfileLoan.directories[0].path!==request.ownedWorkspace)){profileAuthority?.close();facts.errorCategory='invalid-request';save();return;}
      const onboardingOwnerGuard=profileAuthority?()=>ownerGuard()===true&&profileAuthority.verify():ownerGuard;
      try {
      let folderTrust,trustGuard;
      if(trial&&request.ownedWorkspace!==undefined) {
        const folderAuthority=require('./codex-folder-trust.cjs').authority(request.ownedWorkspace);
        trustGuard=(!profileAuthority&&focusGuard)||heldMainGuard(initialMain,browser,onboardingOwnerGuard,correlationDeadline,
          require('./codex-onboarding.cjs').sourceRoute,correlationIdentity,
          ms=>new Promise(resolve=>setTimeout(resolve,ms)),false,process.platform==='win32');
        folderTrust=await require('./codex-folder-trust.cjs').run(page,trustGuard,
          correlationDeadline,folderAuthority,()=>trustGuard.sealInitialActions());
        if(folderTrust.status==='completed'&&folderTrust.clickAttempted&&folderTrust.clickCompleted)
          trustGuard.allowPassiveFolderSettle();
      }
      const heldMain=trial?await bindCorrelationMain(initialMain,browser,onboardingOwnerGuard,correlationDeadline,
        require('./codex-onboarding.cjs').sourceRoute,correlationIdentity,
        ms=>new Promise(resolve=>setTimeout(resolve,ms)),facts.initialMainConfirmation,
        folderTrust?.status==='completed'?trustGuard:null):null;
      trustGuard?.finishPassiveFolderSettle();
      // Trust consumes initial admission; preserve its original auxiliary binding.
      // Fresh role binding above must still succeed before subsequent input.
      const mainGuard=folderTrust?.clickAttempted?(heldMain?trustGuard:undefined):trial&&heldMain?heldMainGuard(heldMain,browser,onboardingOwnerGuard,correlationDeadline,
        require('./codex-onboarding.cjs').sourceRoute,correlationIdentity,
        ms=>new Promise(resolve=>setTimeout(resolve,ms)),false,true):undefined;
      facts.publicOnboarding = await require('./codex-onboarding.cjs').run(page,
        onboardingOwnerGuard,
        correlationDeadline,mainGuard,folderTrust,request.codexProfileLoan);
      const bindingVerified=!!mainGuard&&await mainGuard();
      const codingComposerReady=bindingVerified&&await page.evaluate(require('./codex-onboarding.cjs').codingScope);
      facts.codexSession={bindingVerified,codingComposerReady:!!codingComposerReady,auxiliaryInert:bindingVerified,
        pageCount:Math.min(32,browser.contexts().flatMap(context=>context.pages()).length)};
      if(bindingVerified)publishCodexBinding(output,request.ownerPid,connection,mainGuard);
      if(trial&&browser.contexts().flatMap(c=>c.pages()).length!==1) {
        facts.mainAuxCorrelation=await observeMainAux(heldMain,browser,onboardingOwnerGuard,correlationDeadline,
          require('./codex-onboarding.cjs').sourceRoute);
      }
      } finally {profileAuthority?.close();}
    }
    const counts = await page.evaluate(appName => {
      const visible = e => e.isConnected && e.getBoundingClientRect().width > 0
        && e.getBoundingClientRect().height > 0 && getComputedStyle(e).visibility === 'visible';
      const count = selector => Math.min(4096, [...document.querySelectorAll(selector)].filter(visible).length);
      const buttons = [...document.querySelectorAll('button,[role="button"]')].filter(visible);
      const named = pattern => Math.min(4096, buttons.filter(e => pattern.test(e.getAttribute('aria-label') || e.innerText || '')).length);
      // Exact public distribution headings are classified in memory; never
      // retain headings, labels, HTML or other application text.
      const headings = [...document.querySelectorAll('h1')].filter(visible);
      const cliConnectionFailed = appName === 'chatgpt-desktop'
        && (document.body?.innerText || '').includes('Something went wrong connecting to the Codex CLI. Try restarting');
      const startupScreen = cliConnectionFailed ? 'cli-connection-failed'
        : appName !== 'pen-desktop' ? 'unmeasured'
        : headings.length === 1 && headings[0].textContent === 'Hardware acceleration unavailable' ? 'gpu-unavailable'
        : headings.length === 1 && headings[0].textContent === 'Failed to start pen.dev' ? 'startup-failed' : 'other';
      let sourceScreen, managedSignIn, sourceDialog;
      if (appName === 'chatgpt-desktop') {
        // Frozen public headings, including the source's non-heading all-set title.
        // These are observations only; no category authorizes an action or login.
        const titles = [...document.querySelectorAll('h1,h2,h3,[role="heading"],[class~="text-3xl"][class~="leading-9"][class~="font-normal"]')].filter(visible);
        const exact = labels => Math.min(32, titles.filter(e => labels.includes(e.textContent || '')).length);
        const screenCounts = {
          gatewayHeading: exact(['Connect to your gateway']),
          recoveryHeading: exact(['ChatGPT hit a snag']),
          importHeading: exact(['Import other AI setup', 'Import work from other AI apps', 'Import from other AI apps']),
          allSetHeading: exact(["You're all set"]),
          permissionHeading: exact(['Give ChatGPT access to your computer']),
          continueSignIn: Math.min(32, buttons.filter(e => (e.getAttribute('aria-label') || e.innerText || '') === 'Continue to Sign In').length),
        };
        const categories = [['gatewayHeading', 'gateway-connect'], ['recoveryHeading', 'app-recovery'],
          ['importHeading', 'external-import'], ['allSetHeading', 'all-set'], ['permissionHeading', 'permission-setup']];
        const observed = categories.filter(([key]) => screenCounts[key] > 0);
        const status = observed.length === 0 ? 'unknown'
          : observed.length === 1 && screenCounts[observed[0][0]] === 1 ? observed[0][1] : 'ambiguous';
        sourceScreen = {status, counts: screenCounts};
        // The source renders pre-role sign-in requirements as status/alert
        // text rather than headings. Match only complete public strings.
        const notices = [...document.querySelectorAll('[role="status"],[role="alert"]')].filter(visible);
        const notice = text => Math.min(32, notices.filter(e => (e.textContent || '').trim() === text).length);
        const choice = text => Math.min(32, buttons.filter(e => (e.getAttribute('aria-label') || e.innerText || '') === text).length);
        const requirements = {loading: notice('Loading sign-in requirements…'),
          unsupported: notice('Update Codex on this machine to read its managed sign-in requirements'),
          disabled: notice('Your administrator has disabled all available sign-in methods'),
          error: notice('Unable to load sign-in requirements'),
          chatgptChoice: choice('Continue with ChatGPT'), apiKeyChoice: choice('Enter API key')};
        const statuses = ['loading', 'unsupported', 'disabled', 'error'].filter(key => requirements[key] > 0);
        const choices = requirements.chatgptChoice + requirements.apiKeyChoice;
        const requirementStatus = Object.values(requirements).some(count => count > 1) ? 'ambiguous'
          : statuses.length === 0 ? (choices > 0 ? 'sign-in-options' : 'unknown')
          : statuses.length === 1 && choices === 0 ? statuses[0] : 'ambiguous';
        managedSignIn = {status: requirementStatus, counts: requirements};
        // Frozen workspace-discovery failure is a modal title wrapper, not
        // necessarily an HTML heading. Its exact public title and button are
        // observed only inside visible dialogs; neither permits interaction.
        const dialogs = [...document.querySelectorAll('[role="dialog"],[role="alertdialog"]')].filter(visible);
        const dialogCounts = {dialogs: Math.min(32, dialogs.length), workspaceFailureTitle: 0, retryButton: 0};
        for (const dialog of dialogs.slice(0, 32)) {
          dialogCounts.workspaceFailureTitle += [...dialog.querySelectorAll('*')].filter(e =>
            visible(e) && e.children.length === 0 && (e.textContent || '').trim() === 'Could not load workspaces').length;
          dialogCounts.retryButton += [...dialog.querySelectorAll('button,[role="button"]')].filter(e =>
            visible(e) && (e.getAttribute('aria-label') || e.innerText || '').trim() === 'Try again').length;
        }
        const sourceDialogStatus = dialogs.length > 1 || dialogCounts.workspaceFailureTitle > 1 || dialogCounts.retryButton > 1
          ? 'ambiguous' : dialogs.length === 1 && dialogCounts.workspaceFailureTitle === 1 && dialogCounts.retryButton === 1
            ? 'workspace-discovery-failed' : 'unknown';
        dialogCounts.workspaceFailureTitle = Math.min(32, dialogCounts.workspaceFailureTitle);
        dialogCounts.retryButton = Math.min(32, dialogCounts.retryButton);
        sourceDialog = {status: sourceDialogStatus, counts: dialogCounts};
      }
      return { textareaCount: count('textarea'), editableCount: count('[contenteditable="true"]'),
        startupScreen,
        ...(sourceScreen ? {sourceScreen, managedSignIn, sourceDialog} : {}),
        landingCounts: {
          importHeading: Math.min(4096, [...document.querySelectorAll('h1,h2,h3,[role="heading"]')]
            .filter(visible).filter(e => /^(Import other AI setup|Import work from other AI apps)$/.test(e.textContent || '')).length),
          importDismiss: named(/^(Not now|Skip)$/),
          createProject: named(/^Create project$/),
          sourceFolders: named(/^(Choose source folders|Add folder)$/),
          projectName: Math.min(4096, [...document.querySelectorAll('input')].filter(visible)
            .filter(e => e.getAttribute('aria-label') === 'Project name' || e.placeholder === 'Project name').length),
        },
        onboardingCounts: {
          roleRadios: count('input[type="radio"][name="conversational-onboarding-inline-role"]'),
          roleLegend: Math.min(4096, [...document.querySelectorAll('legend')].filter(visible)
            .filter(e => e.textContent === 'Select the kind of work you do').length),
          workHeading: Math.min(4096, headings.filter(e => e.textContent === 'What type of work do you do?').length),
          suggestionsCheckbox: count('input[type="checkbox"][id="personalized-suggestions"]'),
        },
        sendCount: named(/^(send|send message|submit)$/i), retryCount: named(/^(retry|try again)$/i),
        newThreadCount: named(/^(new chat|new thread|new conversation)$/i),
        loginCount: named(/^(log in|sign in|continue with google|continue with apple)$/i),
        dialogCount: count('[role="dialog"],[role="alertdialog"]'),
        documentState: { readyState: document.readyState,
          targetKind: location.href === 'about:blank' ? 'blank'
            : ({ 'file:': 'file', 'http:': 'http', 'https:': 'https',
                 'chrome-error:': 'browser-error', 'app:': 'app' })[location.protocol] || 'other',
          bodyPresent: document.body !== null,
          elementCount: Math.min(4096, document.querySelectorAll('*').length),
          visibleElementCount: count('*'), inputCount: count('input'),
          frameCount: Math.min(4096, document.querySelectorAll('iframe,frame').length), pageErrorCount: 0 } };
    }, app);
    counts.documentState.pageErrorCount = pageErrorCount;
    Object.assign(facts, counts, { errorCategory: null }); save();
  } finally { await browser.close(); }
}
run().catch(() => { facts.errorCategory = 'attachment-or-action-failed'; save(); process.exitCode = 1; });
