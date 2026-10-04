'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const code=fs.readFileSync(__dirname+'/codex-project-control-transition.cjs','utf8');
function fixture(){
 const scope={module:{exports:{}},getComputedStyle:()=>({display:'block',visibility:'visible'})};
 const node=(attrs={})=>({attrs,isConnected:true,disabled:false,tagName:'BUTTON',type:'button',tabIndex:0,
  getAttribute(k){return this.attrs[k]??null;},getBoundingClientRect:()=>({left:10,top:10,width:30,height:20}),closest:()=>null,
  classList:{contains:()=>true},querySelectorAll:()=>[]});
 const home=node(),editor=node(),old=node(),button=node({'data-slot':'popover-trigger','aria-haspopup':'dialog','aria-expanded':'false','aria-label':'Change project: synthetic'});
 const clear=node({'data-clear-project-button':'','aria-label':"Don't work in a project"});clear.tabIndex=-1;
 const wrapper=node();let homes=[home],editors=[editor],controls=[old],overlays=[];
 home.contains=e=>[editor,old,button,wrapper,clear].includes(e);
 home.querySelectorAll=s=>s.startsWith('.ProseMirror')?editors:s.includes('clear-project-button')?(old.isConnected?[]:[clear]):controls;
 old.closest=()=>null;button.closest=s=>s==='button'?button:s.startsWith('div.')?wrapper:null;
 button.querySelectorAll=()=>[node()];wrapper.querySelectorAll=s=>s==='button'?[button,clear]:s.includes('clear-project-button')?[clear]:[button];
 scope.document={querySelectorAll:s=>s.startsWith('[data-codex')?homes:overlays};
 vm.runInNewContext(code,scope);const api=scope.module.exports;
 const held={home,editor,control:old,button:old};assert.equal(api.prepare(held),true);
 old.isConnected=false;controls=[button];const menu=node();menu.isConnected=false;
 return {api,held,button,clear,wrapper,editor,menu,capture:()=>api.capture(held,{projectName:'synthetic',originalMenu:menu}),
  wrongHome(){homes=[node()];},wrongEditor(){editors=[node()];},foreign(){overlays=[node()];},duplicateClear(){wrapper.querySelectorAll=s=>s.includes('clear-project-button')?[clear,clear]:[button];}};
}
let tests=0;function test(name,run){run();tests++;console.log('PASS '+name);}
test('exact declared control replacement consumes once',()=>{const f=fixture();const next=f.capture();assert.equal(next.home,f.held.home);assert.equal(next.editor,f.editor);assert.equal(next.button,f.button);assert.equal(f.capture(),null);});
test('wrong home editor and foreign overlay reject',()=>{for(const change of ['wrongHome','wrongEditor','foreign']){const f=fixture();f[change]();assert.equal(f.capture(),null);}});
test('wrong wrapper clear count and label reject',()=>{for(const change of ['wrapper','clear','label']){const f=fixture();if(change==='wrapper')f.button.closest=s=>s==='button'?f.button:null;if(change==='clear')f.duplicateClear();if(change==='label')f.button.attrs['aria-label']='Change project: unrelated';assert.equal(f.capture(),null);}});
test('connected original trigger and missing pre-action capability reject',()=>{for(const change of ['connected','capability']){const f=fixture();if(change==='connected')f.held.button.isConnected=true;else delete f.held.declaredProjectlessControl;assert.equal(f.capture(),null);}});
test('popup still visible and disabled new control reject',()=>{for(const change of ['popup','disabled']){const f=fixture();if(change==='popup')f.menu.isConnected=true;else f.button.disabled=true;assert.equal(f.capture(),null);}});
console.log(tests+' transition fixture groups passed');
