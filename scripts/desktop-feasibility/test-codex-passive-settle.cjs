'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/observe-renderer.cjs','utf8');
const start=source.indexOf('function heldMainGuard('),end=source.indexOf('function publishCodexBinding(',start);
function fixture(options={}) {
 let now=0,pages=[],reads=0,activations=0,mainReads=0;
 const page={url:()=> 'app://-/index.html',bringToFront:async()=>{activations++;}};
 const aux={url:()=>options.foreign?'app://-/foreign':'app://-/avatar-overlay'};
 pages=[page];
 const api=vm.runInNewContext(source.slice(start,end)+';({heldMainGuard,focusCapturedMain})',{
  Date:{now:()=>now},setTimeout,sameCorrelationIdentity:(a,b)=>a.key===b.key});
 const held={page,key:'main'};
 const browser={contexts:()=>[{pages:()=>pages}]};
 const identity=async p=>{
  reads++;
  if(p===page) {
   mainReads++;
   if(options.appear&&mainReads===1)pages=options.multiple?[page,aux,{}]:[page,aux];
   return {key:options.mainChanged&&mainReads>1?'replacement':'main',scope:{mainScope:true,
    focused:options.focusDelay?mainReads>=options.focusDelay:!options.unfocused,counts:{}}};
  }
  return {key:options.auxChanged&&reads>3?'replacement':'aux',scope:{mainScope:false,
   focused:!!options.auxFocused,counts:Object.fromEntries(['roleLegend','roleRadios','engineering','dialog','quickChatComposer','editable'].map(k=>[k,options.auxControl&&k==='dialog'?1:0]))}};
 };
 const guard=api.heldMainGuard(held,browser,()=>!options.ownerLost||reads<2,1000,
  url=>url==='app://-/avatar-overlay'?'avatarOverlay':'other',identity,async ms=>{now+=ms},false,false,!options.focusDelay&&!options.unfocused);
 return {api,guard,held,identity,pause:async ms=>{now+=ms},activations:()=>activations,reads:()=>reads};
}
(async()=>{
 let f=fixture({appear:true});f.guard.sealInitialActions();assert.equal(await f.guard(),false);
 f=fixture({appear:true});f.guard.sealInitialActions();assert.equal(f.guard.allowPassiveFolderSettle(),true);
 assert.equal(await f.guard(),true);assert.equal(f.guard.allowPassiveFolderSettle(),false);
 assert.equal(await f.guard(),true);assert.equal(f.activations(),0);
 for(const options of [{foreign:true},{multiple:true},{mainChanged:true},{auxChanged:true},{auxFocused:true},{auxControl:true},{ownerLost:true}]) {
  f=fixture({appear:true,...options});f.guard.sealInitialActions();f.guard.allowPassiveFolderSettle();
  assert.equal(await f.guard(),false,JSON.stringify(options));assert.equal(f.activations(),0);
 }
 f=fixture();assert.equal(f.guard.allowPassiveFolderSettle(),false,'unsealed action cannot grant ticket');
 f=fixture({appear:true});f.guard.sealInitialActions();f.guard.allowPassiveFolderSettle();f.guard.finishPassiveFolderSettle();assert.equal(await f.guard(),false,'ticket expires before subsequent actions');
 f=fixture({focusDelay:4});assert.equal(await f.api.focusCapturedMain(f.held,f.guard,1000,f.identity,(a,b)=>a.key===b.key,f.pause),true);assert.equal(f.activations(),1);
 for(const options of [{unfocused:true},{focusDelay:4,mainChanged:true},{focusDelay:4,ownerLost:true}]) {
  f=fixture(options);assert.equal(await f.api.focusCapturedMain(f.held,f.guard,1000,f.identity,(a,b)=>a.key===b.key,f.pause),false);
  assert(f.activations()<=1);assert(f.reads()<50);
 }
 console.log('passive folder appearance ticket and delayed focus fixtures PASS');
})().catch(error=>{console.error(error);process.exitCode=1;});
