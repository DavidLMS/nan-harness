const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(`${__dirname}/codex-onboarding.cjs`,'utf8');
const begin=source.indexOf('function skipConfirmation('),end=source.indexOf('// The frozen role Continue',begin);
function fixture(options={}) {
 const node=(text,attrs={})=>({textContent:text,children:[],isConnected:true,disabled:false,clientLeft:0,clientTop:0,clientWidth:100,clientHeight:30,
  getBoundingClientRect:()=>({left:10,top:10,width:100,height:30}),getAttribute:k=>attrs[k]??null,closest:()=>null,contains:e=>false});
 const title=node('Skip setup?'),subtitle=node(options.subtitle??'You’ll go straight to ChatGPT');
 const keep=node('Keep setting up',{type:'submit'}),go=node(options.label??'Go to ChatGPT',{type:options.type??'button'});
 const form=node(''),dialog=node('',{role:'dialog'});dialog.querySelectorAll=()=>[form];
 form.querySelectorAll=s=>s==='button'?[keep,go,...(options.duplicate?[node('Go to ChatGPT',{type:'button'})]:[])]:s==='h1,h2,h3,[role="heading"]'?[title]:[title,subtitle,keep,go];
 go.disabled=!!options.disabled; go.parentElement=form;form.parentElement=dialog;
 const retained={form,dialog:options.replaced?node(''):dialog,diagnostic:options.diagnostic};
 const context={document:{querySelectorAll:()=>options.multiple?[dialog,node('')]:[dialog],elementFromPoint:()=>options.covered?node(''):go},innerWidth:1000,innerHeight:1000,
  getComputedStyle:e=>({display:options.hidden&&e===dialog?'none':'block',visibility:'visible',pointerEvents:'auto'})};
 return vm.runInNewContext(`(${source.slice(begin,end).trim()})(control,retained)`,{...context,control:go,retained});
}
assert.equal(fixture().points.length,9);
for(const o of [{subtitle:'PRIVATE'},{label:'Skip'},{type:'submit'},{duplicate:true},{disabled:true},{replaced:true},{multiple:true},{hidden:true}])assert.equal(fixture(o),null);
assert.equal(fixture({covered:true}).points.length,0);
assert(!JSON.stringify(fixture()).includes('PRIVATE'));
console.log('PASS: source skip confirmation, duplicates, retained scope, overlays, actionability and privacy');

// Execute the actual release gate, not a duplicate admission implementation.
const policyStart=source.indexOf('const skipAdmitted='),policyEnd=source.indexOf(';',policyStart)+1;
function admitted(platform,digest) {
 return vm.runInNewContext(`(()=>{${source.slice(policyStart,policyEnd)}return skipAdmitted;})()`,
  {process:{platform,env:{NANH_CODEX_PROJECT_ARTIFACT_SHA256:digest}}});
}
const macPin='f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7';
assert.equal(admitted('darwin',macPin),true);
for(const [platform,digest] of [['linux',macPin],['win32',macPin],['darwin',undefined],['darwin',macPin+'0'],['darwin',macPin.toUpperCase()]])
 assert.equal(admitted(platform,digest),false);
assert.equal(admitted('linux','ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c'),true);
assert.equal(admitted('win32','f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87'),true);
console.log('PASS: exact platform-bound macOS Skip release, other release admission preserved');

assert.equal(fixture({subtitle:'PRIVATE',diagnostic:true}).rejection,'subtitle');
assert.equal(fixture({replaced:true,diagnostic:true}).rejection,'retained-identity');
assert.equal(fixture({multiple:true,diagnostic:true}).rejection,'overlay-count');
