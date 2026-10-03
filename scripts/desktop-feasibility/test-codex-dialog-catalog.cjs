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
 doc.querySelectorAll=s=>s==='[id]'?ids:dialogs;
 const context={document:doc,getComputedStyle:e=>e.style};
 const classify=vm.runInNewContext('('+helper.classifyTitle.toString()+')',context);
 return {doc,title,dialog,context,classify,setDialogs:v=>dialogs=v,setIds:v=>ids=v,
  held:{document:doc,dialog,title,reference:title.id},entries:catalog.entries.filter(e=>e.platform==='mac')};
}
async function main() {
 const f=fixture();
 let result=f.classify({held:f.held,entries:f.entries});
 assert.equal(result.status,'matched');assert.equal(result.sourceTitleIds[0],'electron.onboarding.conversationalOnboarding.skipDialog.title');
 assert.ok(!JSON.stringify(result).includes('PRIVATE')); // actual DOM ID never exported
 const original=f.title.textContent;f.title.textContent='PRIVATE user-generated conversation';
 assert.equal(f.classify({held:f.held,entries:f.entries}).status,'unknown');f.title.textContent=original;
 assert.equal(f.classify({held:f.held,entries:[...f.entries,{id:'other-known-source',text:original}]}).status,'ambiguous');
 f.setIds([f.title,{...f.title}]);assert.equal(f.classify({held:f.held,entries:f.entries}),null);f.setIds([f.title]);
 f.setDialogs([f.dialog,{...f.dialog}]);assert.equal(f.classify({held:f.held,entries:f.entries}),null);f.setDialogs([f.dialog]);
 f.dialog.getAttribute=k=>k==='role'?'dialog':k==='aria-labelledby'?':one: :two:':null;
 assert.equal(f.classify({held:f.held,entries:f.entries}),null);
 const g=fixture();g.title.tagName='DIV';assert.equal(g.classify({held:g.held,entries:g.entries}),null);
 for(const bad of ['0','unknown','2','']) {
  const x=fixture();x.dialog.style.opacity=bad;assert.equal(x.classify({held:x.held,entries:x.entries}),null);
 }
 const invisible=fixture();invisible.dialog.inert=true;assert.equal(invisible.classify({held:invisible.held,entries:invisible.entries}),null);
 const pointer=fixture();pointer.dialog.style.pointerEvents='none';assert.equal(pointer.classify({held:pointer.held,entries:pointer.entries}),null);
 for(const platform of ['linux','darwin','win32']) {
  const pin=helper.pins[platform],env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:pin.runner,NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_PROJECT_POLICY:'open-project',NANH_CODEX_PROJECT_ARTIFACT_SHA256:pin.artifact};
  assert.equal(helper.policy('chatgpt-desktop',platform,env),true);
  for(const key of Object.keys(env))assert.equal(helper.policy('chatgpt-desktop',platform,{...env,[key]:'wrong'}),false);
 }
 let owns=true,now=0,reads=0;const d=fixture();
 const page={evaluateHandle:async fn=>({value:vm.runInNewContext('('+fn.toString()+')',d.context)(),dispose:async()=>{}}),
  evaluate:async(fn,arg)=>{reads++;return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});}};
 const held={page,target:'PRIVATE_TARGET',frame:'PRIVATE_FRAME',loader:'PRIVATE_LOADER',url:'app://-/index.html'};
 let current=held;const opts={guard:async()=>owns,identity:async()=>current,same:(a,b)=>['page','target','frame','loader','url'].every(k=>a[k]===b[k]),deadline:Date.now()+1000};
 const good=await helper.observe(held,'darwin',opts);assert.equal(good.status,'matched');assert.equal(reads,2);assert.ok(!JSON.stringify(good).includes('PRIVATE'));
 for(const key of ['target','frame','loader','url','page']) {
  current={...held,[key]:'changed'};reads=0;assert.equal((await helper.observe(held,'darwin',opts)).status,'guard-rejected');assert.equal(reads,0);
 }
 current=held;owns=false;reads=0;assert.equal((await helper.observe(held,'darwin',opts)).status,'guard-rejected');assert.equal(reads,0);
 owns=true;reads=0;assert.equal((await helper.observe(held,'darwin',{...opts,deadline:Date.now()-1})).status,'guard-rejected');assert.equal(reads,0);
 page.evaluate=async(fn,arg)=>{reads++;if(reads===2)d.title.textContent='changed';return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});};
 const changed=await helper.observe(held,'darwin',opts);assert.equal(changed.status,'guard-rejected');assert.equal(changed.matchCount,null);
 assert.equal(new Set(catalog.entries.map(e=>e.id)).size,60);
 const windowsCatalog=require('./codex-dialog-title-catalog-windows.json');
 assert.equal(new Set(windowsCatalog.entries.map(e=>e.id)).size,60);
 assert.ok(windowsCatalog.entries.every(e=>catalog.entries.some(original=>original.id===e.id&&original.text===e.text)));
 d.title.textContent='Skip setup?';reads=0;
 page.evaluate=async(fn,arg)=>{reads++;return vm.runInNewContext('('+fn.toString()+')',d.context)({...arg,held:arg.held.value});};
 const windows=await helper.observe(held,'win32',opts);
 assert.equal(windows.status,'matched');assert.equal(reads,2);
 assert.equal(windows.platform,'windows');assert.equal(windows.artifactSha256,helper.pins.win32.artifact);
 assert.ok(catalog.entries.every(e=>!e.text.includes('{')&&/^[a-f0-9]{64}$/.test(e.sha)));
 console.log('PASS: static dialog titles, sr-only label projection, duplicates, immutable two reads, privacy; zero actions');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
