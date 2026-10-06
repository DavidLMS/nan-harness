const assert=require('node:assert/strict');
const {codingScope}=require('./codex-onboarding.cjs');
function sample(options={}) {
 const make=(label,tag='BUTTON',role=null)=>({tagName:tag,isConnected:true,disabled:false,textContent:label,children:[],
  getAttribute:k=>k==='role'?role:null,closest:()=>null,contains:()=>false,
  getBoundingClientRect:()=>({left:10,top:10,width:30,height:20})});
 const controls=[make('Switch mode, current mode: ChatGPT'),make('Select project'),make('New chat')];
 if(!options.absent)controls.push(make('Codex',options.link?'A':'BUTTON'));
 if(options.duplicate)controls.push(make('Codex'));
 if(options.disabled)controls.at(-1).disabled=true;
 for(const e of controls)e.contains=x=>x===e;
 global.getComputedStyle=()=>({display:'block',visibility:'visible'});
 global.document={documentElement:{clientWidth:100,clientHeight:100},
  elementFromPoint:()=>options.intercepted?{}:controls.at(-1),
  querySelectorAll:s=>s==='*'?controls:s==='button'||s==='button,a,[role="menuitem"]'?controls:[]};
 return codingScope(true).navigation;
}
const good=sample();assert.equal(good.codexButtonCount,1);assert.equal(good.uniqueCodexRole,'button');assert.equal(good.uniqueCodexHitActionable,true);
assert.equal(sample({link:true}).uniqueCodexRole,'link');
for(const opts of [{duplicate:true},{absent:true},{disabled:true},{intercepted:true}])assert.equal(sample(opts).uniqueCodexHitActionable,false);
assert.equal(sample({duplicate:true}).uniqueCodexRole,'none');
assert.equal(JSON.stringify(good).includes('Switch mode'),false);
console.log('PASS navigation single/duplicate/absent/disabled/intercepted/link/privacy');
