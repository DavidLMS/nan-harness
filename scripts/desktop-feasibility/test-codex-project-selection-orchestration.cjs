'use strict';
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const base=path.resolve(process.argv[2]||__dirname);
const contextModule=require(path.join(base,'codex-context-session.cjs'));
const choice=require(path.join(base,'codex-project-choice.cjs'));
async function fixture(kind='normal'){
 let clock=0,owner=true,opened=false,selected=false,buttonClicks=0,itemClicks=0,detached=0,released=0,witnesses=0,loader='original',reopened=false;
 const disposed=[],proofPhases=[],calls=[],handles=[];let currentMenu,currentPopup;
 function node(name,attrs={},rect={left:20,top:20,width:100,height:30}){
  return {name,attrs,isConnected:true,disabled:false,children:[],classList:{contains:c=>name==='editor'&&c==='ProseMirror'},
   getAttribute(k){return this.attrs[k]??null},hasAttribute(k){return k in this.attrs},getBoundingClientRect(){return rect},
   closest(s){if(s==='[inert]')return null;if(s==='button')return this===button?button:null;if(s.includes('popover-content'))return this===currentMenu?currentPopup:null;return null},
   contains(e){return e===this||this.children.includes(e)||this.children.some(c=>c.contains(e))},
   querySelectorAll(s){if(this===home)return s.includes('ProseMirror')?[liveEditor]:[button];if(this===currentMenu)return [list];if(this===list)return [item];return []}};
 }
 const home=node('home',{'data-codex-composer-root':'','data-composer-placement':'home'});
 const editor=node('editor',{contenteditable:'true'});let liveEditor=editor;
 const button=node('button',{'data-composer-navigation-target':'workspace-project','data-slot':'popover-trigger','aria-haspopup':'dialog','aria-expanded':'false','aria-controls':'original-popup'});
 const list=node('list'),item=node('item',{'cmdk-item':'',role:'option','data-value':'private-ID'});
 home.children=[editor,button];list.children=[item];
 const document={querySelectorAll(s){if(s.includes('data-codex-composer-root'))return [home];if(s==='[cmdk-root]')return opened?[currentMenu]:[];return opened?[currentPopup]:[]},elementFromPoint(){return opened?item:button}};
 const globals={document,getComputedStyle:()=>({display:'block',visibility:'visible'}),innerWidth:1000,innerHeight:800};
 const evaluate=(fn,args,held)=>vm.runInNewContext('('+fn.toString()+')(held,args)',{...globals,held,args});
 function open(){opened=true;button.attrs['aria-expanded']='true';currentPopup=node('popup');currentPopup.id=kind==='foreign-reopen'&&reopened?'foreign-popup':'original-popup';currentMenu=node(reopened?'new-menu':'old-menu',{'cmdk-root':''});currentMenu.children=[list];currentPopup.children=[currentMenu];}
 function close(){opened=false;button.attrs['aria-expanded']='false';currentMenu.isConnected=false;currentPopup.isConnected=false;}
 class Handle{
  constructor(value,label){this.value=value;this.label=label||value?.name||'loan';this.closed=false;handles.push(this);}
  async evaluate(fn,args){calls.push('pw-evaluate');if(this.closed)throw Error('disposed');return evaluate(fn,args&&{...args,menu:args.menu?.value??args.menu},this.value)}
  async getProperty(key){calls.push('pw-get-property:'+key);if(kind==='choice-property-delay'&&key==='item')clock=1000;return new Handle(this.value[key],key)}
  asElement(){return this.value?.getBoundingClientRect?this:null}
  async dispose(){if(!this.closed){this.closed=true;disposed.push(this.label)}}
  async click(){assert(clock<1000&&owner,'no click beyond original authority');
   if(this.value===button){buttonClicks++;if(kind==='initial-button-uncertain'&&buttonClicks===1)throw Error('uncertain');if(buttonClicks===2){reopened=true;if(kind==='reopen-button-uncertain')throw Error('uncertain')}open();}
   else if(this.value===item){itemClicks++;assert.equal(itemClicks,1);selected=true;if(kind!=='popup-stays-open')close();if(kind==='owner-after-selection')owner=false;if(kind==='loader-after-selection')loader='replacement';if(kind==='editor-after-selection'){liveEditor=node('editor',{contenteditable:'true'});home.children=[liveEditor,button];editor.isConnected=false;}if(kind==='selection-uncertain')throw Error('uncertain');}
   else throw Error('foreign click');
  }
 }
 let cdpHeld,selectionObservations=0;
 const session={async send(method,params){calls.push(method);
  if(method==='Target.getTargetInfo')return {targetInfo:{targetId:'original-target'}};
  if(method==='Page.getFrameTree')return {frameTree:{frame:{id:'original-frame',loaderId:loader,url:'app://original'}}};
  if(method==='Runtime.evaluate'){cdpHeld=vm.runInNewContext(params.expression,globals);return {result:{objectId:'held-dom'}};}
  if(method==='Runtime.getProperties'){if(kind==='prepare-properties-delay')clock=1000;return {result:[{name:'editor',value:{objectId:'held-editor'}}]};}
  if(method==='Runtime.callFunctionOn'){assert.equal(params.objectId,'held-dom');return {result:{value:vm.runInNewContext('('+params.functionDeclaration+').call(held)',{...globals,held:cdpHeld})}};}
  if(method==='Runtime.releaseObjectGroup'){released++;return {};}
  throw Error('unexpected method');},async detach(){detached++}};
 const page={url:()=> 'app://original',context:()=>({newCDPSession:async()=>session}),
  async evaluateHandle(fn,args){return new Handle(evaluate(fn,undefined,args&&{...args,menu:args.menu?.value??args.menu}),args?'choice-loan':'pw-loan')},
  locator(){return {count:async()=>opened?1:0,elementHandle:async()=>new Handle(currentMenu)}},
  async evaluate(){selectionObservations++;if(kind==='selection-race'&&selectionObservations===2)return {status:'observed',selectedIdCorrelated:true};return selected?{status:'observed',selectedIdCorrelated:true}:{status:'blocked',reason:'selected-id',selectedItemCount:0,matchingItemCount:1}}};
 const exports={};
 const mockedContext={create(options){const original=options.pwProof;const loan=contextModule.create({...options,now:()=>clock,pwProof:async held=>{proofPhases.push(opened?(reopened?'new':'old'):'closed');return original(held)},makeWitness:()=>({observe:async()=>{witnesses++;return {verified:true,inputAuthorized:false}}})});const prepare=loan.prepare;loan.prepare=async held=>{const r=await prepare(held);if(kind==='owner-after-prepare')owner=false;return r};return loan}};
 let snapshots=0;
 const project={id:'private-ID',name:'Owned',rootPaths:['/owned'],createdAt:1,updatedAt:1};
 const initialState={'local-projects':{'private-ID':project}};
 const selectedState={'local-projects':{'private-ID':{...project}},'selected-project':{type:'local',projectId:'private-ID'}};
 if(kind==='changed-project-record')selectedState['local-projects']['private-ID'].updatedAt=2;
 if(kind==='wrong-stored-selection')selectedState['selected-project'].projectId='other';
 const identity={dev:1,ino:2,uid:3,size:4,mtimeNs:5,ctimeNs:6,mode:7,nlink:1};
 const fixtureAuthority={verify:()=>owner&&clock<1000,close(){calls.push('profile-close')},snapshotPair(){snapshots++;
   const value=selected?selectedState:initialState;
   const row={value,identity,digest:kind==='baseline-drift'&&snapshots===2?'changed-before-input':selected?'selected':'initial'};
   return {first:row,second:row};}};
 const profile={};let profileSource=fs.readFileSync(path.join(base,'codex-profile-state.cjs'),'utf8');
 const begin=profileSource.indexOf('function authority('),end=profileSource.indexOf('exports.authority=authority;');
 profileSource=profileSource.slice(0,begin)+'function authority(){return fixtureAuthority;}\n'+profileSource.slice(end);
 vm.runInNewContext(profileSource,{exports:profile,fixtureAuthority,Date:{now:()=>clock},require:n=>n==='./codex-selected-project.cjs'?{sample(){}}:require(n),process,Buffer,TextDecoder});

 vm.runInNewContext(fs.readFileSync(path.join(base,'codex-workspace-menu.cjs'),'utf8'),{exports,Date:{now:()=>clock},require:n=>n==='./codex-context-session.cjs'?mockedContext:n==='./codex-profile-state.cjs'?profile:n==='./codex-project-choice.cjs'?choice:require(path.join(base,n))});
 const result=await exports.run(page,async()=>owner,()=>owner,1000,{directories:[{path:'/owned'}]},'/owned',{frozenLinuxTrial:true});
 assert.equal(detached,1,kind+' detached');assert.equal(released,1,kind+' released');assert(disposed.includes('pw-loan'));assert(disposed.includes('button'));assert.equal(result.sendAuthorized,false);assert(!JSON.stringify(result).includes('private-ID'));assert(!JSON.stringify(result).includes('/owned'));assert(handles.every(h=>h.closed),kind+' every acquired PW handle disposed');
 assert(buttonClicks<=2&&itemClicks<=1,kind+' no retry');
 return {result,buttonClicks,itemClicks,witnesses,proofPhases,disposed};
}
(async()=>{
 const ok=await fixture();assert.equal(ok.result.status,'observed');assert.equal(ok.result.selectionClickCompleted,true);assert.equal(ok.buttonClicks,2);assert.equal(ok.itemClicks,1);assert.equal(ok.witnesses,1);for(const p of ['closed','old','new'])assert(ok.proofPhases.includes(p),p);assert(ok.disposed.includes('old-menu'));assert(ok.disposed.includes('new-menu'));assert(ok.disposed.includes('choice-loan'));assert(ok.disposed.includes('item'));
 for(const kind of ['owner-after-prepare','prepare-properties-delay','choice-property-delay','initial-button-uncertain','owner-after-selection','loader-after-selection','editor-after-selection','selection-uncertain','reopen-button-uncertain','foreign-reopen','popup-stays-open','changed-project-record','wrong-stored-selection','baseline-drift','selection-race']){
  const r=await fixture(kind);assert.equal(r.witnesses,0,kind+' witness blocked');if(!['changed-project-record','wrong-stored-selection','baseline-drift','selection-race'].includes(kind))assert.notEqual(r.result.selectionClickCompleted,true,kind);else assert.equal(r.result.profileStateObservation.status,'blocked');
  if(['owner-after-prepare','prepare-properties-delay'].includes(kind))assert.equal(r.buttonClicks,0,kind);
  if(['choice-property-delay','initial-button-uncertain','baseline-drift','selection-race'].includes(kind))assert.equal(r.itemClicks,0,kind);
  if(['owner-after-selection','loader-after-selection','editor-after-selection','selection-uncertain','popup-stays-open'].includes(kind))assert.equal(r.buttonClicks,1,kind+' no reopen');
 }
 console.log('16 workspace/profile/CDP popup, deadline, uncertainty and cleanup cases passed');
})().catch(e=>{console.error(e);process.exitCode=1});
