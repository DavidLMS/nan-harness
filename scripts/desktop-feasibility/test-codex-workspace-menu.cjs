'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const sandbox={exports:{},innerWidth:800,innerHeight:600,getComputedStyle:()=>({display:'block',visibility:'visible'}),require:()=>{throw Error('fixture must not read state');}};
vm.runInNewContext(fs.readFileSync(require('node:path').join(__dirname,'codex-workspace-menu.cjs'),'utf8'),sandbox);
function fixture() {
  const element=(attrs={})=>({attrs,isConnected:true,disabled:false,classList:{contains:c=>c==='ProseMirror'},
    getAttribute(k){return this.attrs[k]??null;},getBoundingClientRect:()=>({left:20,top:30,width:40,height:20}),
    closest:()=>null,contains(e){return e===this;}});
  const home=element({'data-codex-composer-root':'','data-composer-placement':'home'}),editor=element({contenteditable:'true'});
  const button=element({'aria-expanded':'false','aria-controls':'fixture-popup','data-composer-navigation-target':'workspace-project','data-slot':'popover-trigger','aria-haspopup':'dialog'}),control=button;
  control.closest=s=>s==='button'?button:null;home.contains=e=>[home,editor,control,button].includes(e);
  let homes=[home],editors=[editor],controls=[control],menus=[],overlays=[],hit=button;
  home.querySelectorAll=s=>s.startsWith('.ProseMirror')?editors:controls;
  sandbox.document={querySelectorAll:s=>s.startsWith('[data-codex')?homes:s==='[cmdk-root]'?menus:overlays,elementFromPoint:()=>hit};
  const held={home,editor,control,button},popup=element();popup.id='fixture-popup';
  const menu=element();menu.closest=s=>s.startsWith('[role=')?popup:null;popup.contains=e=>e===menu||e===popup;
  return {held,editor,button,control,menu,popup,sample:opened=>sandbox.exports.sample(held,{opened,menu}),
    open(){button.attrs['aria-expanded']='true';menus=[menu];overlays=[popup];},
    foreignOverlay(){overlays.push(element());},wrongHit(){hit=element();},duplicate(){homes.push(home);},replaceEditor(){editors=[element({contenteditable:'true'})];},
    capture:()=>sandbox.exports.capture()};
}
let groups=0;function test(name,run){run();groups++;console.log('PASS '+name);}
test('exact retained home control has an ordinary center hit',()=>{const f=fixture();assert.equal(f.capture().button,f.button);assert.equal(f.sample(false).matched,true);});
test('foreign hit duplicate home and replaced editor reject before input',()=>{for(const change of ['wrongHit','duplicate','replaceEditor']){const f=fixture();f[change]();assert.equal(f.sample(false).matched,false);}});
test('disabled detached and changed source control reject',()=>{for(const change of ['disabled','detached','marker']){const f=fixture();if(change==='disabled')f.button.disabled=true;if(change==='detached')f.editor.isConnected=false;if(change==='marker')f.control.attrs['data-composer-navigation-target']='other';assert.equal(f.sample(false).matched,false);}});
test('popup requires exact control linkage and no foreign overlay',()=>{const f=fixture();f.open();assert.equal(f.sample(true).matched,true);f.popup.id='other';assert.equal(f.sample(true).matched,false);f.popup.id='fixture-popup';f.foreignOverlay();assert.equal(f.sample(true).matched,false);});
(async()=>{
  let queries=0;
  const page={evaluateHandle:async()=>{queries++;throw Error('guard must prevent query');}};
  const denied=await sandbox.exports.run(page,async()=>false,()=>true,Date.now()+1000,{},'/private');
  assert.equal(queries,0);assert.equal(denied.clickAttempted,false);assert.equal(denied.sendAuthorized,false);
  const expired=await sandbox.exports.run(page,async()=>true,()=>true,Date.now()-1,{},'/private');
  assert.equal(queries,0);assert.equal(expired.clickAttempted,false);
  assert.equal(JSON.stringify(denied).includes('/private'),false);
  console.log('PASS expired or revoked endpoint cannot query or dispatch');
  console.log((groups+1)+' workspace-menu fixture groups passed');
})().catch(e=>{console.error(e);process.exitCode=1;});
