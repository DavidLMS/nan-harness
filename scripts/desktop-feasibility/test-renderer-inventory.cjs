// Read-only public startup headings reduce to closed enums, without a browser.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-renderer.cjs`, 'utf8');
const timingStart = source.indexOf('function onboardingTrial(');
const timingEnd = source.indexOf('async function run()', timingStart);
const timing = vm.runInNewContext(`(() => { ${source.slice(timingStart, timingEnd)}
  return {onboardingTrial, onboardingDeadline,onboardingBudget,inventoryDeadlines}; })()`);
const hosted = {GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted',
  RUNNER_OS: 'Windows', NANH_CODEX_PUBLIC_ONBOARDING: 'engineering'};
assert.equal(timing.onboardingTrial('chatgpt-desktop', 'win32', hosted), true);
for (const [app, platform, env] of [
  ['claude-desktop', 'win32', hosted], ['chatgpt-desktop', 'freebsd', hosted],
  ['chatgpt-desktop', 'win32', {...hosted, RUNNER_ENVIRONMENT: 'self-hosted'}],
  ['chatgpt-desktop', 'win32', {...hosted, NANH_CODEX_PUBLIC_ONBOARDING: 'unknown'}],
]) assert.equal(timing.onboardingTrial(app, platform, env), false);
const prepared={codexProfileIsolation:'prepared-windows',inventoryDeadlineUnixMs:100000};
assert.equal(timing.inventoryDeadlines(true,'win32',prepared,1000).total,100000);
assert.equal(timing.inventoryDeadlines(true,'win32',prepared,1000).startup,36000);
assert.equal(timing.inventoryDeadlines(true,'win32',{...prepared,inventoryDeadlineUnixMs:2000},1000).startup,2000);
for(const value of [undefined,1000,999,121001,Infinity,NaN,'100000',1000.5]) {
 assert.throws(()=>timing.inventoryDeadlines(true,'win32',{...prepared,inventoryDeadlineUnixMs:value},1000));
}
assert.throws(()=>timing.inventoryDeadlines(true,'linux',prepared,1000));
assert.throws(()=>timing.inventoryDeadlines(false,'win32',prepared,1000));
assert.equal(timing.inventoryDeadlines(true,'linux',{},1000).total,61000);
assert.equal(timing.inventoryDeadlines(true,'win32',{},1000).total,121000);
// Every setup stage keeps the original total deadline, including early Trust.
// Repeated calls after actions cannot reset or add to the original allocation.
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 36000), 60000);
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 10000), 60000);
for(const now of [0,10000,35000,59000,61000]) {
  const deadline=timing.onboardingDeadline(true,35000,60000,now);
  assert.equal(deadline,60000);
  assert.equal(now<deadline,now<60000);
}
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 61000), 60000);
assert.equal(timing.onboardingDeadline(false, 25000, 25000, 26000), 25000);
assert.equal(timing.onboardingBudget(true,'win32'),120000);
for(const platform of ['linux','darwin'])assert.equal(timing.onboardingBudget(true,platform),60000);
for(const platform of ['win32','linux','darwin'])assert.equal(timing.onboardingBudget(false,platform),25000);
assert.equal(timing.onboardingDeadline(true,35000,120000,121000),120000);
const start = source.indexOf('const counts = await page.evaluate(') + 'const counts = await page.evaluate('.length;
const end = source.indexOf('}, app);', start) + 1;
function trial(headings, app = 'pen-desktop', bodyText = '', roleCount = 0) {
  const nodes = headings.map(textContent => ({textContent, isConnected: true,
    getBoundingClientRect: () => ({width: 10, height: 10})}));
  const read = vm.runInNewContext(`(${source.slice(start, end)})`, {
    document: {readyState: 'complete', body: {innerText: bodyText},
      querySelectorAll: selector => selector === 'h1' || selector === '*' ? nodes
        : selector === 'input[type="radio"][name="conversational-onboarding-inline-role"]'
          ? Array.from({length: roleCount}, () => ({textContent: 'PRIVATE_ROLE', isConnected: true,
              getBoundingClientRect: () => ({width: 1, height: 1})})) : []},
    location: {href: 'pen:synthetic', protocol: 'pen:'},
    getComputedStyle: () => ({visibility: 'visible'}),
  });
  const facts = read(app);
  const output = JSON.stringify(facts);
  for (const heading of headings) assert(!output.includes(heading));
  if (bodyText) assert(!output.includes(bodyText));
  assert(!output.includes('synthetic'));
  assert(!output.includes('PRIVATE_ROLE'));
  assert.equal(facts.onboardingCounts.roleRadios, roleCount);
  return facts.startupScreen;
}
assert.equal(trial(['Hardware acceleration unavailable']), 'gpu-unavailable');
assert.equal(trial(['Failed to start pen.dev']), 'startup-failed');
assert.equal(trial(['PRIVATE_UNKNOWN_HEADING']), 'other');
assert.equal(trial(['Hardware acceleration unavailable', 'PRIVATE']), 'other');
assert.equal(trial(['Hardware acceleration unavailable'], 'chatgpt-desktop'), 'unmeasured');
const connectionError = 'Something went wrong connecting to the Codex CLI. Try restarting';
assert.equal(trial([], 'chatgpt-desktop', connectionError), 'cli-connection-failed');
assert.equal(trial([], 'chatgpt-desktop', 'PRIVATE_UNKNOWN_ERROR'), 'unmeasured');
assert.equal(trial([], 'claude-desktop', connectionError), 'unmeasured');
assert.equal(trial([], 'chatgpt-desktop', '', 11), 'unmeasured');
console.log('Renderer inventory: closed startup headings passed');

// Execute the actual standalone browser callback: role identity is independent
// of a separate dialog, while duplicate source roots must never identify main.
function roleSourceScope({dialogCount=1,duplicateRoot=false,foreignFieldset=false}={}) {
  const node=()=>({isConnected:true,getBoundingClientRect:()=>({width:10,height:10})});
  const scope={...node(),contains:e=>e===fieldset||radios.includes(e)};
  const fieldset={...node(),contains:e=>radios.includes(e)};
  const legend={...node(),innerText:'Select the kind of work you do',parentElement:fieldset};
  const radios=Array.from({length:11},node);
  radios[0].value='engineering';radios[0].labels=[{innerText:'Engineering'}];
  const dialogs=Array.from({length:dialogCount},()=>({...node(),contains:()=>false}));
  if(foreignFieldset)scope.contains=()=>false;
  const read=vm.runInNewContext(`(${source.slice(source.indexOf('function correlationScope()'),source.indexOf('async function correlationIdentity('))})`,{
    document:{hasFocus:()=>true,querySelectorAll:selector=>
      selector.startsWith('div[class~=')?(duplicateRoot?[scope,node()]:[scope])
        :selector.startsWith('input[type="radio"]')?radios
        :selector==='fieldset > legend'?[legend]
        :selector==='[role="dialog"],[role="alertdialog"]'?dialogs:[]},
    getComputedStyle:()=>({display:'block',visibility:'visible'})});
  return read();
}
for(const dialogCount of [0,1,2]) {
  const scope=roleSourceScope({dialogCount});
  assert.equal(scope.mainScope,true);
  assert.equal(scope.counts.dialog,dialogCount);
}
assert.equal(roleSourceScope({duplicateRoot:true}).mainScope,false);
assert.equal(roleSourceScope({foreignFieldset:true}).mainScope,false);

// Passive main/aux binding never sends input or changes the singleton guard.
(async () => {
  let clock=0;
  const helper=vm.runInNewContext(`(() => { ${source.slice(timingStart,timingEnd)}
    return {observeMainAux,correlationScope,correlationIdentity,captureCorrelationMain,bindCorrelationMain,heldMainGuard,initialMainFacts,initialMainRoute,mainConfirmationFacts,focusCapturedMain}; })()`,
    {Date:{now:()=>clock},setTimeout,clearTimeout,URL,require});
  const empty={roleLegend:0,roleRadios:0,engineering:0,dialog:0,quickChatComposer:0,editable:0};
  function fixture() {
    clock=0;
    const main={url:()=> 'app://-/index.html'},aux={url:()=> 'app://-/index.html?initialRoute=%2Favatar-overlay'};
    let current=[main,aux],owned=true,queries=0;
    const state=page=>({page,url:page.url(),target:page===main?'private-main':'private-aux',
      frame:page===main?'frame-main':'frame-aux',loader:'private-loader',frameUrl:page.url(),fragment:'',
      scope:{mainScope:page===main,focused:page===main,
        counts:page===main?{...empty,roleLegend:1,roleRadios:11,engineering:1,dialog:1}: {...empty}}});
    const held=state(main),browser={contexts:()=>[{pages:()=>current}]};
    let alter=()=>{};
    const identity=async page=>{queries++;const result=state(page);alter(result,queries);return result;};
    return {main,aux,held,browser,identity,setAlter:fn=>{alter=fn;},setPages:p=>{current=p;},
      loseOwner:()=>{owned=false;},run:(heldOverride=held)=>helper.observeMainAux(heldOverride,browser,
        ()=>owned,1000,()=> 'avatarOverlay',identity,async ms=>{clock+=ms;})};
  }
  {
    const sampled=fixture();sampled.setPages([sampled.main]);
    sampled.setAlter(value=>{value.scope.mainScope=false;});
    const diagnostic=helper.mainConfirmationFacts(),saved=[];
    const result=await helper.bindCorrelationMain(sampled.held,sampled.browser,()=>true,300,
      ()=> 'avatarOverlay',sampled.identity,async ms=>{clock+=ms;},diagnostic,async()=>true,false,
      ()=>saved.push(JSON.parse(JSON.stringify(diagnostic))));
    assert.equal(result,null);assert.equal(diagnostic.status,'deadline');
    assert.ok(saved.some(value=>value.status==='unmeasured'&&value.mainScopeUnique===false
      &&value.counts.roleRadios===11&&value.identityUnchanged===true));
    assert.equal(saved.at(-1).status,'deadline');assert.ok(!JSON.stringify(saved).includes('private-'));
    const lost=fixture();let owned=true;
    lost.setAlter(()=>{owned=false;});
    const rejected=helper.mainConfirmationFacts(),progress=[];
    assert.equal(await helper.bindCorrelationMain(lost.held,lost.browser,()=>owned,1000,
      ()=> 'avatarOverlay',lost.identity,async()=>{},rejected,null,true,
      ()=>progress.push(JSON.parse(JSON.stringify(rejected)))),null);
    assert.equal(progress.at(-1).status,'ownership-lost');
    assert.equal(progress.at(-1).counts.roleRadios,11);
  }
  let f=fixture(),result=await f.run();
  assert.equal(result.status,'observed');assert.equal(result.stableSamples,2);
  assert.equal(result.main.roleRadios,11);assert.equal(result.auxComposerAbsent,true);
  assert.equal(JSON.stringify(result).includes('private-'),false);
  f=fixture();result=await f.run(null);assert.equal(result.status,'initial-main-unavailable');
  f=fixture();f.setPages([f.main,f.aux,{}]);assert.equal((await f.run()).status,'page-count');
  for(const change of [r=>{r.loader='reloaded';},r=>{r.target='replaced';},r=>{r.url+='?changed';}]) {
    f=fixture();f.setAlter((r,n)=>{if(n===2)change(r);});
    assert.equal((await f.run()).status,'identity-changed');
  }
  f=fixture();f.setAlter((r,n)=>{if(n===2)f.setPages([{},f.aux]);});
  assert.equal((await f.run()).status,'identity-changed');
  f=fixture();f.setAlter((r,n)=>{if(n===2)f.loseOwner();});
  assert.equal((await f.run()).status,'ownership-lost');
  for(const change of [r=>{r.scope.counts.quickChatComposer=1;},r=>{r.scope.counts.editable=1;},
    r=>{r.scope.focused=true;},r=>{r.scope.counts.roleLegend=1;}]) {
    f=fixture();f.setAlter((r,n)=>{if(n===3)change(r);});
    assert.equal((await f.run()).status,'source-scope');
  }
  for(const [mutate,status] of [[r=>{r.loader='changed';},'identity-changed'],
    [r=>{r.scope.mainScope=false;},'source-scope'],[r=>{r.scope.focused=false;},'document-unfocused']]) {
    f=fixture();f.setAlter((r,n)=>{if(n===1)mutate(r);});
    const facts=helper.mainConfirmationFacts();
    assert.equal(await helper.bindCorrelationMain(f.held,f.browser,()=>true,1000,
      ()=> 'avatarOverlay',f.identity,async()=>{},facts),null);
    assert.equal(facts.status,status);
    assert.equal(facts.inputChannel,'native-focused');
    assert.deepEqual(Object.keys(facts).sort(),['counts','documentFocused','identityUnchanged','inputChannel','mainScopeUnique','status']);
    assert.equal(JSON.stringify(facts).includes('app://'),false);
  }
  f=fixture();f.setAlter((r,n)=>{if(n===2)r.scope.mainScope=false;});
  assert.equal((await f.run()).status,'source-scope');
  f=fixture();f.setAlter((r,n)=>{if(n===2)clock=1001;});
  assert.equal((await f.run()).status,'deadline');
  f=fixture();result=await helper.observeMainAux(f.held,f.browser,()=>true,1000,
    ()=> 'unknown',f.identity,async()=>{});assert.equal(result.status,'source-scope');
  for(const [mutate,reason,count,held] of [
    [f=>f.setPages([f.main,f.aux,{}]),'initial-count',3,true],
    [f=>f.setPages([f.aux]),'held-main-missing',1,false],
    [f=>f.setAlter(()=>f.setPages([f.main])),'after-sample-changed',1,true],
  ]) {
    f=fixture();mutate(f);
    const guard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,
      ()=> 'avatarOverlay',f.identity,async()=>{});
    assert.equal(await guard(),false);
    assert.equal(guard.failure(),'page-set');
    assert.equal(guard.failureDetails().reason,reason);
    assert.equal(guard.failureDetails().currentCount,count);
    assert.equal(guard.failureDetails().heldPresent,held);
    assert.equal(Object.keys(guard.failureDetails()).length,4);
  }
  // Each complete source sample is bracketed by fresh native ownership proofs.
  // Ownership loss during either read must prevent publishing the auxiliary.
  for(const lostAt of [1,2]) {
    f=fixture();let owned=true,reads=0;
    f.setAlter((r,n)=>{if(n===lostAt)owned=false;});
    const guard=helper.heldMainGuard(f.held,f.browser,()=>{reads++;return owned;},1000,
      ()=> 'avatarOverlay',f.identity,async()=>{});
    assert.equal(await guard(),false);
    assert.equal(guard.failure(),'native-ownership');
    assert.equal(guard.binding().auxiliary,null);
    assert.equal(reads,2);
  }
  f=fixture();let ownerReads=0;
  const bounded=helper.heldMainGuard(f.held,f.browser,()=>{ownerReads++;return true;},1000,
    ()=> 'avatarOverlay',f.identity,async()=>{});
  assert.equal(await bounded(),true);assert.equal(ownerReads,4);
  assert.equal(await bounded(),true);assert.equal(ownerReads,6);
  f=fixture();
  const between=helper.heldMainGuard(f.held,f.browser,()=>true,1000,
    ()=> 'avatarOverlay',f.identity,async()=>f.setPages([f.main]));
  assert.equal(await between(),false);
  assert.equal(between.failureDetails().reason,'before-sample-changed');
  assert.equal(between.failureDetails().initialCount,2);
  assert.equal(between.failureDetails().currentCount,1);
  f=fixture();f.setPages([f.main,...Array.from({length:32},()=>({}))]);
  let overflow=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'avatarOverlay',f.identity);
  assert.equal(await overflow(),false);
  assert.equal(overflow.failureDetails().initialCount,null);
  assert.equal(overflow.failureDetails().currentCount,null);
  for(const unsafe of ['safe','route','focused','editable','owner','deadline','sealed']) {
    f=fixture();f.setPages([f.main]);let appeared=false;
    f.setAlter((r)=>{
      if(!appeared){appeared=true;f.setPages([f.main,f.aux]);}
      if(r.page===f.aux&&unsafe==='focused')r.scope.focused=true;
      if(r.page===f.aux&&unsafe==='editable')r.scope.counts.editable=1;
      if(unsafe==='deadline')clock=1001;
    });
    const guard=helper.heldMainGuard(f.held,f.browser,()=>unsafe!=='owner'||!appeared,1000,
      ()=>unsafe==='route'?'unknown':'avatarOverlay',f.identity,async()=>{},true,true);
    if(unsafe==='sealed')guard.sealInitialActions();
    assert.equal(await guard(),unsafe==='safe');
    if(unsafe==='safe') {
      assert(guard.binding().auxiliary);
      assert.equal(await guard(),true);
    }
  }
  // Cold activation gets one scoped passive restart, never a new target/input.
  for(const unsafe of ['safe','route','focused','editable','owner','deadline','role','three','missing','sealed','closed']) {
    f=fixture();f.setPages([f.main]);let appeared=false,owned=true;
    const cold=helper.heldMainGuard(f.held,f.browser,()=>owned,1000,
      value=>unsafe==='route'?'unknown':'avatarOverlay',f.identity,async()=>{},false,false,false);
    assert.equal(cold.allowPassiveActivationSettle(),false); // No completed source proof.
    assert.equal(await cold(),true);
    if(unsafe==='sealed')cold.sealInitialActions();
    assert.equal(cold.allowPassiveActivationSettle(),unsafe!=='sealed');
    assert.equal(cold.allowPassiveActivationSettle(),false); // Never issue twice.
    if(unsafe==='closed')cold.finishPassiveActivationSettle();
    f.setAlter(r=>{
      if(!appeared) {
        appeared=true;
        f.setPages(unsafe==='three'?[f.main,f.aux,{}]:unsafe==='missing'?[f.aux,{}]:[f.main,f.aux]);
      }
      if(r.page===f.main&&unsafe==='role')r.scope.mainScope=false;
      if(r.page===f.aux&&unsafe==='focused')r.scope.focused=true;
      if(r.page===f.aux&&unsafe==='editable')r.scope.counts.editable=1;
      if(unsafe==='owner')owned=false;
      if(unsafe==='deadline')clock=1001;
    });
    assert.equal(await cold(),unsafe==='safe');
    if(unsafe==='safe') {
      assert(cold.binding().auxiliary);
      cold.finishPassiveActivationSettle();
      f.setPages([f.main,{url:()=>f.aux.url()}]);
      assert.equal(await cold(),false);assert.equal(cold.failure(),'auxiliary-identity');
    } else assert.equal(cold.binding().auxiliary,null);
  }
  // A completed Trust grants only passive settlement of one retained blank auxiliary.
  for(const failure of ['none','replacement','foreign-route','main-focus','main-identity','controls','deadline','no-ticket']) {
    f=fixture();f.setPages([f.main]);let url='about:blank';f.aux.url=()=>url;
    const route=value=>value.includes('avatar-overlay')?'avatarOverlay':'unknown';
    const settle=helper.heldMainGuard(f.held,f.browser,()=>true,1000,route,f.identity,
      async ms=>{clock+=ms;url=failure==='foreign-route'?'https://foreign.invalid':'app://-/index.html?initialRoute=%2Favatar-overlay';
        if(failure==='replacement')f.setPages([f.main,{url:()=>url}]);
        if(failure==='deadline')clock=1001;});
    assert.equal(await settle(),true);settle.sealInitialActions();
    if(failure!=='no-ticket')assert.equal(settle.allowPassiveFolderSettle(),true);
    f.setPages([f.main,f.aux]);
    f.setAlter(r=>{if(r.page===f.main&&failure==='main-focus')r.scope.focused=false;
      if(r.page===f.main&&failure==='main-identity')r.loader='replaced';
      if(r.page===f.aux&&failure==='controls')r.scope.counts.editable=1;});
    assert.equal(await settle(),failure==='none');
    if(failure==='none'){assert(settle.binding().auxiliary);assert.equal(await settle(),true);}
  }
  // Startup focus acceptance stays capped at35s; its SAME held guard remains
  // valid for Trust and role proofs until the original60s total, with no rebinding.
  f=fixture();f.setPages([f.main]);
  const shared=helper.heldMainGuard(f.held,f.browser,()=>true,60000,()=> 'avatarOverlay',
    f.identity,async ms=>{clock+=ms;},false,false,false);
  clock=34000;
  assert.equal(await helper.focusCapturedMain(f.held,shared,35000,f.identity,undefined,
    async ms=>{clock+=ms;}),true);
  clock=40000;assert.equal(await shared(),true);
  clock=60000;assert.equal(await shared(),false);assert.equal(shared.failure(),'deadline');
  f=fixture();f.setPages([f.main]);clock=35000;let lateActivations=0;
  f.main.bringToFront=async()=>{lateActivations++;};
  const expiredStartup=helper.heldMainGuard(f.held,f.browser,()=>true,60000,()=> 'avatarOverlay',f.identity);
  assert.equal(await helper.focusCapturedMain(f.held,expiredStartup,35000,f.identity),false);
  assert.equal(lateActivations,0);
  // A scoped native replacement consumes one compound action, never CDP too.
  for(const scenario of ['ready','association-rejected','post-native-rejected','source-replaced','expired-prepare','action-uncertain']) {
    f=fixture();f.setPages([f.main]);clock=0;let focused=false;
    const calls=[];
    f.main.bringToFront=async()=>{throw Error('CDP activation must not be appended');};
    f.setAlter(r=>{r.scope.focused=focused;});
    const proof=async()=>true;proof.requireDocumentFocus=()=>{};
    proof.allowPassiveActivationSettle=()=>{calls.push('grant');};
    proof.finishPassiveActivationSettle=()=>{calls.push('close');};
    const native={prepare(){calls.push('prepare');if(scenario==='association-rejected')throw Error('PRIVATE');
      if(scenario==='expired-prepare')clock=1000;
      if(scenario==='source-replaced')f.setAlter(r=>{r.loader='replacement';});},
      activate(){calls.push('activate');focused=true;if(scenario==='action-uncertain')throw Error('PRIVATE');},
      verify(){calls.push('verify');return scenario!=='post-native-rejected';}};
    const facts={phase:'pre-proof',status:'unmeasured',activationAttempted:false,guardFailure:null};
    assert.equal(await helper.focusCapturedMain(f.held,proof,1000,f.identity,undefined,
      async ms=>{clock+=ms;},facts,native),scenario==='ready');
    assert.equal(calls.filter(c=>c==='activate').length,
      ['association-rejected','source-replaced','expired-prepare'].includes(scenario)?0:1);
    if(scenario==='ready')assert.deepEqual(calls,['prepare','grant','activate','verify','verify','close']);
    assert.equal(calls.at(-1),'close');
    assert.equal(calls.filter(c=>c==='grant').length,scenario==='association-rejected'?0:1);
    assert.equal(facts.activationAttempted,!['association-rejected','source-replaced','expired-prepare'].includes(scenario));
  }
  // Real retained guard: cold avatar appears during polling after ONE activation.
  for(const unsafe of ['safe','controls','owner','replaced']) {
    f=fixture();f.setPages([f.main]);let focused=false,armed=false,appeared=false,owned=true,activations=0;
    f.setAlter(r=>{
      if(r.page===f.main)r.scope.focused=focused;
      if(armed&&!appeared){appeared=true;f.setPages([f.main,f.aux]);}
      if(r.page===f.aux&&unsafe==='controls')r.scope.counts.editable=1;
      if(appeared&&unsafe==='owner')owned=false;
      if(appeared&&unsafe==='replaced')r.loader='replacement';
    });
    const proof=helper.heldMainGuard(f.held,f.browser,()=>owned,1000,()=> 'avatarOverlay',
      f.identity,async ms=>{clock+=ms;},false,false,false);
    const native={prepare(){},activate(){activations++;focused=true;armed=true;},verify(){return true;}};
    assert.equal(await helper.focusCapturedMain(f.held,proof,1000,f.identity,undefined,
      async ms=>{clock+=ms;},{},native),unsafe==='safe');
    assert.equal(activations,1);
    assert.equal(proof.allowPassiveActivationSettle(),false); // Finally permanently closed issuance.
    if(unsafe==='safe')assert(proof.binding().auxiliary);
  }
  // Activation diagnostics use the original proof/read/action sequence only.
  for(const scenario of ['focused','deadline','pre-reject','activation-error','focus-lost']) {
    f=fixture();f.setPages([f.main]);let focused=false,activations=0,proofs=0;
    f.main.bringToFront=async()=>{activations++;if(scenario==='activation-error')throw Error('PRIVATE');
      if(scenario==='focused')focused=true;};
    f.setAlter(r=>{r.scope.focused=focused;});
    const proof=async()=>{proofs++;return scenario!=='pre-reject'&&(scenario!=='focus-lost'||activations===0);};
    proof.failure=()=> 'native-ownership';proof.requireDocumentFocus=()=>{};
    const facts={phase:'pre-proof',status:'unmeasured',activationAttempted:false,guardFailure:null};
    assert.equal(await helper.focusCapturedMain(f.held,proof,1000,f.identity,undefined,
      async ms=>{clock+=ms;},facts),scenario==='focused');
    assert.equal(activations,scenario==='pre-reject'?0:1);
    assert.equal(facts.activationAttempted,activations===1);
    assert.equal(facts.status,scenario==='focused'?'focused':scenario==='deadline'?'deadline':
      scenario==='activation-error'?'query-failed':'rejected');
    assert.equal(facts.phase,scenario==='pre-reject'?'pre-proof':scenario==='activation-error'?'activation':
      scenario==='focused'?'final-proof':'polling');
    assert.equal(facts.guardFailure,['pre-reject','focus-lost'].includes(scenario)?'native-ownership':null);
    if(scenario==='focused')assert.equal(proofs,7);
    assert(!JSON.stringify(facts).includes('PRIVATE'));
  }
  // Initial source readiness is passive and never substitutes for identity.
  for(const scenario of ['ready','identity-change','never-ready']) {
    f=fixture();f.setPages([f.main]);let ready=false,activations=0;
    f.main.bringToFront=async()=>{activations++;};
    f.setAlter(r=>{r.scope.mainScope=ready;r.scope.focused=true;
      if(ready&&scenario==='identity-change')r.loader='replaced';});
    const proof=async()=>true;proof.requireDocumentFocus=()=>{};
    const diagnostic={};
    assert.equal(await helper.focusCapturedMain(f.held,proof,1000,f.identity,undefined,
      async ms=>{clock+=ms;ready=scenario!=='never-ready';},diagnostic),scenario==='ready');
    assert.equal(activations,0);
    assert.equal(diagnostic.status,scenario==='ready'?'focused':scenario==='never-ready'?'deadline':'rejected');
    clock=0;
  }
  // The auxiliary capability retains immutable identities across real actions;
  // the main role may transition while the auxiliary must remain inert.
  f=fixture();let owner=true;
  let inputGuard=helper.heldMainGuard(f.held,f.browser,()=>owner,1000,()=> 'avatarOverlay',f.identity,async ms=>{clock+=ms;});
  assert.equal(inputGuard.failure(),'unmeasured');
  assert.equal(await inputGuard(),true);
  f.setAlter(r=>{if(r.page===f.main){r.scope.mainScope=false;r.scope.counts.roleRadios=0;}});
  assert.equal(await inputGuard(),true);
  f.setAlter(r=>{if(r.page===f.aux)r.scope.counts.editable=1;});
  assert.equal(await inputGuard(),false);
  assert.equal(inputGuard.failure(),'auxiliary-controls');
  for(const [change,reason] of [[r=>{r.loader='changed';},'auxiliary-identity'],[r=>{r.scope.focused=true;},'auxiliary-focus'],[r=>{r.scope.counts.dialog=1;},'auxiliary-controls']]) {
    f=fixture();inputGuard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'avatarOverlay',f.identity,async()=>{});
    assert.equal(await inputGuard(),true);
    f.setAlter(r=>{if(r.page===f.aux)change(r);});assert.equal(await inputGuard(),false);
    assert.equal(inputGuard.failure(),reason);
  }
  f=fixture();inputGuard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'unknown',f.identity,async()=>{});
  assert.equal(await inputGuard(),false);
  f=fixture();inputGuard=helper.heldMainGuard(null,f.browser,()=>true,1000,()=> 'avatarOverlay',f.identity,async()=>{});
  assert.equal(await inputGuard(),false);
  assert.equal(inputGuard.failure(),'main-identity');
  const cases=[
    [f=>f.setPages([f.main,f.aux,{}]),'page-set'],
    [f=>f.setAlter(r=>{if(r.page===f.main)r.loader='PRIVATE replacement';}),'main-identity'],
    [f=>f.setAlter(r=>{if(r.page===f.main)r.scope.focused=false;}),'main-focus'],
    [f=>f.setAlter(r=>{if(r.page===f.main)r.scope.mainScope=false;}),'main-scope'],
    [f=>f.setAlter(()=>{clock=1001;}),'deadline'],
    [f=>f.setAlter(()=>{throw Error('PRIVATE query');}),'query-failed'],
  ];
  for(const [mutate,reason] of cases) {
    f=fixture();mutate(f);
    inputGuard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'avatarOverlay',f.identity,async()=>{},true);
    assert.equal(await inputGuard(),false);assert.equal(inputGuard.failure(),reason);
    assert(!JSON.stringify({failure:inputGuard.failure()}).includes('PRIVATE'));
  }
  f=fixture();inputGuard=helper.heldMainGuard(f.held,f.browser,()=>false,1000,()=> 'avatarOverlay',f.identity);
  assert.equal(await inputGuard(),false);assert.equal(inputGuard.failure(),'native-ownership');
  f=fixture();inputGuard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'unknown',f.identity);
  assert.equal(await inputGuard(),false);assert.equal(inputGuard.failure(),'auxiliary-route');
  // The callback runs serialized in a standalone browser realm, without Node helpers.
  const scope=vm.runInNewContext(`(${helper.correlationScope.toString()})()`, {
    document:{querySelectorAll:()=>[],hasFocus:()=>false},getComputedStyle:()=>({})});
  assert.equal(scope.mainScope,false);assert.equal(scope.counts.quickChatComposer,0);
  // Read-only public CDP identity queries are closed/detached; no action APIs exist.
  let detached=0;
  const page={url:()=> 'app://-/index.html',context:()=>({newCDPSession:async()=>({
    send:async method=>method==='Target.getTargetInfo'?{targetInfo:{targetId:'private-target'}}:
      {frameTree:{frame:{id:'private-frame',loaderId:'private-loader',url:'app://-/index.html'}}},
    detach:async()=>{detached++;}})}),evaluate:async()=>({counts:empty,focused:false,mainScope:false})};
  clock=0;const identity=await helper.correlationIdentity(page,1000);
  assert.equal(identity.loader,'private-loader');assert.equal(detached,1);
  const closed=helper.initialMainFacts();
  const soleBrowser={contexts:()=>[{pages:()=>[page]}]};
  assert.ok(await helper.captureCorrelationMain(page,soleBrowser,()=>true,1000,
    helper.correlationIdentity,async ms=>{clock+=ms;},closed));
  assert.equal(closed.status,'captured');assert.equal(closed.route,'primary');
  assert.equal(closed.targetPresent,true);assert.equal(closed.framePresent,true);
  assert.equal(closed.loaderPresent,true);assert.equal(JSON.stringify(closed).includes('private-'),false);
  assert.equal(helper.initialMainRoute('app://-/index.html?initialRoute=PRIVATE_PATH'),'primary-query');
  assert.equal(helper.initialMainRoute('app://-/index.html#PRIVATE'),'primary-fragment');
  assert.equal(helper.initialMainRoute('https://private.invalid/PRIVATE'),'other');
  const rejected=helper.initialMainFacts();
  const queryPage={url:()=> 'app://-/index.html?initialRoute=PRIVATE_PATH'};
  assert.equal(await helper.captureCorrelationMain(queryPage,{contexts:()=>[{pages:()=>[queryPage]}]},
    ()=>true,1000,async()=>{throw Error('no query permitted');},async()=>{},rejected),null);
  assert.equal(rejected.status,'route-rejected');assert.equal(rejected.route,'primary-query');
  assert.equal(JSON.stringify(rejected).includes('PRIVATE'),false);
  // Capture full immutable identity while loading/sole, without evaluating
  // source DOM. Confirm only after readiness and the source-known aux appears.
  f=fixture();f.setPages([f.main]);let ready=false,scopeQueries=0;
  const earlyIdentity=async (selected,deadline,includeScope=true)=>{
    const value=await f.identity(selected,deadline);
    if(includeScope) {scopeQueries++;value.scope.mainScope=ready&&selected===f.main;}
    else value.scope=null;
    return value;
  };
  const early=await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,earlyIdentity);
  assert.ok(early);assert.equal(early.scope,null);assert.equal(scopeQueries,0);
  ready=true;f.setPages([f.main,f.aux]);
  const confirmed=await helper.bindCorrelationMain(early,f.browser,()=>true,1000,
    ()=> 'avatarOverlay',earlyIdentity,async ms=>{clock+=ms;});
  assert.ok(confirmed);assert.equal(confirmed.scope.mainScope,true);
  assert.equal(confirmed.page,f.main);
  f=fixture();assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity),null);
  // A sole blank target may commit the official main before identity binding.
  // Ownership loss, a second target and deadline expiry still stop acquisition.
  f=fixture();f.setPages([f.main]);let committed=false;
  f.main.url=()=>committed?'app://-/index.html':'about:blank';
  const committedMain=await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity,
    async ms=>{clock+=ms;committed=true;});
  assert.ok(committedMain);assert.equal(committedMain.url,'app://-/index.html');
  f=fixture();f.setPages([f.main]);f.main.url=()=> 'about:blank';
  assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity,
    async ms=>{clock+=ms;}),null);
  f=fixture();f.setPages([f.main]);f.main.url=()=> 'about:blank';
  assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity,
    async ms=>{clock+=ms;f.setPages([f.main,f.aux]);}),null);
  f=fixture();f.setPages([f.main]);let identityAttempts=0;
  assert.ok(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,
    async (...args)=>{if(++identityAttempts===1)throw Error('loader not committed');return f.identity(...args);},
    async ms=>{clock+=ms;}));
  // Initial blank, query/fragment/aux routes, replacement or incomplete loader
  // never produce a capability that can be recovered from a later page list.
  for(const mutate of [r=>{r.url='about:blank';},r=>{r.url+='?initialRoute=%2Favatar-overlay';},
    r=>{r.url+='#changed';},r=>{r.frameUrl='app://-/other.html';},r=>{r.fragment='#changed';},
    r=>{r.loader='';},r=>{r.frame='';},r=>{r.target='';}]) {
    f=fixture();f.setPages([f.main]);f.setAlter(mutate);
    assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity),null);
  }
  for(const mutate of [r=>{r.loader='changed';},r=>{r.frame='changed';},r=>{r.target='changed';},
    r=>{r.page={};},r=>{r.url='app://-/other.html';}]) {
    f=fixture();f.setPages([f.main]);f.setAlter((r,n)=>{if(n===2)mutate(r);});
    assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity),null);
  }
  f=fixture();f.setPages([f.main]);f.setAlter((r,n)=>{if(n===1)f.setPages([{},f.aux]);});
  assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>true,1000,f.identity),null);
  f=fixture();f.setPages([f.main]);let owned=true;f.setAlter(()=>{owned=false;});
  assert.equal(await helper.captureCorrelationMain(f.main,f.browser,()=>owned,1000,f.identity),null);
  for(const mutate of [r=>{r.loader='new-document';},r=>{r.scope.mainScope=false;},r=>{r.scope.focused=false;}]) {
    f=fixture();f.setAlter((r,n)=>{if(n===1)mutate(r);});
    assert.equal(await helper.bindCorrelationMain(f.held,f.browser,()=>true,1000,
      ()=> 'avatarOverlay',f.identity,async()=>{}),null);
  }
  f=fixture();f.setAlter((r,n)=>{if(n===2)r.scope.mainScope=false;});
  assert.equal(await helper.bindCorrelationMain(f.held,f.browser,()=>true,1000,
    ()=> 'avatarOverlay',f.identity,async()=>{}),null);
  // The real identity callback does not read DOM before load; missing loader
  // and loading-to-reloaded frame are rejected instead of rebound.
  const loadingPage={...page,evaluate:async()=>{throw Error('DOM must not be queried');}};
  const loadingIdentity=await helper.correlationIdentity(loadingPage,1000,false);
  assert.equal(loadingIdentity.scope,null);
  const missingLoader={...page,context:()=>({newCDPSession:async()=>({send:async method=>
    method==='Target.getTargetInfo'?{targetInfo:{targetId:'private-target'}}:
      {frameTree:{frame:{id:'private-frame',loaderId:'',url:'app://-/index.html'}}},detach:async()=>{}})})};
  await assert.rejects(helper.correlationIdentity(missingLoader,1000,false));
  console.log('Renderer correlation: passive identity, early binding and privacy cases passed');
})().catch(error=>{console.error(error);process.exitCode=1;});

// Run the production final DOM reducer with exact source titles and buttons.
function sourceScreenFixture(titles, labels = [], app = 'chatgpt-desktop', notices = [], field = 'sourceScreen') {
  const node = text => ({textContent: text, innerText: text, isConnected: true,
    getAttribute: () => null, getBoundingClientRect: () => ({width: 10, height: 10})});
  const headings = titles.map(node), buttons = labels.map(node), alerts = notices.map(node);
  const callback = vm.runInNewContext(`(${source.slice(start, end)})`, {
    document: {readyState: 'complete', body: {innerText: ''}, querySelectorAll: selector =>
      selector === 'button,[role="button"]' ? buttons : selector === 'h1' ? headings
        : selector.startsWith('h1,h2,h3') ? headings : selector === '[role="status"],[role="alert"]' ? alerts : []},
    location: {href: 'app:private', protocol: 'app:'}, getComputedStyle: () => ({visibility: 'visible'}),
  });
  const result = callback(app);
  const bytes = JSON.stringify(result[field]);
  for (const value of [...titles, ...labels, ...notices]) if (bytes) assert(!bytes.includes(value));
  return result[field];
}
for (const [title, status] of [['Connect to your gateway','gateway-connect'],
  ['ChatGPT hit a snag','app-recovery'], ['Import from other AI apps','external-import'],
  ["You're all set",'all-set'], ['Give ChatGPT access to your computer','permission-setup']]) {
  assert.equal(sourceScreenFixture([title]).status, status);
  assert.equal(sourceScreenFixture([title,title]).status, 'ambiguous');
}
assert.equal(sourceScreenFixture(['PRIVATE_UNKNOWN']).status, 'unknown');
assert.equal(sourceScreenFixture(['Connect to your gateway','ChatGPT hit a snag']).status, 'ambiguous');
const signIn = sourceScreenFixture([], ['Continue to Sign In']);
assert.equal(signIn.status, 'unknown');
assert.equal(signIn.counts.continueSignIn, 1);
assert.equal(sourceScreenFixture(['Connect to your gateway'], [], 'claude-desktop'), undefined);
assert.equal(sourceScreenFixture(Array(40).fill('ChatGPT hit a snag')).counts.recoveryHeading, 32);
console.log('Renderer source-screen classifier: privacy, ambiguity and bounded counts passed');

const managed = (notices = [], labels = [], app = 'chatgpt-desktop') =>
  sourceScreenFixture([], labels, app, notices, 'managedSignIn');
for (const [message, status] of [['Loading sign-in requirements…', 'loading'],
  ['Update Codex on this machine to read its managed sign-in requirements', 'unsupported'],
  ['Your administrator has disabled all available sign-in methods', 'disabled'],
  ['Unable to load sign-in requirements', 'error']]) {
  assert.equal(managed([message]).status, status);
  assert.equal(managed([message,message]).status, 'ambiguous');
  assert.equal(managed([message], ['Enter API key']).status, 'ambiguous');
}
assert.equal(managed(['PRIVATE_UNKNOWN']).status, 'unknown');
assert.equal(managed([], ['Enter API key']).status, 'sign-in-options');
assert.equal(managed([], ['Continue with ChatGPT','Enter API key']).status, 'sign-in-options');
assert.equal(managed([], ['Enter API key','Enter API key']).status, 'ambiguous');
assert.equal(managed(Array(40).fill('Loading sign-in requirements…')).counts.loading, 32);
assert.equal(managed([], [], 'claude-desktop'), undefined);
console.log('Renderer managed sign-in classifier: exact source notices and passive choices passed');

// Public modal title wrappers need not be semantic headings. Only the exact
// source title plus its ordinary retry control can identify this passive state.
function publicDialogTrial(titles, buttons, dialogCount=1, app='chatgpt-desktop') {
  const node=text=>({textContent:text,innerText:text,children:[],isConnected:true,
    getBoundingClientRect:()=>({width:10,height:10}),getAttribute:()=>null});
  const dialogs=Array.from({length:dialogCount},()=>({...node(''),querySelectorAll:selector=>
    selector==='*'?titles.map(node):buttons.map(node)}));
  const read=vm.runInNewContext(`(${source.slice(start,end)})`,{
    document:{readyState:'complete',body:{innerText:''},querySelectorAll:selector=>
      selector==='[role="dialog"],[role="alertdialog"]'?dialogs:[]},
    location:{protocol:'app:',href:'app://PRIVATE'},getComputedStyle:()=>({visibility:'visible'})});
  const result=read(app).sourceDialog;
  assert(!JSON.stringify(result??null).includes('Could not load workspaces'));
  assert(!JSON.stringify(result??null).includes('PRIVATE'));
  return result;
}
assert.equal(publicDialogTrial(['Could not load workspaces'],['Try again']).status,'workspace-discovery-failed');
assert.equal(publicDialogTrial(['Could not load workspaces'],[]).status,'unknown');
assert.equal(publicDialogTrial(['PRIVATE'],['Try again']).status,'unknown');
assert.equal(publicDialogTrial(['Could not load workspaces','Could not load workspaces'],['Try again']).status,'ambiguous');
assert.equal(publicDialogTrial(['Could not load workspaces'],['Try again'],2).status,'ambiguous');
assert.equal(publicDialogTrial(['Could not load workspaces'],['Try again'],1,'claude-desktop'),undefined);
console.log('Renderer public modal classifier: exact source pair and privacy passed');

// A passive catalog does not require role controls behind a startup modal.
const passiveStart=source.indexOf('function passiveCatalogGuard(');
const passiveEnd=source.indexOf('function recordStaticDialog(',passiveStart);
const passive=vm.runInNewContext(`(() => { ${source.slice(passiveStart,passiveEnd)} return passiveCatalogGuard; })()`);
const immutablePage={},replacementPage={};let passivePages=[immutablePage],passiveOwner=true,proofs=0;
const passiveBrowser={contexts:()=>[{pages:()=>passivePages}]};
const passiveGuard=passive(passiveBrowser,immutablePage,()=>{++proofs;return passiveOwner;});
assert.equal(passiveGuard(),true);assert.equal(proofs,1);
passivePages=[replacementPage];assert.equal(passiveGuard(),false);
passivePages=[immutablePage,replacementPage];assert.equal(passiveGuard(),false);
passivePages=[];assert.equal(passiveGuard(),false);
passivePages=[immutablePage];passiveOwner=false;assert.equal(passiveGuard(),false);
console.log('Renderer passive title guard: held sole page and fresh ownership, independent of role controls passed');

// A later exception/termination cannot erase a previously sealed phase receipt.
const checkpointRoot=fs.mkdtempSync(require('node:path').join(require('node:os').tmpdir(),'renderer-checkpoint-'));
try {
  const output=require('node:path').join(checkpointRoot,'facts.json');
  const context=vm.createContext({fs,output,app:'chatgpt-desktop'});
  vm.runInContext(source.slice(source.indexOf('const facts ='),timingStart),context);
  vm.runInContext("checkpoint('request');facts.endpointOwned=true;checkpoint('folder-trust');",context);
  assert.throws(()=>vm.runInContext("throw Error('synthetic interrupted operation')",context));
  const receipt=JSON.parse(fs.readFileSync(output,'utf8'));
  assert.equal(receipt.observerStage,'folder-trust');assert.equal(receipt.errorCategory,'unclassified');
  assert.equal(receipt.endpointOwned,true);assert.equal(receipt.attached,false);
  assert(!JSON.stringify(receipt).includes(checkpointRoot));
  assert.equal(fs.existsSync(output+'.tmp'),false);
  vm.runInContext("facts.errorCategory=null;checkpoint('complete');",context);
  assert.equal(JSON.parse(fs.readFileSync(output,'utf8')).observerStage,'complete');
} finally {fs.rmSync(checkpointRoot,{recursive:true,force:true});}
console.log('Renderer checkpoint survives interrupted work without claiming completion');

// Persist the pending boundary before an awaited identity query. Diagnostics
// cannot grant ownership, even when their consumer throws.
(async()=>{
  const {createHeldMainGuard}=require('./codex-main-guard.cjs');
  let now=0,release,receipt,owned=true;
  const page={},held={page,key:'main'};
  const browser={contexts:()=>[{pages:()=>[page]}]};
  const pending=new Promise(resolve=>{release=resolve;});
  const guard=createHeldMainGuard(held,browser,()=>{now+=7;return owned;},1000,
    ()=> 'avatarOverlay',async()=>{await pending;now+=11;return {key:'main',scope:{focused:true,mainScope:true}};},
    async()=>{},false,false,true,{now:()=>now,sameCorrelationIdentity:(a,b)=>a.key===b.key,
      settleFolderAuxiliary:async()=>false,progress:value=>{receipt=value;throw Error('observer unavailable');}});
  const running=guard();
  assert.equal(receipt.phase,'main-identity');
  assert.equal(receipt.nativeProofCount,1);assert.equal(receipt.nativeProofMs,7);
  release();assert.equal(await running,true);
  assert.equal(receipt.phase,'complete');assert.equal(receipt.identityMs,11);
  assert.equal(receipt.nativeProofMs,14);assert.equal(receipt.elapsedMs,25);
  assert(!JSON.stringify(receipt).includes('mainScope'));
  owned=false;assert.equal(await guard(),false);assert.equal(receipt.phase,'rejected');
  assert.equal(guard.failure(),'native-ownership');
  console.log('Retained guard pending boundary, timing and observer failure fixtures PASS');
})().catch(error=>{console.error(error);process.exitCode=1;});

// Exercise the actual shutdown and top-level completion code without an app.
// A pending disconnect must remain distinguishable from a returned observer.
(async()=>{
  const shutdownStart=source.lastIndexOf('  } finally {');
  const shutdownEnd=source.indexOf('\n}\nrun().then(',shutdownStart);
  const shutdown=source.slice(shutdownStart+'  } finally {'.length,shutdownEnd);
  const completion=source.slice(shutdownEnd+3);
  async function fixture(rejectClose=false) {
    let release;
    const pending=new Promise((resolve,reject)=>{release=()=>rejectClose?reject(Error('PRIVATE')):resolve();});
    const facts={errorCategory:null},saved=[],process={exitCode:0};
    const context={facts,process,browser:{close:()=>pending},save:()=>saved.push({...facts})};
    const run=vm.runInNewContext(`(async()=>{${shutdown})`,context);
    const finished=vm.runInNewContext(completion,{...context,run});
    assert.equal(saved.at(-1).observerShutdown,'disconnecting');
    assert.equal(saved.length,1);
    release();await finished;
    if(rejectClose) {
      assert.equal(facts.observerShutdown,'disconnecting');
      assert.equal(facts.errorCategory,'attachment-or-action-failed');
      assert.equal(process.exitCode,1);
    } else {
      assert.deepEqual(saved.map(value=>value.observerShutdown),['disconnecting','disconnected','returned']);
      assert.equal(facts.errorCategory,null);
      assert.equal(process.exitCode,0);
    }
    assert(!JSON.stringify(saved).includes('PRIVATE'));
  }
  await fixture();await fixture(true);
  console.log('Renderer shutdown distinguishes pending, returned and failed disconnects');
})().catch(error=>{console.error(error);process.exitCode=1;});
