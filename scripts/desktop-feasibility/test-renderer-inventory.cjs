// Read-only public startup headings reduce to closed enums, without a browser.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/observe-renderer.cjs`, 'utf8');
const timingStart = source.indexOf('function onboardingTrial(');
const timingEnd = source.indexOf('async function run()', timingStart);
const timing = vm.runInNewContext(`(() => { ${source.slice(timingStart, timingEnd)}
  return {onboardingTrial, onboardingDeadline}; })()`);
const hosted = {GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted',
  RUNNER_OS: 'Windows', NANH_CODEX_PUBLIC_ONBOARDING: 'engineering'};
assert.equal(timing.onboardingTrial('chatgpt-desktop', 'win32', hosted), true);
for (const [app, platform, env] of [
  ['claude-desktop', 'win32', hosted], ['chatgpt-desktop', 'freebsd', hosted],
  ['chatgpt-desktop', 'win32', {...hosted, RUNNER_ENVIRONMENT: 'self-hosted'}],
  ['chatgpt-desktop', 'win32', {...hosted, NANH_CODEX_PUBLIC_ONBOARDING: 'unknown'}],
]) assert.equal(timing.onboardingTrial(app, platform, env), false);
// An exhausted startup clock cannot consume the public action budget, while
// the original total observation deadline still limits late attachment.
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 36000), 60000);
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 10000), 35000);
assert.equal(timing.onboardingDeadline(true, 35000, 60000, 61000), 60000);
assert.equal(timing.onboardingDeadline(false, 25000, 25000, 26000), 25000);
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

// Passive main/aux binding never sends input or changes the singleton guard.
(async () => {
  let clock=0;
  const helper=vm.runInNewContext(`(() => { ${source.slice(timingStart,timingEnd)}
    return {observeMainAux,correlationScope,correlationIdentity,captureCorrelationMain,bindCorrelationMain,heldMainGuard}; })()`,
    {Date:{now:()=>clock},setTimeout,clearTimeout,URL});
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
  f=fixture();f.setAlter((r,n)=>{if(n===2)r.scope.mainScope=false;});
  assert.equal((await f.run()).status,'source-scope');
  f=fixture();f.setAlter((r,n)=>{if(n===2)clock=1001;});
  assert.equal((await f.run()).status,'deadline');
  f=fixture();result=await helper.observeMainAux(f.held,f.browser,()=>true,1000,
    ()=> 'unknown',f.identity,async()=>{});assert.equal(result.status,'source-scope');
  // The auxiliary capability retains immutable identities across real actions;
  // the main role may transition while the auxiliary must remain inert.
  f=fixture();let owner=true;
  let inputGuard=helper.heldMainGuard(f.held,f.browser,()=>owner,1000,()=> 'avatarOverlay',f.identity,async ms=>{clock+=ms;});
  assert.equal(await inputGuard(),true);
  f.setAlter(r=>{if(r.page===f.main){r.scope.mainScope=false;r.scope.counts.roleRadios=0;}});
  assert.equal(await inputGuard(),true);
  f.setAlter(r=>{if(r.page===f.aux)r.scope.counts.editable=1;});
  assert.equal(await inputGuard(),false);
  for(const change of [r=>{r.loader='changed';},r=>{r.scope.focused=true;},r=>{r.scope.counts.dialog=1;}]) {
    f=fixture();inputGuard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'avatarOverlay',f.identity,async()=>{});
    assert.equal(await inputGuard(),true);
    f.setAlter(r=>{if(r.page===f.aux)change(r);});assert.equal(await inputGuard(),false);
  }
  f=fixture();inputGuard=helper.heldMainGuard(f.held,f.browser,()=>true,1000,()=> 'unknown',f.identity,async()=>{});
  assert.equal(await inputGuard(),false);
  f=fixture();inputGuard=helper.heldMainGuard(null,f.browser,()=>true,1000,()=> 'avatarOverlay',f.identity,async()=>{});
  assert.equal(await inputGuard(),false);
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
