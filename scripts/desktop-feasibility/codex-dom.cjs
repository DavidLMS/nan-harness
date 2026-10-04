// Source-bound public Codex coding UI. No application stores, RPCs or injected actions.
const {candidate,codingScope,sourceRoute}=require('./codex-onboarding.cjs');
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

function turnObservation({prompt,marker}) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const users=[...document.querySelectorAll('[data-local-conversation-user-anchor]')]
    .filter(visible).filter(anchor=>{
      const bubbles=[...anchor.querySelectorAll('[data-user-message-bubble]')].filter(visible);
      return bubbles.length===1&&bubbles[0].innerText.trim()===prompt;
    });
  if(users.length!==1)return {userCount:Math.min(4096,users.length),assistantCount:0,responseVerified:false};
  const turn=users[0].closest('[data-turn-key]');
  if(!turn)return {userCount:1,assistantCount:0,responseVerified:false};
  // Virtual transcript rows split one semantic turn into user/assistant blocks.
  // Their public search key is shared; data-turn-key identifies layout rows.
  const semantic=users[0].closest('[data-content-search-turn-key]');
  let headings,semanticKey=null,heldConversation=null;
  if(semantic) {
    const key=semantic.getAttribute('data-content-search-turn-key');
    const conversation=users[0].closest('[data-thread-find-target="conversation"]');
    semanticKey=key;heldConversation=conversation;
    if(typeof key!=='string'||key.length===0||key.length>512||!conversation
        ||!conversation.contains(semantic))return {userCount:1,assistantCount:0,responseVerified:false};
    const scopes=[...conversation.querySelectorAll('[data-content-search-turn-key]')];
    if(scopes.length>4096)return {userCount:1,assistantCount:0,responseVerified:false};
    headings=[];
    for(const scope of scopes) {
      if(scope.getAttribute('data-content-search-turn-key')!==key
          ||scope.closest('[data-thread-find-target="conversation"]')!==conversation)continue;
      for(const heading of scope.querySelectorAll('[data-conversation-role="assistant"]')) {
        if(heading.closest('[data-content-search-turn-key]')===scope)headings.push(heading);
        if(headings.length>4096)return {userCount:1,assistantCount:0,responseVerified:false};
      }
    }
  } else headings=[...turn.querySelectorAll('[data-conversation-role="assistant"]')];
  const units=new Set(headings.map(heading=>heading.closest('[data-content-search-unit-key]'))
    .filter(unit=>unit&&(semantic?unit.closest('[data-thread-find-target="conversation"]')===heldConversation
      &&unit.closest('[data-content-search-turn-key]')?.getAttribute('data-content-search-turn-key')===semanticKey:turn.contains(unit))));
  let matches=0;
  for(const unit of units) {
    if(unit.querySelector('[data-local-conversation-user-anchor],[data-user-message-bubble]'))continue;
    const paragraphs=[...unit.querySelectorAll('p')].filter(visible)
      .filter(p=>!p.closest('[data-markdown-copy="exclude"]'));
    if(paragraphs.length===1&&paragraphs[0].innerText.trim()===marker)matches++;
  }
  return {userCount:1,assistantCount:Math.min(4096,units.size),responseVerified:matches===1};
}

// Capacity Retry starts a new empty turn. Retain its preceding user and
// conversation plus pre-existing source unit IDs before consuming that action.
function holdRetryContinuation({prompt}) {
  const users=[...document.querySelectorAll('[data-local-conversation-user-anchor]')]
    .filter(e=>e.isConnected&&e.querySelectorAll('[data-user-message-bubble]').length===1
      &&e.querySelector('[data-user-message-bubble]').innerText.trim()===prompt);
  if(users.length!==1)return null;
  const user=users[0],conversation=user.closest('[data-thread-find-target="conversation"]');
  if(!conversation||conversation.querySelectorAll('*').length>4096)return null;
  const unitKeys=[...conversation.querySelectorAll('[data-content-search-unit-key]')]
    .map(e=>e.getAttribute('data-content-search-unit-key'));
  if(unitKeys.some(key=>typeof key!=='string'||key.length===0||key.length>512)
      ||new Set(unitKeys).size!==unitKeys.length)return null;
  const laterUsers=[...conversation.querySelectorAll('[data-local-conversation-user-anchor]')]
    .some(e=>e!==user&&(user.compareDocumentPosition(e)&5)!==0);
  if(laterUsers)return null;
  return {document,user,conversation,unitKeys};
}
function retryContinuationObservation({held,prompt,marker}) {
  const empty={userCount:0,assistantCount:0,responseVerified:false};
  if(!held||held.document!==document||!held.user.isConnected||!held.conversation.isConnected
      ||held.user.closest('[data-thread-find-target="conversation"]')!==held.conversation)return empty;
  const users=[...document.querySelectorAll('[data-local-conversation-user-anchor]')].filter(e=>
    e.querySelectorAll('[data-user-message-bubble]').length===1&&e.querySelector('[data-user-message-bubble]').innerText.trim()===prompt);
  if(users.length!==1||users[0]!==held.user||held.conversation.querySelectorAll('*').length>4096)return empty;
  if([...held.conversation.querySelectorAll('[data-local-conversation-user-anchor]')]
      .some(e=>e!==held.user&&(held.user.compareDocumentPosition(e)&5)!==0))return empty;
  const units=new Set([...held.conversation.querySelectorAll('[data-conversation-role="assistant"]')]
    .map(e=>e.closest('[data-content-search-unit-key]')).filter(Boolean));
  let matches=0,count=0;
  const visible=e=>{const r=e.getBoundingClientRect(),style=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&style.display!=='none'&&style.visibility!=='hidden';};
  const keys=new Set;
  for(const unit of units) {
    const key=unit.getAttribute('data-content-search-unit-key');
    if(typeof key!=='string'||key.length===0||key.length>512||keys.has(key))return empty;
    keys.add(key);
    if(held.unitKeys.includes(key)||unit.closest('[data-thread-find-target="conversation"]')!==held.conversation
        ||unit.querySelector('[data-local-conversation-user-anchor],[data-user-message-bubble]'))continue;
    const order=held.user.compareDocumentPosition(unit);
    if((order&1)!==0||(order&4)===0)continue;
    count++;
    const paragraphs=[...unit.querySelectorAll('p')].filter(visible)
      .filter(e=>!e.closest('[data-markdown-copy="exclude"]'));
    if(paragraphs.length===1&&paragraphs[0].innerText.trim()===marker)matches++;
  }
  return {userCount:1,assistantCount:count,responseVerified:matches===1};
}

// Input hit testing has its own public contenteditable contract; button-only
// onboarding controls cannot establish a ProseMirror editor's actionability.
function sampleEditor(control,kind='editor') {
  const blocked=reason=>({blocked:reason});
  if(kind==='button'?control.tagName!=='BUTTON':control.tagName!=='DIV'||!control.classList.contains('ProseMirror')
      ||control.getAttribute('contenteditable')!=='true')return blocked('unsupported-control');
  if(!control.isConnected||control.ownerDocument!==document||control.closest('[inert]'))return blocked('detached-or-inert');
  if(control.disabled||control.readOnly||control.getAttribute('aria-disabled')==='true')return blocked('disabled');
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  if(!visible(control))return blocked('hidden');
  if([...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"],[role="menu"]')].some(visible))return blocked('foreign-overlay');
  if(getComputedStyle(control).pointerEvents==='none')return blocked('pointer-disabled');
  for(let e=control,depth=0;e;e=e.parentElement) {
    // An ancestor can disable its own hit area while a child explicitly opts in.
    // The editor's computed property and elementFromPoint prove its actual hit.
    if(++depth>64)return blocked('ancestor-limit');
    if(!e.isConnected||e.ownerDocument!==document)return blocked('detached-or-inert');
  }
  const r=control.getBoundingClientRect(),points=[];
  for(const fy of [0.25,0.5,0.75])for(const fx of [0.25,0.5,0.75]) {
    const x=r.left+control.clientLeft+control.clientWidth*fx;
    const y=r.top+control.clientTop+control.clientHeight*fy;
    if(x<0||y<0||x>=innerWidth||y>=innerHeight)continue;
    const front=document.elementFromPoint(x,y);
    if(front===control||control.contains(front))points.push({x:x-r.left-control.clientLeft,y:y-r.top-control.clientTop});
  }
  return {rect:[r.left,r.top,r.width,r.height],points};
}

async function ordinaryClick(locator,guard,deadline,attempt,after=guard) {
  if(!await guard()||Date.now()>=deadline||await locator.count()!==1||!await locator.isEnabled())return false;
  const handle=await locator.elementHandle();if(!handle)return false;
  try {
    const first=await handle.evaluate(sampleEditor,'button');if(first.blocked)return false;
    await pause(Math.min(100,Math.max(0,deadline-Date.now())));
    if(!await guard()||!await locator.evaluate((e,held)=>e===held,handle))return false;
    const second=await handle.evaluate(sampleEditor,'button'),point=candidate(first,second);if(!point)return false;
    if(!await guard()||Date.now()>=deadline)return false;
    const final=await handle.evaluate(sampleEditor,'button');
    if(!candidate(first,final)||!final.points.some(p=>p.x===point.x&&p.y===point.y))return false;
    if(!await guard()||Date.now()>=deadline)return false;
    attempt();
    await handle.click({position:point,timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
    return await after()&&Date.now()<deadline;
  } finally {await handle.dispose();}
}

function directCDPPolicy(platform=process.platform,env=process.env) {
  return ['linux','darwin','win32'].includes(platform)&&env.GITHUB_ACTIONS==='true'
    &&env.RUNNER_ENVIRONMENT==='github-hosted'&&env.RUNNER_OS===({linux:'Linux',darwin:'macOS',win32:'Windows'}[platform])
    &&env.NANH_DESKTOP_RENDERER_APP==='chatgpt-desktop'&&env.NANH_CODEX_INPUT_CHANNEL==='cdp-dom';
}

function homeComposerScope() {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  const nodes=[...document.querySelectorAll('*')];
  if(nodes.length>4096)return false;
  const live=nodes.filter(visible);
  const homes=live.filter(e=>e.getAttribute('data-codex-composer-root')!==null
    &&e.getAttribute('data-composer-placement')==='home');
  const editors=live.filter(e=>e.getAttribute('contenteditable')==='true'||e.tagName==='TEXTAREA');
  return homes.length===1&&editors.length===1&&homes[0].contains(editors[0])
    &&editors[0].classList?.contains('ProseMirror')&&editors[0].getAttribute('contenteditable')==='true'
    &&editors[0].getAttribute('aria-disabled')!=='true'&&!editors[0].disabled&&!editors[0].readOnly
    &&!live.some(e=>['dialog','alertdialog','menu'].includes(e.getAttribute('role'))
      ||e.getAttribute('aria-modal')==='true'||e.getAttribute('data-thread-find-target')==='conversation');
}

async function runTurn(page,guard,request,deadline=Date.now()+request.timeoutMs) {
  const facts={schemaVersion:1,mechanism:'codex-renderer-qualification',diagnosticsOnly:true,
    endpointOwned:false,targetVerified:false,attached:true,bindingVerified:false,auxiliaryInert:false,
    codingComposerReady:false,uniqueComposer:false,inputReadback:false,inputSubmitted:false,
    userTurnObserved:false,assistantTurnCount:0,responseVerified:false,errorObserved:false,
    retryControl:false,retryAttempted:false,retryCompleted:false,errorCategory:null};
  let admissionFailure=null,readinessObservation=null,retryWitness=null;
  const stop=category=>{
    facts.errorCategory=category;
    if(category==='composer-unavailable') {
      if(admissionFailure)facts.composerAdmissionFailure=admissionFailure;
      if(readinessObservation)facts.composerReadinessObservation=readinessObservation;
    }
    return facts;
  };
  const owned=async()=>{
    if(Date.now()>=deadline||!await guard(deadline))return false;
    facts.endpointOwned=true;facts.targetVerified=true;facts.bindingVerified=true;facts.auxiliaryInert=true;
    return true;
  };
  try {
    if(!await owned())return stop('ownership-lost');
    let threadReady=false,homeReady=false;
    while(Date.now()<deadline) {
      threadReady=await page.evaluate(codingScope);
      homeReady=!threadReady&&request.action!=='retry'&&await page.evaluate(homeComposerScope);
      if(!await owned())return stop('ownership-lost');
      facts.codingComposerReady=threadReady||homeReady;
      if(facts.codingComposerReady)break;
      admissionFailure='scope-not-ready';
      const observed=await page.evaluate(codingScope,true);
      if(!await owned())return stop('ownership-lost');
      const publicDOM=observed?.publicDOM;
      if(publicDOM)readinessObservation={overflow:publicDOM.home.status!=='observed'||publicDOM.editable.status!=='observed',
        homeComposerCount:publicDOM.home.homeComposerCount,pendingTextareaCount:publicDOM.home.pendingTextareaCount,
        proseMirrorEditableCount:publicDOM.home.proseMirrorEditableCount,workspaceControlCount:publicDOM.home.workspaceControlCount,
        editableCount:publicDOM.editable.editableCount,codexThreadCount:publicDOM.editable.codexThreadCount,
        classicChatGPTCount:publicDOM.editable.classicChatGPTCount};
      await pause(Math.min(100,Math.max(0,deadline-Date.now())));
      if(Date.now()<deadline&&!await owned())return stop('ownership-lost');
    }
    if(!facts.codingComposerReady)return stop('composer-unavailable');
    admissionFailure=null;
    if(request.action==='ready')return facts;
    const composerSelector=homeReady?'[data-codex-composer-root][data-composer-placement="home"]':'[data-thread-find-composer]';
    const editor=page.locator(composerSelector+' .ProseMirror[contenteditable="true"]:visible');
    facts.uniqueComposer=await editor.count()===1;
    if(!facts.uniqueComposer){admissionFailure='nonunique-editor';return stop('composer-unavailable');}
    if(request.action==='submit') {
      const stale=await page.evaluate(turnObservation,{prompt:request.prompt,marker:request.expectedMarker});
      if(stale.userCount!==0||stale.responseVerified)return stop('stale-turn');
      if(!await owned())return stop('ownership-lost');
      const heldEditor=await editor.elementHandle();
      if(!heldEditor){admissionFailure='missing-editor';return stop('composer-unavailable');}
      try {
      const inputGuard=async filled=>await owned()&&await editor.count()===1
        &&await editor.evaluate((e,held)=>e===held,heldEditor)
        &&(!homeReady||await page.evaluate(homeComposerScope))
        &&await heldEditor.evaluate((e,text)=>e.isConnected&&e.textContent===text,filled?request.prompt:'');
      if(!await inputGuard(false))return stop('input-mismatch');
      const firstInput=await heldEditor.evaluate(sampleEditor);
      if(firstInput.blocked){admissionFailure=firstInput.blocked;return stop('composer-unavailable');}
      if(firstInput.points.length===0){admissionFailure='hit-unavailable';return stop('composer-unavailable');}
      const secondInput=await heldEditor.evaluate(sampleEditor);
      if(secondInput.blocked){admissionFailure=secondInput.blocked;return stop('composer-unavailable');}
      if(secondInput.points.length===0){admissionFailure='hit-unavailable';return stop('composer-unavailable');}
      if(!candidate(firstInput,secondInput)){admissionFailure='sample-changed';return stop('composer-unavailable');}
      if(!await inputGuard(false))return stop('ownership-lost');
      await editor.fill(request.prompt,{timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
      facts.inputReadback=await editor.evaluate((e,prompt)=>e.textContent===prompt,request.prompt);
      if(!facts.inputReadback)return stop('input-mismatch');
      if(!await inputGuard(true))return stop('ownership-lost');
      const send=page.locator(composerSelector).getByRole('button',{name:'Send',exact:true});
      // Streaming may still expose Stop when the response marker first appears.
      // Wait for ordinary Send without steering/queuing or changing the held draft.
      while(Date.now()<deadline) {
        if(!await inputGuard(true))return stop('ownership-lost');
        const count=await send.count();
        if(count>1)return stop('action-uncertain');
        if(count===1&&await send.isEnabled())break;
        await pause(Math.min(100,Math.max(0,deadline-Date.now())));
      }
      if(!await ordinaryClick(send,()=>inputGuard(true),deadline,()=>{facts.inputSubmitted=true;},owned))return stop('action-uncertain');
      } finally {await heldEditor.dispose();}
    } else {
      const current=await page.evaluate(turnObservation,{prompt:request.prompt,marker:request.expectedMarker});
      if(current.userCount!==1||current.responseVerified)return stop('stale-turn');
      // Only the latest failed local turn's public capacity Retry is eligible.
      const user=page.locator('[data-local-conversation-user-anchor]:visible')
        .filter({has:page.locator('[data-user-message-bubble]').filter({hasText:request.prompt})});
      if(await user.count()!==1||!await user.evaluate((e,prompt)=>e.querySelector('[data-user-message-bubble]')?.innerText.trim()===prompt,request.prompt))return stop('stale-turn');
      const turn=user.locator('xpath=ancestor::*[@data-turn-key][1]');
      const retry=turn.getByRole('button',{name:/^Retry(?: in [1-9][0-9]*s)?$/,exact:true});
      facts.retryControl=await retry.count()===1&&await retry.isEnabled();
      if(!facts.retryControl)return stop('retry-unavailable');
      retryWitness=await page.evaluateHandle(holdRetryContinuation,{prompt:request.prompt});
      const retryOwned=async()=>await owned()&&(await page.evaluate(retryContinuationObservation,{held:retryWitness,prompt:request.prompt,marker:request.expectedMarker})).userCount===1;
      if(!await ordinaryClick(retry,retryOwned,deadline,()=>{facts.retryAttempted=true;}))return stop('action-uncertain');
      facts.retryCompleted=true;
    }
    while(Date.now()<deadline) {
      if(!await owned())return stop('ownership-lost');
      const current=await page.evaluate(retryWitness?retryContinuationObservation:turnObservation,{...(retryWitness?{held:retryWitness}:{}),prompt:request.prompt,marker:request.expectedMarker});
      if(!await owned())return stop('ownership-lost');
      facts.userTurnObserved=current.userCount===1;facts.assistantTurnCount=current.assistantCount;
      if(current.responseVerified) {facts.responseVerified=true;return facts;}
      if(request.purpose==='failure'&&facts.userTurnObserved) {
        const user=page.locator('[data-local-conversation-user-anchor]:visible')
          .filter({has:page.locator('[data-user-message-bubble]').filter({hasText:request.prompt})});
        if(await user.count()===1) {
          const retry=user.locator('xpath=ancestor::*[@data-turn-key][1]')
            .getByRole('button',{name:/^Retry(?: in [1-9][0-9]*s)?$/,exact:true});
          facts.retryControl=await retry.count()===1&&await retry.isEnabled();
          if(facts.retryControl&&await owned()) {facts.errorObserved=true;return facts;}
        }
      }
      await pause(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    return stop('response-timeout');
  } catch {return stop(facts.inputSubmitted||facts.retryAttempted?'action-uncertain':'query-failed');}
  finally {if(retryWitness)await retryWitness.dispose().catch(()=>{});}
}

exports.directCDPPolicy=directCDPPolicy;
exports.homeComposerScope=homeComposerScope;
exports.turnObservation=turnObservation;
exports.holdRetryContinuation=holdRetryContinuation;
exports.retryContinuationObservation=retryContinuationObservation;
exports.ordinaryClick=ordinaryClick;
exports.runTurn=runTurn;
exports.sampleEditor=sampleEditor;

async function pageIdentity(page,deadline) {
  const bounded=async promise=>{
    let timer;
    try{return await Promise.race([promise,new Promise((_,reject)=>{
      timer=setTimeout(()=>reject(Error('deadline')),Math.max(1,deadline-Date.now()));
    })]);}finally{clearTimeout(timer);}
  };
  let session;
  try {
    if(Date.now()>=deadline)throw Error('deadline');
    session=await bounded(page.context().newCDPSession(page));
    const target=await bounded(session.send('Target.getTargetInfo'));
    const tree=await bounded(session.send('Page.getFrameTree'));
    const frame=tree.frameTree?.frame;
    if(!frame?.loaderId||!target.targetInfo?.targetId)throw Error('identity');
    return {url:page.url(),target:target.targetInfo.targetId,frame:frame.id,
      loader:frame.loaderId,frameUrl:frame.url,fragment:frame.urlFragment??''};
  } finally {if(session)await session.detach().catch(()=>{});}
}
function sameIdentity(a,b) {
  return ['url','target','frame','loader','frameUrl','fragment'].every(key=>a[key]===b[key]);
}
function auxiliaryScope() {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const controls=[...document.querySelectorAll('textarea,[contenteditable="true"],input:not([type="hidden"]),[role="dialog"],[aria-modal="true"],[role="alertdialog"],[role="menu"]')].filter(visible);
  return !document.hasFocus()&&controls.length===0;
}
async function bindRecordedMain(browser,binding,owner,deadline,identity=pageIdentity,directCDP=false) {
  if(Date.now()>=deadline||!owner())return null;
  const pages=browser.contexts().flatMap(context=>context.pages());
  if(pages.length!==1+(binding.auxiliary?1:0))return null;
  const identities=await Promise.all(pages.map(page=>identity(page,deadline)));
  if(Date.now()>=deadline||!owner())return null;
  const mainMatches=identities.map((value,index)=>sameIdentity(binding.main,value)?index:-1).filter(index=>index>=0);
  if(mainMatches.length!==1)return null;
  const main=pages[mainMatches[0]];
  const auxiliary=pages.find(page=>page!==main);
  if(auxiliary) {
    const aux=identities[pages.indexOf(auxiliary)];
    if(!sameIdentity(binding.auxiliary,aux)||sourceRoute(aux.url)!=='avatarOverlay'
      ||!await auxiliary.evaluate(auxiliaryScope))return null;
  }
  if(!await main.evaluate(scoped=>document.visibilityState==='visible'&&(scoped||document.hasFocus()),directCDP))return null;
  const after=browser.contexts().flatMap(context=>context.pages());
  if(Date.now()>=deadline||!owner()||after.length!==pages.length||!after.every(page=>pages.includes(page)))return null;
  return main;
}
function validRequest(request) {
  const keys=['connectionPath','ownerPid','prompt','expectedMarker','timeoutMs','action','purpose','mainBindingPath'];
  return request&&Object.keys(request).length===keys.length&&keys.every(key=>Object.hasOwn(request,key))
    &&Number.isSafeInteger(request.ownerPid)&&request.ownerPid>1
    &&typeof request.connectionPath==='string'&&typeof request.mainBindingPath==='string'
    &&['ready','submit','retry'].includes(request.action)&&['response','failure'].includes(request.purpose)
    &&(request.action!=='ready'||request.purpose==='response')
    &&(request.purpose!=='failure'||request.action==='submit'&&request.expectedMarker==='NAN_CHECK_EXPECTED_FAILURE')
    &&['Check this connection','Read read-target.txt using your file tool.','Check the expected provider failure'].includes(request.prompt)
    &&typeof request.expectedMarker==='string'&&request.expectedMarker.length>=1&&request.expectedMarker.length<=2048
    &&Number.isSafeInteger(request.timeoutMs)&&request.timeoutMs>0&&request.timeoutMs<=45000;
}
function readPrivate(fs,path,file,root,basename) {
  const metadata=fs.lstatSync(file),canonicalRoot=fs.realpathSync(root);
  if(!metadata.isFile()||metadata.isSymbolicLink()||metadata.size>8192
    ||path.relative(canonicalRoot,fs.realpathSync(file))!==basename
    ||process.platform!=='win32'&&(metadata.mode&0o077)!==0)throw Error('private-file');
  return JSON.parse(fs.readFileSync(file,'utf8'));
}
function validBinding(binding,request,connection) {
  const keys=['schemaVersion','main','auxiliary','ownerPid','launcherPid','port'];
  const identity=value=>value&&Object.keys(value).length===6
    &&['url','target','frame','loader','frameUrl','fragment'].every(key=>typeof value[key]==='string'&&value[key].length<=4096)
    &&['target','frame','loader'].every(key=>value[key].length>0&&value[key].length<=256)
    &&value.url.startsWith('app://-/');
  return binding&&Object.keys(binding).length===keys.length&&keys.every(key=>Object.hasOwn(binding,key))
    &&binding.schemaVersion===1&&binding.ownerPid===request.ownerPid
    &&binding.launcherPid===connection.launcherPid&&binding.port===connection.port
    &&identity(binding.main)&&(binding.auxiliary===null||identity(binding.auxiliary)
      &&sourceRoute(binding.auxiliary.url)==='avatarOverlay');
}
async function main() {
  const fs=require('node:fs'),path=require('node:path');
  const output=process.argv[4];
  let browser,facts={schemaVersion:1,mechanism:'codex-renderer-qualification',diagnosticsOnly:true,
    endpointOwned:false,targetVerified:false,attached:false,bindingVerified:false,auxiliaryInert:false,
    codingComposerReady:false,uniqueComposer:false,inputReadback:false,inputSubmitted:false,
    userTurnObserved:false,assistantTurnCount:0,responseVerified:false,errorObserved:false,
    retryControl:false,retryAttempted:false,retryCompleted:false,errorCategory:'invalid-request'};
  let preAttachFailure='request-json';
  try {
    const request=JSON.parse(fs.readFileSync(process.argv[3],'utf8'));
    preAttachFailure='request-policy';
    if(process.argv[2]!=='--qualify'||!validRequest(request)
      ||process.env.GITHUB_ACTIONS!=='true'||process.env.RUNNER_ENVIRONMENT!=='github-hosted'
      ||process.env.NANH_DESKTOP_RENDERER_APP!=='chatgpt-desktop')throw Error('policy');
    const root=path.dirname(process.argv[3]);
    preAttachFailure='connection-read';
    const connection=readPrivate(fs,path,request.connectionPath,root,`connection-${request.ownerPid}.json`);
    preAttachFailure='binding-read';
    const binding=readPrivate(fs,path,request.mainBindingPath,root,`main-binding-${request.ownerPid}.private`);
    preAttachFailure='connection-schema';
    if(!connection||Object.keys(connection).length!==3||connection.schemaVersion!==1
      ||!Number.isSafeInteger(connection.port)||connection.port<1025||connection.port>65535
      ||!Number.isSafeInteger(connection.launcherPid)||connection.launcherPid<2)throw Error('connection');
    preAttachFailure='binding-schema';
    if(!validBinding(binding,request,connection))throw Error('binding');
    facts.errorCategory='ownership-lost';
    const rootProof=require('./endpoint-ownership.cjs').proof(String(request.ownerPid),String(connection.port));
    const endpoint=require('./endpoint-ownership.cjs').proof(String(connection.launcherPid),String(connection.port));
    const owner=()=>rootProof.descendant(connection.launcherPid)&&endpoint.ownedEndpoint();
    if(!owner())throw Error('owner');
    const {chromium}=require('../../.github/web-check/node_modules/playwright');
    const deadline=Date.now()+request.timeoutMs;
    browser=await chromium.connectOverCDP(`http://127.0.0.1:${connection.port}`,{timeout:2000,noDefaults:true});
    const directCDP=directCDPPolicy();
    const page=await bindRecordedMain(browser,binding,owner,deadline,pageIdentity,directCDP);
    if(!page)throw Error('main');
    facts=await runTurn(page,async until=>await bindRecordedMain(browser,binding,owner,until,pageIdentity,directCDP)===page,request,deadline);
  } catch {
    if(facts.errorCategory==='invalid-request')facts.preAttachFailure=preAttachFailure;
    if(facts.errorCategory===null)facts.errorCategory='query-failed';
  }
  finally {if(browser)await browser.close().catch(()=>{});}
  fs.writeFileSync(output,JSON.stringify(facts)+'\n',{mode:0o600,flag:'wx'});
  process.exitCode=facts.errorCategory===null?0:1;
}
exports.bindRecordedMain=bindRecordedMain;
exports.sameIdentity=sameIdentity;
exports.validBinding=validBinding;
exports.validRequest=validRequest;
exports.readPrivate=readPrivate;
if(require.main===module)main().catch(()=>{process.exitCode=1;});
