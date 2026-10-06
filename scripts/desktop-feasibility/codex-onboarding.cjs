const {isHeldMainGuard}=require('./codex-main-guard.cjs');
// Public UI only. This module never reads accounts or changes application stores.
const TOKENS = ['relative', 'flex', 'h-full', 'min-h-0', 'w-full', 'flex-col',
  'bg-transparent', 'tracking-normal', 'text-default', 'select-text'];
const SCOPE = 'div' + TOKENS.map(token => `[class~="${token}"]`).join('');
const GROUP = 'input[type="radio"][name="conversational-onboarding-inline-role"]';
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));

// Standalone browser callback: no Node helpers or application internals.
function sample(control, optionalSkip=false) {
  const blocked=reason=>({blocked:reason});
  if (!['LABEL','BUTTON'].includes(control.tagName)) return blocked('unsupported-control');
  if (!control.isConnected || control.ownerDocument !== document || control.closest('[inert]')) return blocked('detached-or-inert');
  const visible = e => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.display !== 'none' && s.visibility !== 'hidden'; };
  const overlays=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"],[role="menu"]')].filter(visible);
  if (overlays.length>1) return blocked('ambiguous-overlays');
  if (overlays.length===1) {
    const dialog=overlays[0];
    // Native project entry can present the same verified public role form in
    // a dialog. Only its own enclosing dialog can admit an ordinary click.
    if (dialog.getAttribute('role')!=='dialog' || !dialog.contains(control)
        || dialog.querySelectorAll('input[type="radio"][name="conversational-onboarding-inline-role"][value="engineering"]').length!==1
        || [...dialog.querySelectorAll('fieldset > legend')].filter(e=>visible(e)&&e.innerText.trim()==='Select the kind of work you do').length!==1) {
      const acknowledgement='Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.';
      const acknowledgements=[...dialog.querySelectorAll('*')].filter(e=>visible(e)&&e.textContent?.trim()===acknowledgement
        &&![...e.children].some(child=>child.textContent?.trim()===acknowledgement));
      if(dialog.getAttribute('role')!=='dialog'||!dialog.contains(control)||control.tagName!=='BUTTON'
          ||control.textContent?.trim()!==(optionalSkip?'Skip':'Get Started')||acknowledgements.length!==1
          ||dialog.querySelectorAll('input[name="conversational-onboarding-inline-role"]').length!==0
          ||[...dialog.querySelectorAll('button')].filter(e=>visible(e)&&e.textContent?.trim()===(optionalSkip?'Skip':'Get Started')).length!==1) return blocked('foreign-overlay');
    }
  }
  for (let e = control, depth = 0; e; e = e.parentElement) {
    if (++depth > 64 || getComputedStyle(e).pointerEvents === 'none') return blocked('pointer-disabled');
  }
  if (!visible(control)) return blocked('hidden');
  if (control.disabled || control.getAttribute('aria-disabled') === 'true') return blocked('disabled');
  const r = control.getBoundingClientRect();
  const points = [];
  for (const fy of [0.25, 0.5, 0.75]) for (const fx of [0.25, 0.5, 0.75]) {
    const x = r.left + control.clientLeft + control.clientWidth * fx;
    const y = r.top + control.clientTop + control.clientHeight * fy;
    if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue;
    const front = document.elementFromPoint(x, y);
    if (front === control || control.contains(front)) points.push({x: x-r.left-control.clientLeft, y:y-r.top-control.clientTop});
  }
  return {rect: [r.left,r.top,r.width,r.height], points};
}

// Frozen Qf/$d plain skip confirmation. Credit variants are deliberately unproved.
function skipConfirmation(control, retained) {
  const rejected=reason=>retained?.diagnostic?{rejection:reason}:null;
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const overlays=[...document.querySelectorAll('[role="dialog"],[role="alertdialog"],[role="menu"],[aria-modal="true"]')].filter(visible);
  if(overlays.length!==1)return rejected('overlay-count');
  const dialog=overlays[0],forms=[...dialog.querySelectorAll('form.select-none')].filter(visible);
  if(dialog.getAttribute('role')!=='dialog'||forms.length!==1)return rejected('form');
  const form=forms[0];
  if(retained&&(retained.form!==form||retained.dialog!==dialog))return rejected('retained-identity');
  const leaf=text=>[...form.querySelectorAll('*')].filter(e=>e.textContent?.trim()===text
    &&![...e.children].some(c=>c.textContent?.trim()===text)&&visible(e));
  const titles=[...form.querySelectorAll('h1,h2,h3,[role="heading"]')].filter(e=>e.isConnected
    &&e.textContent?.trim()==='Skip setup?'&&getComputedStyle(e).display!=='none'
    &&getComputedStyle(e).visibility!=='hidden');
  const buttons=[...form.querySelectorAll('button')].filter(visible);
  const keep=buttons.filter(e=>e.textContent?.trim()==='Keep setting up'&&e.getAttribute('type')==='submit');
  const go=buttons.filter(e=>e.textContent?.trim()==='Go to ChatGPT'&&e.getAttribute('type')==='button');
  if(titles.length!==1)return rejected('heading');
  if(leaf('You’ll go straight to ChatGPT').length!==1)return rejected('subtitle');
  if(buttons.length!==2
      ||keep.length!==1||go.length!==1||go[0]!==control||control.disabled
      ||control.getAttribute('aria-disabled')==='true'||control.closest('[inert]'))return rejected('source-controls');
  // The frozen modal disables outside pointer events on body. Its retained
  // dialog restores them; prove this scope and the actual hit target below.
  let reachedDialog=false;
  for(let e=control,depth=0;e;e=e.parentElement) {
    if(++depth>64||getComputedStyle(e).pointerEvents==='none')return rejected('pointer-ancestry');
    if(e===dialog){reachedDialog=true;break;}
  }
  if(!reachedDialog)return rejected('pointer-ancestry');
  const r=control.getBoundingClientRect(),points=[];
  for(const fy of [0.25,0.5,0.75])for(const fx of [0.25,0.5,0.75]) {
    const x=r.left+control.clientLeft+control.clientWidth*fx,y=r.top+control.clientTop+control.clientHeight*fy;
    if(x<0||y<0||x>=innerWidth||y>=innerHeight)continue;
    const front=document.elementFromPoint(x,y);
    if(front===control||control.contains(front))points.push({x:x-r.left-control.clientLeft,y:y-r.top-control.clientTop});
  }
  return {rect:[r.left,r.top,r.width,r.height],points};
}

// The frozen role Continue callback transitions into task setup. Disappearance
// alone is not proof: retain its source scope and exact Engineering acknowledgement.
function taskContinuation(scope, diagnostic=false) {
  const acknowledgement='Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.';
  const visible=e=>{const r=e.getBoundingClientRect(),style=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&style.display!=='none'&&style.visibility!=='hidden';};
  const connected=scope.ownerDocument===document&&scope.isConnected;
  const showing=connected&&visible(scope)&&!scope.closest('[inert]');
  const result={heldScopeConnected:connected,heldScopeVisible:showing,
    roleRadioCount:null,exactAckLeafCount:null,exactGetStartedCount:null};
  if(!showing)return diagnostic?result:false;
  const radios=scope.querySelectorAll('input[name="conversational-onboarding-inline-role"]');
  const nodes=[...scope.querySelectorAll('*')];
  const buttons=[...scope.querySelectorAll('button')];
  if(radios.length>4096||nodes.length>4096||buttons.length>4096)return diagnostic?result:false;
  const acknowledgementNodes=nodes.filter(e=>visible(e)
    &&e.textContent.trim()===acknowledgement
    &&![...e.children].some(child=>child.textContent.trim()===acknowledgement));
  const start=buttons.filter(e=>visible(e)&&e.textContent.trim()==='Get Started');
  result.roleRadioCount=radios.length;
  result.exactAckLeafCount=acknowledgementNodes.length;
  result.exactGetStartedCount=start.length;
  return diagnostic?result:radios.length===0&&acknowledgementNodes.length===1&&start.length===1;
}

// Qf's optional-capability footer from the pinned platform 26.930.41038 assets.
// This is the ordinary Skip callback; never a permission grant or task replay.
function optionalCapabilities(scope) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  if(scope.ownerDocument!==document||!visible(scope))return false;
  const nodes=[...scope.querySelectorAll('*')],buttons=[...scope.querySelectorAll('button')];
  if(nodes.length>4096||buttons.length>4096||scope.querySelectorAll('input[name="conversational-onboarding-inline-role"]').length!==0)return false;
  const ack='Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.';
  // The source task phase can replace the role acknowledgement. Its retained
  // footer and ordinary Skip remain; Get Started still requires its acknowledgement.
  if(nodes.filter(e=>visible(e)&&e.textContent?.trim()===ack&&![...e.children].some(c=>c.textContent?.trim()===ack)).length>1)return false;
  if(buttons.some(e=>visible(e)&&e.textContent?.trim()==='Get Started'))return false;
  const skips=buttons.filter(e=>visible(e)&&e.textContent?.trim()==='Skip');
  if(skips.length!==1||skips[0].disabled||skips[0].getAttribute('aria-disabled')==='true')return false;
  const footer=skips[0].parentElement?.parentElement;
  return !!footer&&nodes.includes(skips[0])&&nodes.includes(footer)&&scope.contains(footer)&&footer.tagName==='DIV'
    &&['relative','flex','shrink-0','flex-col','items-center','gap-3','px-10','pt-8','pb-12'].every(t=>footer.classList.contains(t));
}

// Exact public local-coding markers from the frozen local conversation thread.
function codingScope(diagnostic=false) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  const all=selector=>[...document.querySelectorAll(selector)].filter(visible);
  const editors=all('[data-thread-find-composer] .ProseMirror[contenteditable="true"]');
  const conversations=all('[data-thread-find-target="conversation"]');
  const modals=all('[role="dialog"],[role="alertdialog"],[role="menu"],[aria-modal="true"]');
  const ready=editors.length===1&&conversations.length===1&&modals.length===0
    &&editors[0].getAttribute('aria-disabled')!=='true';
  if(!diagnostic)return ready;
  const ack='Engineering—got it. I can map an unfamiliar codebase, plan and build features, trace bugs across logs and tests, and run checks to verify behavior.';
  const nodes=[...document.querySelectorAll('*')],buttons=all('button');
  const counts={composerCount:editors.length,conversationCount:conversations.length,modalCount:modals.length,
    roleRadioCount:all('input[name="conversational-onboarding-inline-role"]').length,
    exactAckLeafCount:nodes.length<=4096?nodes.filter(e=>visible(e)&&e.textContent?.trim()===ack
      &&![...e.children].some(c=>c.textContent?.trim()===ack)).length:null,
    exactGetStartedCount:buttons.filter(e=>e.textContent?.trim()==='Get Started').length,
    exactSkipCount:buttons.filter(e=>e.textContent?.trim()==='Skip').length};
  // Passive source labels are not navigation authority. Keep this in the same DOM read.
  const controls=nodes.length<=4096?nodes.filter(e=>(['BUTTON','A'].includes(e.tagName)
    ||e.getAttribute('role')==='menuitem')&&visible(e)):[];
  const name=e=>e.getAttribute('aria-label')??e.textContent?.trim();
  const codex=controls.filter(e=>name(e)==='Codex');
  const role=e=>e.getAttribute('role')??(e.tagName==='BUTTON'?'button':e.tagName==='A'?'link':'unknown');
  const unique=codex.length===1?codex[0]:null;
  let hit=false;
  if(unique) {
    const r=unique.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
    if(Number.isFinite(x)&&Number.isFinite(y)&&x>=0&&y>=0
        &&x<document.documentElement.clientWidth&&y<document.documentElement.clientHeight) {
      const target=document.elementFromPoint(x,y);
      hit=!!target&&(target===unique||unique.contains(target))&&!unique.disabled
        &&unique.getAttribute('aria-disabled')!=='true';
    }
  }
  const navigationCounts={codexButtonCount:codex.filter(e=>role(e)==='button').length,
    codexLinkCount:codex.filter(e=>role(e)==='link').length,
    codexMenuItemCount:codex.filter(e=>role(e)==='menuitem').length,
    chatModeTriggerCount:controls.filter(e=>name(e)==='Switch mode, current mode: ChatGPT'
      ||name(e)==='Switch mode, current mode: ChatGPT Work').length,
    codexModeTriggerCount:controls.filter(e=>name(e)==='Switch mode, current mode: Codex').length,
    projectSelectorCount:controls.filter(e=>name(e)==='Select project').length,
    newChatCount:controls.filter(e=>name(e)==='New chat').length,
    projectsLinkCount:controls.filter(e=>role(e)==='link'&&name(e)==='Projects').length};
  const navigationComplete=nodes.length<=4096&&Object.values(navigationCounts).every(n=>n<=32)
    &&codex.every(e=>['button','link','menuitem'].includes(role(e)));
  const navigation={status:navigationComplete?'observed':'overflow',
    sourceVersion:'26.930.41038',sourceSha256:'28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d',
    ...Object.fromEntries(Object.keys(navigationCounts).map(k=>[k,navigationComplete?navigationCounts[k]:null])),
    uniqueCodexRole:navigationComplete?(unique?role(unique):'none'):null,
    uniqueCodexHitActionable:navigationComplete?hit:null};
  // Frozen Linux Work-home local composer; counters are advisory only.
  const homeNodes=nodes.length<=4096?nodes:[];
  const homeRoots=homeNodes.filter(e=>visible(e)&&e.getAttribute('data-testid')==='chatgpt-work-home-page');
  const localHomes=homeRoots.length<=32?homeNodes.filter(e=>visible(e)&&e.getAttribute('data-codex-composer-root')!==null
    &&e.getAttribute('data-composer-placement')==='home'&&homeRoots.some(root=>root.contains(e))):[];
  const homeEditors=localHomes.length<=32?homeNodes.filter(e=>visible(e)&&e.getAttribute('contenteditable')==='true'
    &&localHomes.some(root=>root.contains(e))):[];
  const homeCounts={homeRootCount:homeRoots.length,localHomeComposerCount:localHomes.length,
    homeEditableCount:homeEditors.length,
    homeProseMirrorCount:homeEditors.filter(e=>e.classList?.contains('ProseMirror')).length,
    workspaceControlCount:controls.filter(e=>e.getAttribute('data-composer-navigation-target')==='workspace-project'
      &&homeRoots.some(root=>root.contains(e))).length};
  const homeComplete=nodes.length<=4096&&Object.values(homeCounts).every(n=>n<=32);
  const home={status:homeComplete?'observed':'overflow',sourcePlatform:'linux',sourceVersion:'26.930.41038',
    composerSourceSha256:'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
    pageSourceSha256:'9c9d0d9247226d43edeb4606a539b518e3be06fb65aa37bd014984fbe3998ba9',
    ...Object.fromEntries(Object.keys(homeCounts).map(k=>[k,homeComplete?homeCounts[k]:null]))};
  // Partition visible editables by frozen source ancestry; never grant input.
  const ancestryCounts={editableCount:0,codexHomeCount:0,codexThreadCount:0,codexOtherCount:0,
    classicChatGPTCount:0,genericInputCount:0,genericBodyCount:0,unboundCount:0,
    sidebarNewChatCount:0};
  let ancestryComplete=nodes.length<=4096;
  for(const editor of nodes.filter(e=>visible(e)&&(e.getAttribute('contenteditable')==='true'
      ||e.tagName==='TEXTAREA')&&!e.disabled&&!e.readOnly)) {
    ancestryCounts.editableCount++;
    const chain=[],seen=new Set();let current=editor;
    while(current&&chain.length<64&&!seen.has(current)) {
      seen.add(current);chain.push(current);current=current.parentElement;
    }
    if(current) {ancestryComplete=false;break;}
    const codex=chain.find(e=>e.getAttribute('data-codex-composer-root')!==null);
    const placement=codex?.getAttribute('data-composer-placement');
    const category=codex?(placement==='home'?'codexHomeCount':placement==='thread'?'codexThreadCount':'codexOtherCount'):
      chain.some(e=>e.getAttribute('data-chatgpt-composer')!==null)?'classicChatGPTCount':
      chain.some(e=>e.getAttribute('data-composer-input')!==null)?'genericInputCount':
      chain.some(e=>e.getAttribute('data-composer-body')!==null)?'genericBodyCount':'unboundCount';
    ancestryCounts[category]++;
  }
  const sidebar=controls.filter(e=>e.tagName==='BUTTON'&&e.getAttribute('type')==='button'
    &&e.classList?.contains('sidebar-item')&&name(e)==='New chat');
  ancestryCounts.sidebarNewChatCount=sidebar.length;
  let sidebarHit=false;
  if(sidebar.length===1) {
    const button=sidebar[0],r=button.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
    if(Number.isFinite(x)&&Number.isFinite(y)&&x>=0&&y>=0
        &&x<document.documentElement.clientWidth&&y<document.documentElement.clientHeight) {
      const target=document.elementFromPoint(x,y);
      sidebarHit=!!target&&(target===button||button.contains(target))&&!button.disabled
        &&button.getAttribute('aria-disabled')!=='true';
    }
  }
  ancestryComplete=ancestryComplete&&Object.values(ancestryCounts).every(n=>n<=32);
  const ancestry={status:ancestryComplete?'observed':'overflow',sourcePlatform:'linux',sourceVersion:'26.930.41038',
    initialSourceSha256:'28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d',
    composerSourceSha256:'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
    ...Object.fromEntries(Object.keys(ancestryCounts).map(k=>[k,ancestryComplete?ancestryCounts[k]:null])),
    sidebarNewChatHitActionable:ancestryComplete?sidebarHit:null};
  const homeStateNodes=nodes.length<=4096?nodes:[];
  const sourceHomes=homeStateNodes.filter(e=>visible(e)&&e.getAttribute('data-codex-composer-root')!==null
    &&e.getAttribute('data-composer-placement')==='home');
  const inHome=e=>sourceHomes.some(root=>root.contains(e));
  const homeStateCounts={homeComposerCount:sourceHomes.length,
    pendingTextareaCount:homeStateNodes.filter(e=>visible(e)&&inHome(e)&&e.tagName==='TEXTAREA'
      &&e.getAttribute('data-pending-input-initialized')==='true').length,
    pendingGroupCount:homeStateNodes.filter(e=>visible(e)&&inHome(e)&&e.classList?.contains('group/pending-composer')).length,
    proseMirrorEditableCount:homeStateNodes.filter(e=>visible(e)&&inHome(e)&&e.getAttribute('contenteditable')==='true'
      &&e.classList?.contains('ProseMirror')).length,
    enabledSendCount:controls.filter(e=>inHome(e)&&e.tagName==='BUTTON'&&name(e)==='Send'
      &&!e.disabled&&e.getAttribute('aria-disabled')!=='true').length,
    disabledSendCount:controls.filter(e=>inHome(e)&&e.tagName==='BUTTON'&&name(e)==='Send'
      &&(e.disabled||e.getAttribute('aria-disabled')==='true')).length,
    workspaceControlCount:controls.filter(e=>inHome(e)&&e.getAttribute('data-composer-navigation-target')==='workspace-project').length};
  const homeStateComplete=nodes.length<=4096&&Object.values(homeStateCounts).every(n=>n<=32);
  const homeState={status:homeStateComplete?'observed':'overflow',sourcePlatform:'linux',sourceVersion:'26.930.41038',
    composerSourceSha256:'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
    ...Object.fromEntries(Object.keys(homeStateCounts).map(k=>[k,homeStateComplete?homeStateCounts[k]:null]))};
  const complete=Object.values(counts).every(n=>Number.isInteger(n)&&n>=0&&n<=32);
  // Public DOM counters carry no platform source pins and never admit input.
  const publicShape=value=>Object.fromEntries(Object.entries(value).filter(([key])=>
    key==='status'||key.endsWith('Count')||key.endsWith('HitActionable')||key==='uniqueCodexRole'));
  const publicDOM={navigation:publicShape(navigation),editable:publicShape(ancestry),home:publicShape(homeState)};
  return {ready,navigation,home,ancestry,homeState,publicDOM,observation:{status:complete?'observed':'overflow',
    ...Object.fromEntries(Object.keys(counts).map(k=>[k,complete?counts[k]:null]))}};
}

// Exact immutable final-onboarding surface. No arbitrary app payload is returned.
function foreignSurface(control) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"],[role="menu"]')].filter(visible);
  const tokens=['relative','flex','h-full','min-h-0','w-full','flex-col','bg-transparent','tracking-normal','text-default','select-text'];
  const scope=control.closest('div'+tokens.map(t=>`[class~="${t}"]`).join(''));
  return {document,scope,dialog:dialogs.length===1?dialogs[0]:null};
}
function classifyForeign(control,held) {
  let surface='unknown', heading='unknown', importSetup=null, sourceCounts=null, actionability=null;
  const result=(category,proof='classified',fingerprint='not-applicable')=>({category,proof,surface,fingerprint,heading,importSetup,sourceCounts,actionability});
  const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  if(control.ownerDocument!==document||!control.isConnected||held.document!==document) return result('guard-rejected','document-replaced');
  if(!visible(held.scope)||!held.scope.contains(control))return result('guard-rejected','scope-missing');
  const group='input[type="radio"][name="conversational-onboarding-inline-role"][value="engineering"]';
  if(held.scope.querySelectorAll(group).length!==1
      ||[...held.scope.querySelectorAll('fieldset > legend')].filter(e=>visible(e)&&e.innerText.trim()==='Select the kind of work you do').length!==1)return result('guard-rejected','role-group-changed');
  const dialogs=[...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"],[role="menu"]')].filter(visible);
  if(dialogs.length!==1)return dialogs.length>1?result('ambiguous'):result('guard-rejected','dialog-absent');
  const dialog=dialogs[0];
  if(dialog!==held.dialog)return result('guard-rejected','dialog-replaced');
  // Diagnostic only: measure the same held overlay and control, never admit input.
  actionability=(()=>{
    const blank={status:'unavailable',unavailableReason:null,dialogOpacityZero:null,ancestorOpacityZero:null,
      dialogPointerEventsNone:null,ancestorPointerEventsNone:null,inert:null,stateClosed:null,
      targetOwnedPointCount:null,dialogOwnedPointCount:null,otherPointCount:null};
    const unavailable=reason=>({...blank,unavailableReason:reason});
    const measured={...blank,status:'observed',dialogOpacityZero:false,ancestorOpacityZero:false,
      dialogPointerEventsNone:false,ancestorPointerEventsNone:false,inert:false,stateClosed:false};
    let e=dialog,depth=0;
    while(e) {
      if(++depth>64)return unavailable('ancestor-limit');
      if(e.ownerDocument!==document||!e.isConnected)return unavailable('ancestor-detached');
      const style=getComputedStyle(e),raw=style.opacity;
      if(typeof raw!=='string'||! /^(?:0(?:\.\d+)?|1(?:\.0+)?)$/.test(raw))return unavailable('opacity-invalid');
      const opacity=Number(raw);
      if(!Number.isFinite(opacity)||opacity<0||opacity>1)return unavailable('opacity-invalid');
      if(e===dialog) {measured.dialogOpacityZero=opacity===0;measured.dialogPointerEventsNone=style.pointerEvents==='none';}
      else {measured.ancestorOpacityZero ||= opacity===0;measured.ancestorPointerEventsNone ||= style.pointerEvents==='none';}
      if(typeof style.pointerEvents!=='string'||!style.pointerEvents)return unavailable('pointer-property-invalid');
      measured.inert ||= e.inert===true||e.hasAttribute('inert');
      measured.stateClosed ||= e.getAttribute('data-state')==='closed';
      e=e.parentElement;
    }
    const r=control.getBoundingClientRect(),values=[r.left,r.top,r.width,r.height,
      control.clientLeft,control.clientTop,control.clientWidth,control.clientHeight,innerWidth,innerHeight];
    if(values.some(v=>!Number.isFinite(v))||r.width<=0||r.height<=0
        ||control.clientWidth<=0||control.clientHeight<=0||innerWidth<=0||innerHeight<=0
        ||values.some(v=>Math.abs(v)>16384)||control.clientLeft<0||control.clientTop<0)return unavailable('geometry-invalid');
    const left=r.left+control.clientLeft,top=r.top+control.clientTop;
    // Client dimensions are integers; DOMRect dimensions can be fractional.
    // Passive samples stay inside both measured boxes rather than treating a
    // rounded client edge as proof that the target lies outside its rectangle.
    const width=Math.min(control.clientWidth,r.width-control.clientLeft);
    const height=Math.min(control.clientHeight,r.height-control.clientTop);
    if(width<=0||height<=0)return unavailable('geometry-invalid');
    if(left<0||top<0||left+width>innerWidth||top+height>innerHeight)return unavailable('geometry-outside');
    measured.targetOwnedPointCount=0;measured.dialogOwnedPointCount=0;measured.otherPointCount=0;
    for(const x of [1/6,1/2,5/6])for(const y of [1/6,1/2,5/6]) {
      const front=document.elementFromPoint(left+x*width,top+y*height);
      if(front&&(front===control||control.contains(front)))measured.targetOwnedPointCount++;
      else if(front&&(front===dialog||dialog.contains(front)))measured.dialogOwnedPointCount++;
      else measured.otherPointCount++;
    }
    return measured;
  })();
  // Public heading text is classified independently of the stronger style fingerprint.
  const publicHeadings=[...dialog.querySelectorAll('[role="heading"],h1,h2,h3')].filter(visible);
  const known=new Map([["You're all set",'all-set'],['Import from other AI apps','external-import'],['Skip setup?','skip-confirmation'],['Continue with your existing setup','imported-setup'],['Connect Computer History','computer-history-consent'],['Select settings to import','project-import']]);
  const matches=publicHeadings.map(e=>known.get(e.innerText.trim())).filter(Boolean);
  heading=matches.length>1?'ambiguous':matches[0]??'unknown';
  const setupTitleCount=publicHeadings.filter(e=>e.innerText.trim()==='Continue with your existing setup').length;
  if(setupTitleCount>0) {
    const setupButtons=[...dialog.querySelectorAll('button')].filter(visible);
    const counts={titleCount:setupTitleCount,
      continueCount:setupButtons.filter(e=>e.innerText.trim()==='Continue').length,
      notNowCount:setupButtons.filter(e=>e.innerText.trim()==='Not now').length,
      skipCount:setupButtons.filter(e=>e.innerText.trim()==='Skip').length};
    if(Object.values(counts).every(count=>count<=32))importSetup=counts;
    const paired=counts.titleCount===1&&counts.continueCount===1
      &&((counts.notNowCount===1&&counts.skipCount===0)||(counts.notNowCount===0&&counts.skipCount===1));
    if(heading==='imported-setup'&&!paired)heading='unknown';
  }
  const role=dialog.getAttribute('role'), enclosing=dialog.contains(control);
  surface=enclosing ? (role==='dialog'?'enclosing-role-dialog':role==='alertdialog'?'enclosing-role-alertdialog':role===null&&dialog.getAttribute('aria-modal')==='true'?'enclosing-role-aria-modal':'unknown')
    : role==='dialog'?'separate-dialog':role==='alertdialog'?'separate-alertdialog':role==='menu'?'separate-menu':'unknown';
  if(role!=='dialog'||enclosing||dialog.querySelectorAll(group).length)return result('other');
  const sourceButtons=[...dialog.querySelectorAll('button')].filter(visible);
  const historyForms=[...dialog.querySelectorAll('form')].filter(e=>visible(e)&&['pointer-events-auto','relative','hide-scrollbar','flex','flex-col','gap-6','overflow-y-auto','pb-10'].every(t=>e.classList.contains(t)));
  const countTitle=text=>publicHeadings.filter(e=>e.innerText.trim()===text).length;
  const countButton=(text,type)=>sourceButtons.filter(e=>e.innerText.trim()===text&&(type===null||e.getAttribute('type')===type)).length;
  const measured={computerHistoryTitleCount:countTitle('Connect Computer History'),
    computerHistoryFormCount:historyForms.length,computerHistoryNotNowCount:countButton('Not now','button'),
    computerHistoryCustomizeCount:countButton('Customize apps','button'),
    computerHistoryAllowCount:countButton('Allow access','submit')+countButton('Allow all apps','submit'),
    projectImportTitleCount:countTitle('Select settings to import'),
    projectImportContinueCount:countButton('Continue',null),projectImportNotNowCount:countButton('Not now',null)};
  if(Object.values(measured).every(count=>count<=32))sourceCounts=measured;

  if(sourceCounts&&sourceCounts.computerHistoryTitleCount===1&&sourceCounts.computerHistoryFormCount===1
      &&sourceCounts.computerHistoryNotNowCount===1&&sourceCounts.computerHistoryCustomizeCount===1
      &&sourceCounts.computerHistoryAllowCount===1) {
    const sourceTitle=publicHeadings.find(e=>e.innerText.trim()==='Connect Computer History');
    if(sourceTitle.tagName==='H2'&&['heading-dialog','select-none'].every(t=>sourceTitle.classList.contains(t))
        &&historyForms[0].contains(sourceTitle))return result('other','classified','computer-history-consent');
  }
  const headings=[...dialog.querySelectorAll('[class~="text-3xl"][class~="leading-9"][class~="font-normal"]')].filter(e=>visible(e)&&e.innerText.trim()==="You're all set");
  const forms=[...dialog.querySelectorAll('form')].filter(e=>visible(e)&&['m-auto','flex','w-full','shrink-0','flex-col','items-center','justify-between','py-4'].every(t=>e.classList.contains(t)));
  if(headings.length!==1)return result('other','classified','heading-mismatch');
  if(forms.length!==1||!forms[0].contains(headings[0]))return result('other','classified','form-mismatch');
  const form=forms[0];
  const buttons=[...form.querySelectorAll('button')].filter(visible);
  const terms=[...form.querySelectorAll('a')].filter(e=>visible(e)&&e.classList.contains('underline')&&e.getAttribute('href')==='https://openai.com/terms');
  const privacy=[...form.querySelectorAll('a')].filter(e=>visible(e)&&e.classList.contains('underline')&&e.getAttribute('href')==='https://openai.com/privacy');
  if(buttons.length!==1||buttons[0].getAttribute('type')!=='submit'||buttons[0].innerText.trim()!=='Continue'
      ||buttons[0].disabled||buttons[0].getAttribute('aria-disabled')==='true')return result('other','classified','continue-mismatch');
  if(terms.length!==1||privacy.length!==1)return result('other','classified','legal-links-mismatch');
  return result('chatgpt-onboarding-complete','classified','matched');
}
function candidate(a, b) {
  return a && b && !a.blocked && !b.blocked && JSON.stringify(a.rect) === JSON.stringify(b.rect)
    && a.points.find(p => b.points.some(q => p.x === q.x && p.y === q.y));
}
async function run(page, ownerGuard, deadline, rejected, mainGuard, folderTrust, profileLoan, directCDP=false, progress=()=>{}) {
  const maxWaitMs = deadline - Date.now();
  const originalUrl = page.url();
  const ownedEndpoint = async () => {
    const fail = reason => { facts.roleProofFailure=reason; return false; };
    checkpoint();
    try {
      if (Date.now() >= deadline) return fail('deadline-expired');
      if (typeof ownerGuard !== 'function'
          || !isHeldMainGuard(mainGuard,ownerGuard) && ownerGuard() !== true) return fail('ownership-lost');
      if (Date.now() >= deadline) return fail('deadline-expired');
      // The factory-issued retained guard below brackets its complete source sample
      // with fresh ownership. Every other callback still needs the direct proof.
      const browser = page.context().browser();
      const pages = browser?.contexts().flatMap(context => context.pages());
      if (!pages) return fail('query-failed');
      const mainProved = typeof mainGuard === 'function' ? await mainGuard() : false;
      if (!mainProved && typeof mainGuard?.failure === 'function') {
        const reason=mainGuard.failure();
        if(['deadline','native-ownership','page-set','main-identity','main-focus','main-scope',
          'auxiliary-route','auxiliary-identity','auxiliary-focus','auxiliary-controls','query-failed','unmeasured'].includes(reason)) facts.mainGuardFailure=reason;
        if(reason==='page-set'&&typeof mainGuard.failureDetails==='function') {
          const details=mainGuard.failureDetails();
          const count=value=>value===null||Number.isInteger(value)&&value>=0&&value<=32;
          if(details&&Object.keys(details).sort().join(',')==='currentCount,heldPresent,initialCount,reason'
            &&['initial-count','held-main-missing','before-sample-changed','after-sample-changed'].includes(details.reason)
            &&count(details.initialCount)&&count(details.currentCount)&&typeof details.heldPresent==='boolean')
            facts.pageSetFailure={...details};
        }
      }
      if (Date.now() >= deadline) return fail('deadline-expired');
      if (pages.length !== 1 && !mainProved) {
        // Retain only a bounded protocol inventory of the rejected snapshot.
        // This never authorizes choosing among renderer targets.
        if (pages.length > 32) facts.rejectedPageInventory={status:'overflow'};
        else {
          const inventory={status:'complete',total:pages.length,held:0,app:0,blank:0,devtools:0,other:0};
          for(const candidate of pages){
            if(candidate===page)inventory.held++;
            const url=candidate.url();
            const kind=url==='about:blank'?'blank':url.startsWith('app:')?'app':url.startsWith('devtools:')?'devtools':'other';
            inventory[kind]++;
          }
          facts.rejectedPageInventory=inventory;
          rejected(pages.slice());
        }
        return fail('page-count');
      }
      if (!pages.includes(page) || pages.length === 1 && pages[0] !== page) return fail('page-changed');
      if (typeof mainGuard === 'function' && !mainProved) return fail('ownership-lost');
      if (page.url() !== originalUrl) return fail('url-changed');
      return true;
    } catch { return fail('query-failed'); }
    finally { checkpoint(); }
  };
  const facts = {schemaVersion:1, mechanism:'codex-public-onboarding', diagnosticsOnly:true,
    stage:'session', errorCategory:null, conversationalScope:false, engineeringControl:false,
    roleClickAttempted:false, roleClickCompleted:false, engineeringChecked:false,
    continueControl:false, continueClickAttempted:false, continueClickCompleted:false,
    roleScopeAbsent:false, taskScopeProved:false, taskClickAttempted:false, taskClickCompleted:false, codingComposerReady:false, roleProofFailure:'unmeasured', sessionProofFailure:'unmeasured'};
  const checkpoint=()=>{try {progress({...facts});}catch { /* Observation cannot admit input. */ }};
  const stop = category => { facts.errorCategory=category; checkpoint(); return facts; };
  checkpoint();
  const sessionFailure = typeof ownerGuard !== 'function' ? 'guard-missing'
    : !Number.isFinite(deadline) || !Number.isFinite(maxWaitMs) || maxWaitMs > (process.platform==='win32'?120000:process.platform==='darwin'?90000:60000) ? 'deadline-invalid'
    : maxWaitMs < 1 ? 'deadline-expired'
    : !['win32','linux','darwin'].includes(process.platform) ? 'platform'
    : process.env.GITHUB_ACTIONS !== 'true' || process.env.RUNNER_ENVIRONMENT !== 'github-hosted'
      || process.env.RUNNER_OS !== ({win32:'Windows',linux:'Linux',darwin:'macOS'}[process.platform]) ? 'host-policy'
    : process.env.NANH_CODEX_PUBLIC_ONBOARDING !== 'engineering' ? 'onboarding-policy' : null;
  if (sessionFailure !== null) {
    facts.sessionProofFailure=sessionFailure;
    return stop('invalid-session');
  }
  let scope, fieldset, radio, label, button, transitionScope;
  async function proof(needChecked=false) {
    const fail = reason => { facts.roleProofFailure=reason; return false; };
    facts.roleProofFailure='unmeasured';
    if (Date.now() >= deadline) return fail('deadline-expired');
    if (!await ownedEndpoint()) return false;
    if (Date.now() >= deadline) return fail('deadline-expired');
    const legends = page.locator('fieldset > legend:visible').filter({hasText:/^Select the kind of work you do$/});
    if (await legends.count() !== 1) return fail('legend-count');
    fieldset = legends.locator('..');
    if (await fieldset.locator(GROUP).count() < 1) return fail('group-absent');
    scope = fieldset.locator(`xpath=ancestor::div[${TOKENS.map(t=>`contains(concat(' ', normalize-space(@class), ' '), ' ${t} ')`).join(' and ')}][1]`);
    if (await scope.count() !== 1) return fail('scope-count');
    // The same public role page also has a footer fieldset for the optional
    // personalized-suggestions checkbox. Only the live role group is unique.
    const roleFieldsets = scope.locator('fieldset:visible')
      .filter({has: page.locator('legend').filter({hasText:/^Select the kind of work you do$/})})
      .filter({has: page.locator(GROUP)});
    if (await roleFieldsets.count() !== 1) return fail('fieldset-count');
    if (await scope.getByRole('button',{name:/^(Log in|Sign in|Continue with Google|Continue with Apple)$/i}).count() !== 0) return fail('login-present');
    radio = fieldset.locator(`${GROUP}[value="engineering"]`);
    if (await radio.count() !== 1) return fail('engineering-count');
    label = fieldset.locator('label').filter({hasText:/^Engineering$/});
    if (await label.count() !== 1) return fail('label-count');
    if (!await label.evaluate((e, group) => {
      const inputs=[...e.ownerDocument.querySelectorAll(group+'[value="engineering"]')];
      return inputs.length===1 && inputs[0].labels?.length===1 && inputs[0].labels[0]===e && e.innerText.trim()==='Engineering';
    },GROUP)) return fail('label-association');
    if (!needChecked && !await radio.isEnabled()) return fail('engineering-disabled');
    if (needChecked && (!await radio.isChecked() || await fieldset.locator(GROUP+':checked').count() !== 1)) return fail('checked-mismatch');
    if (!await ownedEndpoint()) return false;
    if (Date.now() >= deadline) return fail('deadline-expired');
    return true;
  }
  async function click(control, reprove, before, after) {
    if (!await reprove()) return false;
    const blocked=async reason=>{
      facts.actionabilityFailure=reason;
      if(reason==='foreign-overlay') {
        facts.foreignOverlay='guard-rejected';
        facts.foreignOverlayProof='unmeasured';
        let held;
        const guard=async frame=>{
          if(Date.now()>=deadline){facts.foreignOverlayProof='deadline-expired';return false;}
          if(!await ownedEndpoint()){facts.foreignOverlayProof=facts.roleProofFailure==='deadline-expired'?'deadline-expired':'ownership-lost';return false;}
          if(Date.now()>=deadline){facts.foreignOverlayProof='deadline-expired';return false;}
          if(page.mainFrame()!==frame){facts.foreignOverlayProof='frame-replaced';return false;}
          if(!await reprove()){
            facts.foreignOverlayProof=facts.roleProofFailure==='deadline-expired'?'deadline-expired'
              : ['ownership-lost','final-ownership','page-count','page-changed','url-changed','query-failed'].includes(facts.roleProofFailure)?'ownership-lost':'role-proof-rejected';
            return false;
          }
          if(!await control.evaluate((e,original)=>e===original,handle)){
            facts.foreignOverlayProof='control-replaced';return false;
          }
          if(!await ownedEndpoint()){facts.foreignOverlayProof=facts.roleProofFailure==='deadline-expired'?'deadline-expired':'ownership-lost';return false;}
          if(Date.now()>=deadline){facts.foreignOverlayProof='deadline-expired';return false;}
          return true;
        };
        try {
          const frame=page.mainFrame();
          if(await guard(frame)) {
            held=await handle.evaluateHandle(foreignSurface);
            const first=await handle.evaluate(classifyForeign,held);
            if(await guard(frame)) {
              const second=await handle.evaluate(classifyForeign,held);
              if(JSON.stringify(first)===JSON.stringify(second) && await guard(frame)) {
                facts.foreignOverlay=second.category;
                facts.foreignOverlayProof=second.proof;
                if(second.proof==='classified') {
                  facts.foreignOverlaySurface=second.surface;
                  facts.foreignOverlayFingerprint=second.fingerprint;
                  facts.foreignOverlayHeading=second.heading;
                  if(second.importSetup!==null)facts.foreignOverlayImportSetup=second.importSetup;
                  if(second.sourceCounts!==null)facts.foreignOverlaySourceCounts=second.sourceCounts;
                  if(second.actionability!==null)facts.foreignOverlayActionability=second.actionability;
                }
              } else if(second.category==='guard-rejected' && facts.foreignOverlayProof==='unmeasured') {
                facts.foreignOverlayProof=second.proof;
              } else if(facts.foreignOverlayProof==='unmeasured') facts.foreignOverlayProof='unstable-classification';
            }
          }
        } catch { facts.foreignOverlayProof='query-failed'; }
        finally { if(held) await held.dispose(); }
      }
      if(facts.roleProofFailure==='unmeasured')facts.roleProofFailure='control-not-actionable';
      return false;
    };
    const handle = await control.elementHandle();
    if (!handle) return await blocked('detached-or-inert');
    try {
      const first = await handle.evaluate(sample,facts.taskControlKind==='skip-optional-capabilities');
      if (first?.blocked) return await blocked(first.blocked);
      await wait(Math.min(100,Math.max(0,deadline-Date.now())));
      if (!await reprove() || !await control.evaluate((e, held)=>e===held,handle)) return false;
      const second=await handle.evaluate(sample,facts.taskControlKind==='skip-optional-capabilities');
      if (second?.blocked) return await blocked(second.blocked);
      const point = candidate(first,second);
      if (!point) return await blocked(first?.points?.length && second?.points?.length?'unstable':'no-owned-point');
      if (!await reprove() || !await control.evaluate((e, held)=>e===held,handle)) return false;
      const final=await handle.evaluate(sample,facts.taskControlKind==='skip-optional-capabilities');
      if (final?.blocked) return await blocked(final.blocked);
      if (!candidate(first,final) || !final.points.some(p=>p.x===point.x&&p.y===point.y)) return await blocked('unstable');
      if (!await ownedEndpoint()) return false;
      if (Date.now() >= deadline) { facts.roleProofFailure='deadline-expired'; return false; }
      // Seal before recording the first attempt, including uncertain click failures.
      if(before==='roleClickAttempted'&&typeof mainGuard?.sealInitialActions==='function')
        mainGuard.sealInitialActions();
      facts[before]=true;
      checkpoint();
      await handle.click({position:point,timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
      facts[after]=true;
      checkpoint();
      if (!await ownedEndpoint()) return false;
      if (Date.now() >= deadline) { facts.roleProofFailure='deadline-expired'; return false; }
      return true;
    } finally { await handle.dispose(); }
  }
  try {
    facts.stage='role-proof';
    if(folderTrust!==undefined) {
      facts.folderTrust=folderTrust;
      if(!['absent','completed'].includes(facts.folderTrust.status))return stop('action-blocked');
    }
    // Frozen conversational parent disables controls during its pending work.
    // Only that positively matched, disabled control is a pollable startup state.
    while (!await proof()) {
      if (facts.roleProofFailure !== 'engineering-disabled' || Date.now() >= deadline)
        return stop('scope-not-matched');
      await wait(Math.min(100, Math.max(0, deadline-Date.now())));
    }
    facts.conversationalScope=true; facts.engineeringControl=true;
    if (await fieldset.locator(GROUP+':checked').count() !== 0) return stop('role-already-selected');
    facts.stage='role-action';
    if (!await click(label,()=>proof(), 'roleClickAttempted','roleClickCompleted')) return stop('action-blocked');
    facts.stage='role-readback';
    while (Date.now()<deadline && !await proof(true)) {
      if (!await ownedEndpoint()) return stop('ownership-lost');
      await wait(100);
    }
    if (!await proof(true)) return stop('role-readback-failed');
    facts.engineeringChecked=true;
    button=scope.getByRole('button',{name:'Continue',exact:true});
    while (Date.now()<deadline && await button.count()===1 && !await button.isEnabled()) {
      if (!await proof(true)) return stop('role-readback-failed');
      await wait(100);
    }
    if (await button.count() !== 1 || !await button.isEnabled()
        || !await button.evaluate(e=>e.tagName==='BUTTON')) return stop('continue-not-matched');
    facts.continueControl=true;
    transitionScope=await scope.elementHandle();
    if(!transitionScope)return stop('continue-not-matched');
    const continueProof=async()=>await proof(true)&&await button.count()===1&&await button.isEnabled()
      &&await scope.evaluate((element,held)=>element===held,transitionScope);
    facts.stage='continue-action';
    if (!await click(button,continueProof,'continueClickAttempted','continueClickCompleted')) return stop('action-blocked');
    facts.stage='scope-transition';
    let taskKind;
    const skipAdmitted=(process.platform==='linux'&&process.env.NANH_CODEX_PROJECT_ARTIFACT_SHA256==='ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c')
      ||(process.platform==='win32'&&process.env.NANH_CODEX_PROJECT_ARTIFACT_SHA256==='12070c9dd6cca622d043abdaf2225406abe6de19e8061024d93b93255478603e')
      ||(process.platform==='darwin'&&process.env.NANH_CODEX_PROJECT_ARTIFACT_SHA256==='f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7');
    const observeTask=async()=>{
      const shape=await transitionScope.evaluate(taskContinuation,true);
      facts.taskScopeObservation=shape;
      if(shape?.heldScopeConnected!==true||shape.heldScopeVisible!==true
          ||shape.roleRadioCount!==0)return false;
      const kind=shape.exactGetStartedCount===1&&shape.exactAckLeafCount===1?'get-started'
        :shape.exactGetStartedCount===0&&skipAdmitted&&await transitionScope.evaluate(optionalCapabilities)?'skip-optional-capabilities':null;
      if(kind===null||taskKind&&taskKind!==kind)return false;
      taskKind=kind;return true;
    };
    while (Date.now()<deadline) {
      if (!await ownedEndpoint()) return stop('ownership-lost');
      if (await page.locator(GROUP).count()===0) {
        facts.roleScopeAbsent=true;
        if(directCDP&&isHeldMainGuard(mainGuard,ownerGuard)&&require('./codex-dom.cjs').directCDPPolicy()) {
          const coding=await page.evaluate(codingScope,true);
          if(!await ownedEndpoint()||Date.now()>=deadline)return stop('ownership-lost');
          facts.transitionPublicDOMObservation=coding.publicDOM;
          facts.transitionReadinessObservation=coding.observation;
          // Continue can lead directly to the public home instead of a task page.
          // Prove it twice on the original owned document; no task action occurred.
          if(await page.evaluate(require('./codex-dom.cjs').homeComposerScope)) {
            if(!await ownedEndpoint()||Date.now()>=deadline)return stop('ownership-lost');
            if(await page.evaluate(require('./codex-dom.cjs').homeComposerScope)) {
              if(!await ownedEndpoint()||Date.now()>=deadline)return stop('ownership-lost');
              facts.stage='coding-readiness';facts.codingComposerReady=true;
              facts.homeAfterContinueReady=true;return facts;
            }
          }
        }

        if(await observeTask()&&await ownedEndpoint()&&Date.now()<deadline) {
          facts.taskScopeProved=true;facts.taskControlKind=taskKind;break;
        }
      }
      await wait(100);
    }
    if(!facts.taskScopeProved)return stop('scope-remained');
    facts.stage='task-action';
    const taskButton=page.getByRole('button',{name:facts.taskControlKind==='skip-optional-capabilities'?'Skip':'Get Started',exact:true});
    const taskProof=async()=>Date.now()<deadline&&await ownedEndpoint()
      &&await observeTask()
      &&await taskButton.count()===1&&await taskButton.isEnabled()
      &&await taskButton.evaluate((element,held)=>element.tagName==='BUTTON'&&held.contains(element),transitionScope);
    if(!await click(taskButton,taskProof,'taskClickAttempted','taskClickCompleted'))return stop('action-blocked');
    facts.stage='coding-readiness';
    let confirmationConsumed=false;
    while(Date.now()<deadline) {
      if(!await ownedEndpoint())return stop('ownership-lost');
      const coding=await page.evaluate(codingScope,true);
      if(process.platform==='win32') {
        // Recheck the original held owner/document after this passive DOM read.
        if(!await ownedEndpoint()||Date.now()>=deadline)return stop('ownership-lost');
        facts.codingPublicDOMObservation=coding.publicDOM;
      }
      facts.codingReadinessObservation=coding.observation;
      if(process.platform==='linux'&&skipAdmitted) {
        facts.codingNavigationObservation=coding.navigation;
        facts.codingHomeObservation=coding.home;
        facts.codingEditableObservation=coding.ancestry;
        facts.codingHomeStateObservation=coding.homeState;
      }
      if(directCDP&&isHeldMainGuard(mainGuard,ownerGuard)&&require('./codex-dom.cjs').directCDPPolicy()
          &&await page.evaluate(require('./codex-dom.cjs').homeComposerScope)) {
        if(!await ownedEndpoint()||Date.now()>=deadline)return stop('ownership-lost');
        facts.codingComposerReady=true;return facts;
      }
      if(process.platform==='darwin'&&skipAdmitted&&profileLoan
          &&coding.observation.modalCount===0&&coding.homeState.status==='observed'
          &&coding.homeState.homeComposerCount===1&&coding.homeState.proseMirrorEditableCount===1
          &&coding.homeState.workspaceControlCount===1&&coding.homeState.pendingTextareaCount===0
          &&coding.homeState.pendingGroupCount===0) {
        facts.macHomeStateObservation=await require('./codex-macos-home-state.cjs').observe({
          page,alive:ownedEndpoint,ownerGuard,deadline,loan:profileLoan,
          workspace:profileLoan.directories[0].path,openMenu:true});
        return stop('scope-remained');
      }
      if(process.platform==='linux'&&skipAdmitted&&profileLoan
          &&coding.observation.modalCount===0&&coding.homeState.status==='observed'
          &&coding.homeState.homeComposerCount===1&&coding.homeState.proseMirrorEditableCount===1
          &&coding.homeState.enabledSendCount===1&&coding.homeState.workspaceControlCount===1
          &&coding.homeState.pendingTextareaCount===0&&coding.homeState.pendingGroupCount===0) {
        facts.workspaceMenuObservation=await require('./codex-workspace-menu.cjs').run(page,
          ownedEndpoint,ownerGuard,deadline,profileLoan,profileLoan.directories[0].path,{frozenLinuxTrial:true});
        return stop('scope-remained');
      }
      if(coding.ready===true) {facts.codingComposerReady=true;return facts;}
      if(skipAdmitted&&facts.taskControlKind==='skip-optional-capabilities'
          &&coding.observation.modalCount===1&&!confirmationConsumed) {
        const confirmation=page.getByRole('button',{name:'Go to ChatGPT',exact:true});
        if(await confirmation.count()===1) {
          const held=await confirmation.elementHandle();
          try {
            if(!held||!await ownedEndpoint()||Date.now()>=deadline)return stop('action-blocked');
            const form=await held.evaluateHandle(e=>e.closest('form'));
            const dialog=await held.evaluateHandle(e=>e.closest('[role="dialog"]'));
            const retained={form,dialog,diagnostic:true};
            try {
            const first=await held.evaluate(skipConfirmation,retained);
            facts.taskSkipConfirmationProof=first?.rejection??'matched';
            if(!first||first.rejection)return stop('action-blocked');
            await wait(Math.min(100,Math.max(0,deadline-Date.now())));
            if(!await ownedEndpoint()||Date.now()>=deadline
                ||!await confirmation.evaluate((e,h)=>e===h,held))return stop('action-blocked');
            const second=await held.evaluate(skipConfirmation,retained);
            facts.taskSkipConfirmationProof=second?.rejection??'matched';
            if(!second||second.rejection)return stop('action-blocked');
            const point=candidate(first,second);
            if(!point||!await ownedEndpoint()||Date.now()>=deadline)return stop('action-blocked');
            const final=await held.evaluate(skipConfirmation,retained);
            facts.taskSkipConfirmationProof=final?.rejection??'matched';
            if(!final||final.rejection)return stop('action-blocked');
            if(!candidate(first,final)||!final.points.some(p=>p.x===point.x&&p.y===point.y)
                ||!await ownedEndpoint()||Date.now()>=deadline)return stop('action-blocked');
            confirmationConsumed=true;
            facts.taskSkipConfirmationAttempted=true;facts.taskSkipConfirmationCompleted=false;
            await held.click({position:point,timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
            facts.taskSkipConfirmationCompleted=true;
            if(!await ownedEndpoint()||Date.now()>=deadline)return stop('action-blocked');
            } finally {await form.dispose();await dialog.dispose();}
          } finally {if(held)await held.dispose();}
        }
      }
      await wait(Math.min(100,Math.max(0,deadline-Date.now())));
    }
    return stop('scope-remained');
  } catch { return stop(facts.roleClickAttempted || facts.continueClickAttempted ? 'action-uncertain':'observation-failed'); }
  finally { if(transitionScope)await transitionScope.dispose(); }
};
// These routes are bound to the inspected Windows, Linux and macOS distributions' app-protocol and main chunks.
function sourceRoute(raw) {
  try {
    const url=new URL(raw);
    if(url.protocol!=='app:'||url.hostname!=='-'||url.searchParams.getAll('initialRoute').length!==1)return 'unknown';
    const route=url.searchParams.get('initialRoute');
    if(url.pathname==='/detached-window.html'&&route==='/detached-window')return 'detachedWindow';
    if(url.pathname!=='/index.html')return 'unknown';
    return new Map([['/avatar-overlay','avatarOverlay'],['/hotkey-window','hotkeyWindow'],
      ['/chatgpt/quick-chat','quickChat'],['/chatgpt/quick-chat-prewarm','quickChatPrewarm'],
      ['/global-dictation','globalDictation'],['/debug','debug']]).get(route)??'unknown';
  } catch { return 'unknown'; }
}
exports.run=async function(page, ownerGuard, deadline, mainGuard, folderTrust, profileLoan, directCDP=false, progress=()=>{}) {
  let rejectedPages, rejectedUrls;
  const facts=await run(page,ownerGuard,deadline,pages=>{rejectedPages=pages;rejectedUrls=pages.map(p=>p.url());},mainGuard,folderTrust,profileLoan,directCDP,progress);
  if(!rejectedPages)return facts;
  const unavailable=()=>{facts.rejectedPageInventory.source={status:'unavailable'};return facts;};
  const stable=()=>{
    if(Date.now()>=deadline||ownerGuard()!==true)return false;
    const current=page.context().browser().contexts().flatMap(context=>context.pages());
    return Date.now()<deadline&&current.length===rejectedPages.length&&current.every((p,i)=>p===rejectedPages[i]&&p.url()===rejectedUrls[i]);
  };
  try {
    if(!stable())return unavailable();
    const routes=Object.fromEntries(['avatarOverlay','hotkeyWindow','quickChat','quickChatPrewarm',
      'detachedWindow','globalDictation','debug','unknown'].map(key=>[key,0]));
    const visibility={visible:0,hidden:0,unavailable:0};
    for(const candidate of rejectedPages){
      if(!stable())return unavailable();
      const before=candidate.url();
      let timer;
      const state=await Promise.race([candidate.evaluate(()=>document.visibilityState).catch(()=>null),
        new Promise(resolve=>{timer=setTimeout(()=>resolve(null),Math.max(1,deadline-Date.now()));})])
        .finally(()=>clearTimeout(timer));
      if(!stable()||candidate.url()!==before)return unavailable();
      routes[sourceRoute(before)]++;
      visibility[state==='visible'||state==='hidden'?state:'unavailable']++;
    }
    facts.rejectedPageInventory.source={status:'complete',routes,visibility};
    return facts;
  } catch { return unavailable(); }
};
exports.sourceRoute=sourceRoute;
exports.sample = sample;
exports.candidate = candidate;
exports.scopeFingerprint = SCOPE;

exports.taskContinuation=taskContinuation;
exports.codingScope=codingScope;

exports.optionalCapabilities=optionalCapabilities;

exports.skipConfirmation=skipConfirmation;
