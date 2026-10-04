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
 const retained={form,dialog:options.replaced?node(''):dialog};
 const context={document:{querySelectorAll:()=>options.multiple?[dialog,node('')]:[dialog],elementFromPoint:()=>options.covered?node(''):go},innerWidth:1000,innerHeight:1000,
  getComputedStyle:e=>({display:options.hidden&&e===dialog?'none':'block',visibility:'visible',pointerEvents:'auto'})};
 return vm.runInNewContext(`(${source.slice(begin,end).trim()})(control,retained)`,{...context,control:go,retained});
}
assert.equal(fixture().points.length,9);
for(const o of [{subtitle:'PRIVATE'},{label:'Skip'},{type:'submit'},{duplicate:true},{disabled:true},{replaced:true},{multiple:true},{hidden:true}])assert.equal(fixture(o),null);
assert.equal(fixture({covered:true}).points.length,0);
assert(!JSON.stringify(fixture()).includes('PRIVATE'));
console.log('PASS: source skip confirmation, duplicates, retained scope, overlays, actionability and privacy');
