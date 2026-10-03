const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
function fixture(kind,options={}) {
 const doc={hasFocus:()=>!options.unfocused};let controls=[],forms=[],divs=[],titles=[];
 const make=(text='',tokens=[])=>({innerText:text,tagName:'H2',ownerDocument:doc,isConnected:true,
  classList:{contains:t=>tokens.includes(t)},getBoundingClientRect:()=>({width:10,height:10}),
  getAttribute:()=>null,contains:()=>false,querySelectorAll:()=>[]});
 const button=(label,type='button')=>({...make(label),tagName:'BUTTON',getAttribute:k=>k==='type'?type:null});
 const dialog={...make(),getAttribute:k=>k==='role'?'dialog':null};
 if(kind==='all-set') {
  titles=[make("You're all set",['text-3xl','leading-9','font-normal'])];controls=[button('Continue','submit')];
  const links=['https://openai.com/terms','https://openai.com/privacy'].map(href=>({...make('', ['underline']),getAttribute:k=>k==='href'?href:null}));
  forms=[{...make('', ['m-auto','flex','w-full','shrink-0','flex-col','items-center','justify-between','py-4']),
   contains:e=>e===titles[0],querySelectorAll:s=>s==='button'?controls:s==='a'?links:[]}];
 } else if(kind==='computer-history') {
  titles=[make('Connect Computer History',['heading-dialog','select-none'])];
  controls=[button('Customize apps'),button('Allow access','submit'),button('Not now')];
  forms=[{...make('', ['pointer-events-auto','relative','hide-scrollbar','flex','flex-col','gap-6','overflow-y-auto','pb-10']),contains:()=>true}];
 } else if(kind==='imported-setup') {
  titles=[make('Continue with your existing setup')];controls=[button('Continue'),button('Not now')];
  divs=[{...make('', ['flex','w-full','max-w-xl','flex-col','gap-6']),contains:()=>true}];
 } else if(kind==='project-import') {
  titles=[make('Select settings to import')];controls=[button('Continue'),button('Not now')];
  divs=[{...make('', ['max-h-[min(720px,calc(100vh-64px))]','overflow-hidden']),contains:()=>true,
   querySelectorAll:s=>s.startsWith('[role="checkbox"]')?[{}]:[]}];
 } else titles=[make('Unknown dialog')];
 if(options.wrongControls)controls=[button('PRIVATE_LOOKALIKE')];
 if(options.duplicateTitle)titles=[...titles,titles[0]];
 if(options.wrongLayout) {forms=[];divs=[];}
 dialog.querySelectorAll=s=>s==='button'?controls:s==='form'?forms:s==='div'?divs:titles;
 doc.querySelectorAll=()=>options.multiple?[dialog,make()]:[dialog];
 const globals={document:doc,getComputedStyle:()=>({display:'block',visibility:'visible'})};
 const load=()=>vm.runInNewContext('('+source.slice(source.indexOf('function matchLinuxDialog('),source.indexOf('async function observeLinuxDialog('))+')',globals);
 // Evaluate a standalone serialized browser function with no Node helper globals.
 const match=load();
 return {doc,dialog,match,globals};
}
async function main() {
 for(const kind of ['all-set','imported-setup','computer-history','project-import']) {
  const f=fixture(kind),result=f.match({document:f.doc,dialog:f.dialog});
  assert.equal(result.status,'matched');assert.equal(result.candidate,kind);
  assert.equal(Object.values(result.sourceCount).filter(x=>x===1).length,3);
  for(const option of [{wrongControls:true},{duplicateTitle:true},{wrongLayout:true}]) {
   const bad=fixture(kind,option),value=bad.match({document:bad.doc,dialog:bad.dialog});
   assert.equal(value.status,'other');
  }
 }
 for(const option of [{multiple:true},{unfocused:true}]) {
  const f=fixture('all-set',option);assert.equal(f.match({document:f.doc,dialog:f.dialog}),null);
 }
 const f=fixture('all-set');assert.equal(f.match({document:{},dialog:f.dialog}),null);
 assert.equal(f.match({document:f.doc,dialog:{...f.dialog}}),null);
 let now=0,owner=true,reads=0,pages,identity;
 const globals={Date:{now:()=>now},document:f.doc,getComputedStyle:f.globals.getComputedStyle,
  onboardingTrial:vm.runInNewContext('('+source.slice(source.indexOf('function onboardingTrial('),source.indexOf('function onboardingDeadline(')).trim()+')'),
  officialInitialMain:x=>x.url==='app://-/index.html',sameCorrelationIdentity:(a,b)=>['page','url','target','frame','loader'].every(k=>a[k]===b[k])};
 const h=vm.runInNewContext('(()=>{'+source.slice(source.indexOf('function linuxDialogPolicy('),source.indexOf('async function run()'))+';return {observeLinuxDialog,linuxDialogPolicy}})()',globals);
 const page={evaluateHandle:async()=>({dispose:async()=>{},value:{document:f.doc,dialog:f.dialog}}),
  evaluate:async(fn,handle)=>{reads++;return vm.runInNewContext('('+fn.toString()+')',globals)(handle.value);}};
 pages=[page];const held={page,url:'app://-/index.html',target:'PRIVATE_TARGET',frame:'PRIVATE_FRAME',loader:'PRIVATE_LOADER'};
 identity=async()=>held;const browser={contexts:()=>[{pages:()=>pages}]};
 const good=await h.observeLinuxDialog(held,browser,()=>owner,100,()=>identity());
 assert.equal(good.status,'matched');assert.equal(reads,2);assert.ok(!JSON.stringify(good).includes('PRIVATE'));
 assert.equal(h.linuxDialogPolicy('chatgpt-desktop','linux',{GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Linux',NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_PROJECT_POLICY:'open-project',NANH_CODEX_PROJECT_ARTIFACT_SHA256:'e0174d8d0a5f4141145458c814f3c2d863dd67e942b868785a1f5dac9cba3e16'}),true);
 for(const reason of ['owner','pages','loader','frame','url','deadline']) {
  owner=reason!=='owner';pages=reason==='pages'?[page,page]:[page];now=reason==='deadline'?100:0;
  identity=async()=>reason==='loader'?{...held,loader:'replacement'}:reason==='frame'?{...held,frame:'replacement'}:reason==='url'?{...held,url:'app://-/other'}:held;
  const bad=await h.observeLinuxDialog(held,browser,()=>owner,100,()=>identity());
  assert.equal(bad.status,'guard-rejected');assert.ok(Object.values(bad.sourceCount).every(x=>x===null));
 }
 owner=true;pages=[page];now=0;identity=async()=>held;reads=0;
 page.evaluate=async(fn,handle)=>{reads++;if(reads===2)handle.value.dialog={...f.dialog};return vm.runInNewContext('('+fn.toString()+')',globals)(handle.value);};
 assert.equal((await h.observeLinuxDialog(held,browser,()=>owner,100,()=>identity())).status,'guard-rejected');
 console.log('PASS: Linux startup dialog source predicates, standalone callbacks and immutable two-read ownership; no actions');
}
main().catch(e=>{console.error(e);process.exitCode=1;});
