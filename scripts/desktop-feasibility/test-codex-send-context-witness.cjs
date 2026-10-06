const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const crypto=require('node:crypto');
const sources={component:'function mct(){return null}',context:'async()=>null',submit:'async()=>null',prepare:'async()=>null',reserve:'reserve(e){return e}',scopeGetter:'get value(){return null}'};
const fixturePins=Object.fromEntries(Object.entries(sources).map(([k,v])=>[k,crypto.createHash('sha256').update(v).digest('hex')]));
const fixtureModule={exports:{}};
vm.runInNewContext(fs.readFileSync(require.resolve('./codex-send-context-witness.cjs'),'utf8'),{
 module:fixtureModule,exports:fixtureModule.exports,require:key=>key==='./codex-send-context-source-pins.json'?fixturePins:require(key),setTimeout,clearTimeout,Date});
const {create,LOCATE}=fixtureModule.exports;
function fixture(change=()=>{}) {
 const objects=new Map();let number=0,calls=0,guardLost=false;let onCall=null;
 const scalar=x=>x===undefined?{type:'undefined'}:x===null?{type:'object',subtype:'null',value:null}:{type:typeof x,value:x};
 const object=(data={},extra={})=>{const id='PRIVATE-'+(++number),remote={type:'object',objectId:id,...extra};objects.set(id,{data,internal:[]});return remote;};
 const fn=kind=>{const remote=object({}, {type:'function'});objects.get(remote.objectId).source=sources[kind];return remote;};
 const scopes=(fn,label,data)=>{
  const closed=object(data,{description:'Closure ('+label+')'}),list=object({'0':closed,length:scalar(1)});
  objects.get(fn.objectId).internal=[{name:'[[Scopes]]',value:list}];return closed;
 };
 const nil=scalar(null),undef=scalar(undefined),array=x=>object({'0':scalar(x),length:scalar(1)});
 const root='PRIVATE-root',project='PRIVATE-project';
 const editor=object(),controller=object({view:object({dom:editor})});
 const reserve=fn('reserve'),reservation=object({reserve}),cacheScope=scopes(reserve,'h3e',{t:nil});
 const scopeValue=object({kind:scalar('new'),entrypoint:scalar('home')});
 const getter=fn('scopeGetter');scopes(getter,'Ak',{o:scopeValue});const scope=object();objects.get(scope.objectId).accessor={value:getter};
 const context=fn('context'),settings=object({localProjectId:scalar(project),workspaceRoots:array(root)});
 const closure=scopes(context,'mct',{ic:settings,ai:scalar('local'),ui:scalar('local'),dt:scalar(root),
  Ro:object({hostId:scalar('local'),workspaceRoots:undef}),xe:scope,hn:controller});
 const prepare=fn('prepare'),prepareScope=scopes(prepare,'_ct',{B:undef,Zt:nil,Qt:undef});
 const submit=fn('submit'),handler=scopes(submit,'_3e',{c:nil,k:undef,D:scalar('local'),O:scalar(false),e:scope,T:prepare,E:reservation});
 const componentProps=object({selectedProject:object({type:scalar('local'),projectId:scalar(project)}),
  surfacePlacement:object({kind:scalar('home')}),localThreadPrewarmReservation:reservation,onSubmitLocal:submit});
 const utility=object({composerMode:scalar('local'),localRemoteExecutionTarget:object({hostId:scalar('local'),cwd:scalar(root)}),buildLocalContextForPrompt:context});
 const component=object(),utilityFiber=object(),componentType=fn('component');
 const found=object({component,componentType,componentProps,utilityFiber,utilityProps:utility,context,submit});
 const selection=object({cwd:scalar(root),workspaceRoots:array(root),projectAssignment:object({projectKind:scalar('local'),projectId:scalar(project)}),projectlessOutputDirectory:nil});
 const promise=object({}, {subtype:'promise'});objects.get(promise.objectId).internal=[{name:'[[PromiseState]]',value:scalar('fulfilled')},{name:'[[PromiseResult]]',value:selection}];
 const cached=object({hostId:scalar('local'),projectId:scalar(project),workspaceRoots:array(root),selection:promise});
 const data=v=>objects.get(v.objectId).data;
 const api={objects,scalar,object,fn,scopes,nil,undef,editor,controller,scopeValue,scope,getter,context,settings,closure,prepare,prepareScope,handler,
  componentProps,utility,component,componentType,found,selection,promise,cached,cacheScope,data};change(api);
 const session={async send(method,params){
  calls++;if(onCall)onCall(method,params);
  if(method==='Runtime.getProperties'){
   const o=objects.get(params.objectId);assert(o);
   const result=Object.entries(o.data).map(([name,value])=>({name,value,isOwn:true}));
   for(const [name,get] of Object.entries(o.accessor??{}))result.push({name,get,isOwn:true});
   return {result,internalProperties:o.internal};
  }
  if(method==='Runtime.callFunctionOn'){
   if(params.functionDeclaration===LOCATE){assert.equal(params.objectId,editor.objectId);return {result:found};}
   if(params.functionDeclaration.includes('Function.prototype.toString'))return {result:scalar(objects.get(params.objectId).source)};
   if(params.functionDeclaration.includes('this===other'))return {result:scalar(params.objectId===params.arguments[0].objectId)};
  }
  throw Error('PRIVATE unknown operation');
 }};
 return {...api,witness:create({session,editor,guard:()=>!guardLost,deadline:1000,projectId:project,cwd:root,now:()=>0}),calls:()=>calls,loseGuard:()=>guardLost=true,onCall:fn=>onCall=fn};
}
(async()=>{
 let f=fixture();let result=await f.witness.observe();assert.equal(result.verified,true);assert.equal(result.noPriorReservation,true);
 assert.equal(result.inputAuthorized,false);assert(!JSON.stringify(result).includes('PRIVATE'));
 result=await f.witness.observe({requireSelection:true});assert.equal(result.reason,'reservation-unavailable');
 f.data(f.cacheScope).t=f.cached;result=await f.witness.observe({requireSelection:true});assert.equal(result.verified,true);assert.equal(result.prewarmResolvedSelection,true);
 for(const [alter,reason] of [
  [f=>f.data(f.settings).existingWorkspaceRoot=f.scalar('PRIVATE-other'),'context-override'],
  [f=>f.data(f.settings).remoteProjectId=f.scalar('PRIVATE-other'),'context-override'],
  [f=>f.data(f.settings).cloudThreadPrototype=f.scalar(true),'context-override'],
  [f=>f.data(f.settings).aeonStartTarget=f.scalar('durable'),'context-override'],
  [f=>f.data(f.closure).ai=f.scalar('PRIVATE-host'),'host-or-cwd-mismatch'],
  [f=>f.data(f.handler).c=f.object({type:f.scalar('cloud')}),'follow-up'],
  [f=>f.data(f.scopeValue).kind=f.scalar('local'),'controller-mismatch'],
  [f=>f.data(f.componentProps).homeRunLocationRemoteProject=f.object({}),'remote-override'],
  [f=>f.data(f.utility).existingWorkspace=f.object({}),'existing-workspace'],
  [f=>f.data(f.utility).composerMode=f.scalar('worktree'),'mode-mismatch'],
  [f=>f.data(f.prepareScope).B=f.fn('prepare'),'prepare-override'],
  [f=>f.data(f.prepareScope).Zt=f.scalar('account'),'prepare-override'],
  [f=>f.data(f.prepareScope).Zt=f.scalar('runtime'),'prepare-override'],
  [f=>f.data(f.prepareScope).Zt=f.scalar('unknown'),'prepare-override'],
  [f=>f.data(f.prepareScope).Zt=f.scalar(true),'prepare-override'],
  [f=>f.data(f.prepareScope).Zt=f.scalar(0),'prepare-override'],
  [f=>f.objects.get(f.componentType.objectId).source+='PRIVATE','source-mismatch'],
  [f=>f.data(f.cacheScope).t=f.cached,'verified'],
  [f=>{f.data(f.cacheScope).t=f.cached;f.data(f.selection).cwd=f.scalar('PRIVATE-other')},'root-mismatch'],
  [f=>{f.data(f.cacheScope).t=f.cached;f.objects.get(f.promise.objectId).internal[0].value=f.scalar('pending')},'reservation-pending'],
  [f=>f.data(f.scopeValue).entrypoint=f.scalar('quick-chat'),'controller-mismatch'],
 ]){
  const fixtureValue=fixture(alter);const actual=await fixtureValue.witness.observe();assert.equal(actual.reason,reason);
  assert.equal(actual.inputAuthorized,false);assert(!JSON.stringify(actual).includes('PRIVATE'));
 }
 for(const value of [true,'false',0,{},'unknown']){
  const f=fixture();f.data(f.data(f.componentProps).surfacePlacement).quickSend=typeof value==='object'?f.object({}):f.scalar(value);
  assert.equal((await f.witness.observe()).reason,'controller-mismatch');
 }
 for(const value of [false,null,undefined]){
  const f=fixture();f.data(f.data(f.componentProps).surfacePlacement).quickSend=f.scalar(value);
  assert.equal((await f.witness.observe()).verified,true);
 }
 {
  const f=fixture();f.onCall(()=>{if(f.calls()===4)f.loseGuard()});
  assert.equal((await f.witness.observe()).reason,'deadline-or-owner');
 }
 {
  const f=fixture();let changed=false;f.onCall((method,params)=>{
   if(!changed&&method==='Runtime.getProperties'&&params.objectId===f.settings.objectId){
    changed=true;f.data(f.settings).localProjectId=f.scalar('PRIVATE-other');
   }
  });assert.equal((await f.witness.observe()).reason,'project-mismatch');
 }
 console.log('Passive source-pinned context, host/controller/override and prewarm fixtures passed');
})().catch(e=>{console.error(e);process.exitCode=1});
