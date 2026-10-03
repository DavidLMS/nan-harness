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
function onboardingDeadline(trial, startupDeadline, totalDeadline, now) {
  return trial ? Math.min(totalDeadline, now + 25000) : startupDeadline;
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
async function bindCorrelationMain(held,browser,guard,deadline,route,
  identity=correlationIdentity,pause=ms=>new Promise(resolve=>setTimeout(resolve,ms))) {
  try {
    if(!held||Date.now()>=deadline||!guard())return null;
    const fresh=await identity(held.page,deadline);
    if(Date.now()>=deadline||!guard()||!sameCorrelationIdentity(held,fresh)
      ||!fresh.scope.mainScope||!fresh.scope.focused)return null;
    const proof=heldMainGuard(held,browser,guard,deadline,route,identity,pause,true);
    return await proof()?fresh:null;
  } catch{return null;}
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
// Identity is captured while the official primary route is the sole page.
// Source confirmation and focused/inert proofs happen before any input.
// Later source-known inert avatar pages never become selectable input targets.
function heldMainGuard(held, browser, owner, deadline, route,
  identity=correlationIdentity, pause=ms=>new Promise(resolve=>setTimeout(resolve,ms)),requireMainScope=false) {
  let auxiliary=null, auxiliaryIdentity=null;
  const pages=()=>browser.contexts().flatMap(context=>context.pages());
  const valid=()=>Date.now()<deadline&&owner()===true;
  const prove=async function prove() {
    if(!held||!valid())return false;
    try {
      const initial=pages();
      if(initial.length<1||initial.length>2||!initial.includes(held.page))return false;
      const extra=initial.find(page=>page!==held.page);
      if(auxiliary&&extra!==auxiliary)return false;
      if(extra&&route(extra.url())!=='avatarOverlay')return false;
      const samples=extra&&!auxiliary?2:1;
      let candidateAux=null;
      for(let sample=0;sample<samples;sample++) {
        if(!valid())return false;
        const before=pages();
        if(before.length!==initial.length||!before.every(page=>initial.includes(page)))return false;
        const main=await identity(held.page,deadline);
        if(!valid()||!sameCorrelationIdentity(held,main)||!main.scope.focused
          ||requireMainScope&&!main.scope.mainScope)return false;
        if(extra) {
          const aux=await identity(extra,deadline);
          const expected=auxiliaryIdentity??candidateAux;
          if(!valid()||expected&&!sameCorrelationIdentity(expected,aux))return false;
          const counts=aux.scope.counts;
          if(aux.scope.focused||['roleLegend','roleRadios','engineering','dialog','quickChatComposer','editable']
              .some(key=>counts[key]!==0))return false;
          candidateAux=aux;
        }
        const after=pages();
        if(!valid()||after.length!==initial.length||!after.every(page=>initial.includes(page)))return false;
        if(sample+1<samples)await pause(Math.min(100,Math.max(0,deadline-Date.now())));
      }
      if(extra&&!auxiliary){auxiliary=extra;auxiliaryIdentity=candidateAux;}
      return true;
    } catch {return false;}
  };
  const privateIdentity=value=>value&&Object.fromEntries(['url','target','frame','loader','frameUrl','fragment'].map(key=>[key,value[key]]));
  prove.binding=()=>({schemaVersion:1,main:privateIdentity(held),auxiliary:privateIdentity(auxiliaryIdentity)});
  return prove;
}
function publishCodexBinding(output,owner,connection,guard) {
  const root=path.dirname(output),bindingPath=path.join(root,`main-binding-${owner}.private`);
  const checkpoint={...guard.binding(),ownerPid:owner,launcherPid:connection.launcherPid,port:connection.port};
  const metadata=fs.lstatSync(root);
  if(!metadata.isDirectory()||metadata.isSymbolicLink()
      ||process.platform!=='win32'&&(metadata.mode&0o077)!==0)throw new Error('private-root');
  fs.writeFileSync(bindingPath,JSON.stringify(checkpoint)+'\n',{mode:0o600,flag:'wx'});
}
async function run() {
  if (!['chatgpt-desktop', 'claude-desktop', 'pen-desktop'].includes(app)
      || !Number.isSafeInteger(request.ownerPid) || request.ownerPid <= 1
      || !Number.isSafeInteger(connection.launcherPid) || connection.launcherPid <= 1
      || !Number.isSafeInteger(connection.port) || connection.port <= 1024 || connection.port > 65535) {
    facts.errorCategory = 'invalid-request'; save(); return;
  }
  const rootProof = require('./endpoint-ownership.cjs').proof(String(request.ownerPid), String(connection.port));
  facts.launcherOwned = rootProof.descendant(connection.launcherPid);
  if (!facts.launcherOwned) { facts.errorCategory = 'launcher-unowned'; save(); return; }
  const ownership = require('./endpoint-ownership.cjs').proof(String(connection.launcherPid), String(connection.port));
  const trial = onboardingTrial(app, process.platform, process.env);
  const started = Date.now();
  const deadline = started + (trial ? 35000 : 25000);
  const totalDeadline = started + (trial ? 60000 : 25000);
  while (Date.now() < deadline && !ownership.ownedEndpoint()) await new Promise(r => setTimeout(r, 250));
  facts.endpointOwned = ownership.ownedEndpoint();
  if (!facts.endpointOwned) { facts.errorCategory = 'endpoint-unowned'; save(); return; }
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${connection.port}`, { timeout: 2000, noDefaults: true });
  try {
    facts.attached = true;
    let pages = browser.contexts().flatMap(context => context.pages());
    // The debugger can listen before the application creates its first page.
    // Wait for that page, but never choose among multiple application targets.
    while (pages.length === 0 && Date.now() < deadline && ownership.ownedEndpoint()) {
      await new Promise(r => setTimeout(r, 250));
      pages = browser.contexts().flatMap(context => context.pages());
    }
    facts.pageCount = Math.min(4096, pages.length);
    if (pages.length !== 1) {
      if (trial) { facts.mainAuxCorrelation=correlationFacts(); facts.mainAuxCorrelation.totalPages=pages.length<=32?pages.length:null; }
      facts.errorCategory = 'target-ambiguous'; save(); return;
    }
    const page = pages[0];
    const ownerGuard=()=>rootProof.descendant(connection.launcherPid)&&ownership.ownedEndpoint();
    if(trial)facts.initialMainBinding=initialMainFacts();
    const initialMain=trial?await captureCorrelationMain(page,browser,ownerGuard,deadline,
      correlationIdentity,ms=>new Promise(resolve=>setTimeout(resolve,ms)),facts.initialMainBinding):null;
    let pageErrorCount = 0;
    page.on('pageerror', () => { pageErrorCount = Math.min(4096, pageErrorCount + 1); });
    const documentDeadline = Math.min(deadline, Date.now() + 10000);
    while (Date.now() < documentDeadline && ownership.ownedEndpoint()) {
      const loaded = await page.evaluate(() => document.readyState === 'complete'
        && document.body !== null && document.querySelectorAll('button,input,textarea,[contenteditable="true"]').length > 0);
      if (loaded) break;
      await new Promise(r => setTimeout(r, 250));
    }
    if (!ownership.ownedEndpoint()) { facts.endpointOwned = false; facts.errorCategory = 'endpoint-unowned'; save(); return; }
    if (process.env.NANH_CODEX_PUBLIC_ONBOARDING !== undefined) {
      const targetReady = app === 'chatgpt-desktop'
        && await page.evaluate(() => location.protocol === 'app:' && document.readyState === 'complete');
      if (!targetReady) { facts.errorCategory = 'invalid-request'; save(); return; }
      const correlationDeadline=onboardingDeadline(trial,deadline,totalDeadline,Date.now());
      const heldMain=trial?await bindCorrelationMain(initialMain,browser,ownerGuard,correlationDeadline,
        require('./codex-onboarding.cjs').sourceRoute):null;
      const mainGuard=trial&&heldMain?heldMainGuard(heldMain,browser,ownerGuard,correlationDeadline,
        require('./codex-onboarding.cjs').sourceRoute):undefined;
      facts.publicOnboarding = await require('./codex-onboarding.cjs').run(page,
        () => rootProof.descendant(connection.launcherPid) && ownership.ownedEndpoint(),
        correlationDeadline,mainGuard);
      const bindingVerified=!!mainGuard&&await mainGuard();
      const codingComposerReady=bindingVerified&&await page.evaluate(require('./codex-onboarding.cjs').codingScope);
      facts.codexSession={bindingVerified,codingComposerReady:!!codingComposerReady,auxiliaryInert:bindingVerified,
        pageCount:Math.min(32,browser.contexts().flatMap(context=>context.pages()).length)};
      if(bindingVerified)publishCodexBinding(output,request.ownerPid,connection,mainGuard);
      if(trial&&browser.contexts().flatMap(c=>c.pages()).length!==1) {
        facts.mainAuxCorrelation=await observeMainAux(heldMain,browser,ownerGuard,correlationDeadline,
          require('./codex-onboarding.cjs').sourceRoute);
      }
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
      return { textareaCount: count('textarea'), editableCount: count('[contenteditable="true"]'),
        startupScreen,
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
