const assert=require('node:assert/strict'),vm=require('node:vm');
const helper=require('./codex-dialog-catalog.cjs');
const catalog=require('./codex-dialog-title-catalog.json');
function fixture(text='Skip setup?') {
 const doc={hasFocus:()=>true};
 const body={parentElement:null,inert:false,getAttribute:()=>null,style:{opacity:'1',display:'block',visibility:'visible',pointerEvents:'none'}};
 const title={id:':PRIVATE:',tagName:'H2',ownerDocument:doc,isConnected:true,textContent:text,style:{display:'block',visibility:'visible',opacity:'1',pointerEvents:'auto'},getBoundingClientRect:()=>({width:0,height:0})};
 const dialog={parentElement:body,ownerDocument:doc,isConnected:true,inert:false,
  style:{opacity:'1',display:'block',visibility:'visible',pointerEvents:'auto'},
  getBoundingClientRect:()=>({width:100,height:100}),contains:e=>e===title,
  getAttribute:k=>k==='role'?'dialog':k==='aria-labelledby'?title.id:null};
 let dialogs=[dialog],ids=[title];
 doc.querySelectorAll=s=>s==='[id]'?ids:s.includes('[role=')?dialogs:[];
 dialog.querySelectorAll=()=>[];
 const context={document:doc,getComputedStyle:e=>e.style};
 const classify=vm.runInNewContext('('+helper.classifyTitle.toString()+')',context);
 return {doc,title,dialog,context,classify,setDialogs:v=>dialogs=v,setIds:v=>ids=v,
  held:{document:doc,dialog,title,reference:title.id},entries:catalog.entries.filter(e=>e.platform==='mac')};
}
const rendererSource=require('node:fs').readFileSync(`${__dirname}/observe-renderer.cjs`,'utf8');
const boundary=rendererSource.slice(rendererSource.indexOf('function passiveCatalogGuard('),rendererSource.indexOf('function recordStaticDialog'));
const makeGuard=vm.runInNewContext(`(()=>{${boundary};return passiveCatalogGuard})()`);
const heldPage={};let browserPages=[heldPage],owned=true;
const sourceGuard=makeGuard({contexts:()=>[{pages:()=>browserPages}]},heldPage,()=>owned);
assert.equal(sourceGuard(),true);browserPages=[heldPage,{}];assert.equal(sourceGuard(),false);assert.equal(sourceGuard.lastFailure,'page-set');
browserPages=[{}];assert.equal(sourceGuard(),false);assert.equal(sourceGuard.lastFailure,'page-set');
browserPages=[heldPage];owned=false;assert.equal(sourceGuard(),false);assert.equal(sourceGuard.lastFailure,'native-ownership');
owned=true;assert.equal(sourceGuard(),true);assert.equal(sourceGuard.lastFailure,null);
async function main() {
 const linux=require('./codex-dialog-title-catalog-linux.json');
 assert.equal(linux.sourceVersion,'26.930.41038');
 assert.equal(require('node:crypto').createHash('sha256').update(require('node:fs').readFileSync(require.resolve('./codex-dialog-title-catalog-linux.json'))).digest('hex'),helper.facts('linux').catalogSha256);
 assert.equal(new Set(linux.entries.map(e=>e.id)).size,195);
 const lf=fixture('Global search');
 assert.equal(lf.classify({held:lf.held,entries:Object.values(Object.fromEntries(linux.entries.map(e=>[e.id,e])))}).status,'matched');
 assert.equal(helper.facts('linux').sourceVersion,'26.930.41038');
 assert.equal(helper.facts('darwin').sourceVersion,'26.930.41038');
 const linuxEnv={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Linux',NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_PROJECT_POLICY:'open-project',NANH_CODEX_PROJECT_ARTIFACT_SHA256:helper.pins.linux.artifact};
 assert.equal(helper.policy('chatgpt-desktop','linux',linuxEnv),true);
 assert.equal(helper.policy('chatgpt-desktop','linux',{...linuxEnv,NANH_CODEX_PROJECT_ARTIFACT_SHA256:'e0174d8d0a5f4141145458c814f3c2d863dd67e942b868785a1f5dac9cba3e16'}),false);
 const mac=require('./codex-dialog-title-catalog-macos.json');
 assert.equal(new Set(mac.entries.map(e=>e.id)).size,195);
 assert.equal(require('node:crypto').createHash('sha256').update(require('node:fs').readFileSync(require.resolve('./codex-dialog-title-catalog-macos.json'))).digest('hex'),helper.facts('darwin').catalogSha256);
 assert.equal(helper.facts('win32').sourceVersion,'26.930.31730');
 for(const [text,id]of [['Welcome to ChatGPT','workspaceOnboarding.dialogTitle'],['What kind of work do you do?','work.onboarding.role.new.question']]) {
  const f=fixture(text);const entries=Object.values(Object.fromEntries(linux.entries.map(e=>[e.id,e])));
  assert.equal(f.classify({held:f.held,entries}).sourceTitleIds[0],id);
 }
 for(const [text,id]of [['Welcome to ChatGPT','workspaceOnboarding.dialogTitle'],['What kind of work do you do?','work.onboarding.role.new.question']]) {
  const f=fixture(text);const entries=Object.values(Object.fromEntries(mac.entries.map(e=>[e.id,e])));
  assert.equal(f.classify({held:f.held,entries}).sourceTitleIds[0],id);
 }
 for(const [text,id]of [["Process details", "source.dialogTitle.processDetails"], ["Health workspace onboarding", "health.onboarding.dialogLabel"]]) {
  const f=fixture(text);const entries=Object.values(Object.fromEntries(linux.entries.map(e=>[e.id,e])));
  assert.equal(f.classify({held:f.held,entries}).sourceTitleIds[0],id);
 }
 for(const [text,id]of [["Email", "restricted.aeon.email.dialog.loadingTitle"], ["Analysis", "chatgpt.pythonExecution.analysisTitle"]]) {
  const f=fixture(text);const entries=Object.values(Object.fromEntries(require('./codex-dialog-title-catalog-windows.json').entries.map(e=>[e.id,e])));
  assert.equal(f.classify({held:f.held,entries}).sourceTitleIds[0],id);
 }
 for(const [text,id]of [["Redeem credits", "chatgpt.promotion.credit_grant_redemption.modal.title.v2"]]) {
  const f=fixture(text);const entries=Object.values(Object.fromEntries(linux.entries.map(e=>[e.id,e])));
  assert.equal(f.classify({held:f.held,entries}).sourceTitleIds[0],id);
 }
 const underneath=fixture('UNKNOWN');
 const legend={textContent:'Select the kind of work you do',isConnected:true,getBoundingClientRect:()=>({width:10,height:10}),style:{display:'block',visibility:'visible'}};
 const radio={getAttribute:k=>k==='value'?'engineering':null};
 const originalQuery=underneath.doc.querySelectorAll;
 underneath.doc.querySelectorAll=s=>s==='fieldset > legend'?[legend]:s.startsWith('input[type="radio"]')?[radio]:originalQuery(s);
 const outside=underneath.classify({held:underneath.held,entries:underneath.entries});
 assert.equal(outside.status,'unknown');assert.equal(outside.sourceShape.pageEngineering,1);assert.equal(outside.sourceShape.dialogEngineering,0);
 const originalContains=underneath.dialog.contains;underneath.dialog.contains=e=>originalContains(e)||e===legend||e===radio;
 const inside=underneath.classify({held:underneath.held,entries:underneath.entries});
 assert.equal(inside.status,'unknown');assert.equal(inside.sourceShape.dialogEngineering,1);assert.equal(inside.sourceShape.dialogRoleLegend,1);
 assert.equal(JSON.stringify(inside).includes('Select the kind'),false);
 const excessive=fixture();assert.equal(excessive.classify({held:excessive.held,entries:Array(257).fill({id:'known',text:'fixed'})}).rejectionStage,'scope');
 const f=fixture();
 let result=f.classify({held:f.held,entries:f.entries});
 assert.equal(result.status,'matched');assert.equal(result.sourceTitleEmpty,false);assert.equal(result.sourceTitleIds[0],'electron.onboarding.conversationalOnboarding.skipDialog.title');
 assert.ok(!JSON.stringify(result).includes('PRIVATE')); // actual DOM ID never exported
 const original=f.title.textContent;f.title.textContent='';
 const empty=f.classify({held:f.held,entries:f.entries});assert.equal(empty.status,'unknown');assert.equal(empty.sourceTitleEmpty,true);f.title.textContent=original;f.title.textContent='PRIVATE user-generated conversation';
 assert.equal(f.classify({held:f.held,entries:f.entries}).status,'unknown');f.title.textContent=original;
 assert.equal(f.classify({held:f.held,entries:[...f.entries,{id:'other-known-source',text:original}]}).status,'ambiguous');
 f.setIds([f.title,{...f.title}]);assert.equal(f.classify({held:f.held,entries:f.entries}).rejectionStage,'title-count');f.setIds([f.title]);
 f.setDialogs([f.dialog,{...f.dialog}]);assert.equal(f.classify({held:f.held,entries:f.entries}).rejectionStage,'dialog-count');f.setDialogs([f.dialog]);
 f.dialog.getAttribute=k=>k==='role'?'dialog':k==='aria-labelledby'?':one: :two:':null;
 assert.equal(f.classify({held:f.held,entries:f.entries}).rejectionStage,'reference');
 const g=fixture();g.title.tagName='DIV';assert.equal(g.classify({held:g.held,entries:g.entries}).rejectionStage,'title-tag');
 for(const bad of ['0','unknown','2','']) {
  const x=fixture();x.dialog.style.opacity=bad;assert.equal(x.classify({held:x.held,entries:x.entries}).rejectionStage,'actionability');
 }
 const invisible=fixture();invisible.dialog.inert=true;assert.equal(invisible.classify({held:invisible.held,entries:invisible.entries}).rejectionStage,'actionability');
 const pointer=fixture();pointer.dialog.style.pointerEvents='none';assert.equal(pointer.classify({held:pointer.held,entries:pointer.entries}).rejectionStage,'actionability');
 for(const platform of ['linux','darwin','win32']) {
  const pin=helper.pins[platform],env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:pin.runner,NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_PROJECT_POLICY:'open-project',NANH_CODEX_PROJECT_ARTIFACT_SHA256:pin.artifact};
  assert.equal(helper.policy('chatgpt-desktop',platform,env),true);
  for(const key of Object.keys(env))assert.equal(helper.policy('chatgpt-desktop',platform,{...env,[key]:'wrong'}),false);
 }
 let owns=true,now=0,reads=0;const d=fixture();
 const page={evaluateHandle:async fn=>({value:vm.runInNewContext('('+fn.toString()+')',d.context)(),dispose:async()=>{}}),
  evaluate:async(fn,arg)=>{reads++;return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});}};
 const held={page,target:'PRIVATE_TARGET',frame:'PRIVATE_FRAME',loader:'PRIVATE_LOADER',url:'app://-/index.html'};
 let current=held,ownerQueries=0,identityQueries=0;const opts={guard:async()=>{ownerQueries++;return owns;},identity:async()=>{identityQueries++;return current;},same:(a,b)=>['page','target','frame','loader','url'].every(k=>a[k]===b[k]),deadline:Date.now()+1000};
 const phases=[];opts.progress=phase=>phases.push(phase);
 const good=await helper.observe(held,'darwin',opts);assert.equal(good.status,'matched');assert.equal(reads,2);assert.equal(ownerQueries,2);assert.equal(identityQueries,2);assert.ok(!JSON.stringify(good).includes('PRIVATE'));
 assert.ok(phases.includes('sample-first')&&phases.includes('sample-second')&&phases.includes('guard-after'));
 assert.equal(phases.at(-1),'finished');assert.ok(!JSON.stringify(phases).includes('PRIVATE'));
 const stillGood=await helper.observe(held,'darwin',{...opts,progress:()=>{throw Error('PRIVATE');}});assert.equal(stillGood.status,'matched');
 const ordinaryEvaluate=page.evaluate;
 for(const cause of ['owner','loader']) {
  owns=true;current=held;reads=0;
  page.evaluate=async(fn,arg)=>{const result=await ordinaryEvaluate(fn,arg);if(reads===1){if(cause==='owner')owns=false;else current={...held,loader:'changed'};}return result;};
  const rejected=await helper.observe(held,'darwin',opts);assert.equal(rejected.status,'guard-rejected');assert.equal(rejected.matchCount,null);assert.deepEqual(rejected.sourceTitleIds,[]);assert.equal(reads,2);
 }
 owns=true;current=held;page.evaluate=ordinaryEvaluate;
 for(const key of ['target','frame','loader','url','page']) {
  current={...held,[key]:'changed'};reads=0;assert.equal((await helper.observe(held,'darwin',opts)).status,'guard-rejected');assert.equal(reads,0);
 }
 current=held;owns=false;reads=0;assert.equal((await helper.observe(held,'darwin',opts)).status,'guard-rejected');assert.equal(reads,0);
 owns=true;reads=0;assert.equal((await helper.observe(held,'darwin',{...opts,deadline:Date.now()-1})).status,'guard-rejected');assert.equal(reads,0);
 page.evaluate=async(fn,arg)=>{reads++;if(reads===2)d.title.textContent='changed';return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});};
 const changed=await helper.observe(held,'darwin',opts);assert.equal(changed.status,'guard-rejected');assert.equal(changed.matchCount,null);
 // Rejected title projection is still measured twice under the held identity.
 d.title.textContent='Skip setup?';d.title.tagName='DIV';reads=0;
 page.evaluate=async(fn,arg)=>{reads++;return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});};
 const tagRejected=await helper.observe(held,'win32',opts);
 assert.equal(reads,2);assert.equal(tagRejected.rejectionStage,'title-tag');
 assert.equal(tagRejected.titleReferenceCount,null);assert.deepEqual(tagRejected.sourceTitleIds,[]);
 d.title.tagName='H2';reads=0;
 page.evaluate=async()=>{throw new Error('PRIVATE exception');};
 assert.equal((await helper.observe(held,'win32',opts)).rejectionStage,'query');
 assert.equal((await helper.observe(held,'win32',{...opts,deadline:Date.now()-1})).rejectionStage,'deadline');
 assert.ok(!JSON.stringify(tagRejected).includes('PRIVATE'));
 assert.equal(new Set(catalog.entries.map(e=>e.id)).size,63);
 for(const [text,id]of [['Global search','chatgpt.global_search.modal.title'],['Import from your browser','settings.browserUse.profileImport.title'],['Import unverified extensions?','settings.browserUse.profileImport.extensionsConfirmationTitle']]) {const added=fixture(text);assert.equal(added.classify({held:added.held,entries:added.entries}).sourceTitleIds[0],id);}
 const windowsCatalog=require('./codex-dialog-title-catalog-windows.json');
 assert.equal(new Set(windowsCatalog.entries.map(e=>e.id)).size,200);
 assert.ok(catalog.entries.filter(e=>e.platform==='windows').every(original=>windowsCatalog.entries.some(e=>original.id===e.id&&original.text===e.text)));
 d.title.textContent='Skip setup?';reads=0;
 page.evaluate=async(fn,arg)=>{reads++;return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});};
 const windows=await helper.observe(held,'win32',opts);
 assert.equal(windows.status,'matched');assert.equal(reads,2);
 assert.equal(windows.platform,'windows');assert.equal(windows.artifactSha256,helper.pins.win32.artifact);
 assert.ok(catalog.entries.every(e=>!e.text.includes('{')&&/^[a-f0-9]{64}$/.test(e.sha)));
 console.log('PASS: static dialog titles, sr-only label projection, duplicates, immutable two reads, privacy; zero actions');
}
main().catch(error=>{console.error(error);process.exitCode=1;});

// Fixed title identity and source command structure remain independent diagnostics.
{
 const f=fixture('Command menu');
 const current=require('./codex-dialog-title-catalog-windows.json').entries;
 f.dialog.classList={contains:name=>name==='global-command-menu-dialog'};
 const prior=f.dialog.getAttribute;
 f.dialog.getAttribute=k=>k==='cmdk-dialog'?'':prior(k);
 f.dialog.querySelectorAll=selector=>selector==='[cmdk-root]'?[{}]:selector==='input[cmdk-input][role="combobox"]'?[{}]:selector==='[cmdk-list][role="listbox"]'?[{}]:[];
 const out=f.classify({held:f.held,entries:current});
 assert.deepEqual(Array.from(out.sourceTitleIds),['codex.commandMenu.title']);
 assert.equal(out.commandMenuShape.rootCount,1);
 assert.equal(out.commandMenuShape.inputCount,1);
 assert.equal(out.commandMenuShape.globalScopeCount,1);
 assert.ok(!JSON.stringify(out).includes(':PRIVATE:'));
 f.dialog.classList={contains:()=>false};f.dialog.querySelectorAll=()=>[];
 const lookalike=f.classify({held:f.held,entries:current});
 assert.equal(lookalike.commandMenuShape.globalScopeCount,0);
 assert.equal(lookalike.commandMenuShape.inputCount,0);
 const ambiguous=f.classify({held:f.held,entries:[...current,{id:'fixed-other-source-id',text:'Command menu'}]});
 assert.equal(ambiguous.status,'ambiguous');
 f.setDialogs([f.dialog,f.dialog]);
 assert.equal(f.classify({held:f.held,entries:current}).rejectionStage,'dialog-count');
}

for(const platform of ["linux","macos","windows"]) {
 const entries=Object.values(Object.fromEntries(require(`./codex-dialog-title-catalog-${platform}.json`).entries.map(e=>[e.id,e])));
 for(const [text,id] of [["Are you sure you want to close the window?","desktop.windowCloseConfirmation.title"],["Lockdown mode is on for this chat","chatgptConversations.lockdown.dialog.title"]]) {
  const f=fixture(text);assert.deepEqual(Array.from(f.classify({held:f.held,entries}).sourceTitleIds),[id]);
  f.title.textContent="PRIVATE is unavailable in Lockdown mode";assert.equal(f.classify({held:f.held,entries}).status,"unknown");
  f.title.textContent=text;assert.equal(f.classify({held:f.held,entries:[...entries,{id:"duplicate-static-source",text}]}).status,"ambiguous");
 }
}

// The update title binds only the frozen shared-chunk branding constant.
for (const platform of ["linux", "macos", "windows"]) {
 const entries = require(`./codex-dialog-title-catalog-${platform}.json`).entries;
 const title = entries.find(entry => entry.id === "appHeader.installUpdate.confirmTitle");
 assert.equal(title.binding.value, "ChatGPT");
 assert.match(title.binding.sha, /^[a-f0-9]{64}$/);
 const f = fixture("Update ChatGPT now?");
 assert.deepEqual(Array.from(f.classify({held: f.held, entries}).sourceTitleIds), [title.id]);
 for (const value of ["PRIVATE", "nan-harness", "Codex"]) {
  f.title.textContent = `Update ${value} now?`;
  assert.equal(f.classify({held: f.held, entries}).status, "unknown");
 }
}

// Finite ICU folder-count forms never interpolate owned paths or authorize trust.
for (const platform of ['linux','macos','windows']) {
 const entries=require(`./codex-dialog-title-catalog-${platform}.json`).entries;
 for (const [id,text] of [["projectSetup.consent.title.one", "Trust this folder?"], ["projectSetup.consent.title.other", "Trust these folders?"], ["projectSetup.consent.untrustedTitle.one", "This folder is marked untrusted"], ["projectSetup.consent.untrustedTitle.other", "These folders are marked untrusted"]]) {
  const selected=entries.filter(e=>e.id===id);
  assert.equal(selected.length,1); assert.equal(selected[0].text,text);
  const f=fixture(text); const result=f.classify({held:f.held,entries});
  assert.equal(result.matchCount,1); assert.deepEqual(Array.from(result.sourceTitleIds),[id]);
 }
 const wrong=fixture('Trust /private/owned-folder?');
 assert.equal(wrong.classify({held:wrong.held,entries}).matchCount,0);
}
