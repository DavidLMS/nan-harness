// Passive source-pinned runtime witness. No vendor function or accessor is invoked.
const crypto=require('node:crypto');
const pins=require('./codex-send-context-source-pins.json');
// Only owned DOM/React data descriptors are traversed; no getter or vendor callback.
const LOCATE=`function(){
 const data=(o,k)=>{const d=Object.getOwnPropertyDescriptor(o,k);if(!d||!('value'in d))throw Error('data unavailable');return d.value};
 const keys=Reflect.ownKeys(this).filter(k=>typeof k==='string'&&k.startsWith('__reactFiber$'));
 if(keys.length!==1)throw Error('fiber unavailable');
 const chain=f=>{const seen=new Set(),rows=[];while(f){if(seen.has(f)||rows.length>=64)throw Error('fiber limit');seen.add(f);rows.push(f);f=data(f,'return')};return rows};
 const committed=f=>{const rows=chain(f),root=rows.at(-1);return data(data(root,'stateNode'),'current')===root?rows:null};
 let fiber=data(this,keys[0]),rows=committed(fiber);
 if(!rows){fiber=data(fiber,'alternate');if(!fiber||(rows=committed(fiber))===null)throw Error('uncommitted fiber')};
 if(data(fiber,'stateNode')!==this)throw Error('editor identity');
 const matches=rows.filter(f=>{const t=data(f,'type');return typeof t==='function'&&data(t,'name')==='mct'});
 if(matches.length!==1)throw Error('component unavailable');
 const component=matches[0],pending=[data(component,'child')],seen=new Set(),utilities=[];
 while(pending.length){const f=pending.pop();if(!f)continue;if(seen.has(f)||seen.size>=512)throw Error('fiber limit');seen.add(f);
  const props=data(f,'memoizedProps');if(props&&typeof props==='object'){
   const d=Object.getOwnPropertyDescriptor(props,'utilityBarProps');if(d){if(!('value'in d))throw Error('accessor');utilities.push({fiber:f,props:d.value})}
  };pending.push(data(f,'child'),data(f,'sibling'));
 }
 if(utilities.length!==1)throw Error('utility unavailable');
 const utility=utilities[0],componentProps=data(component,'memoizedProps');
 return {component,componentType:data(component,'type'),componentProps,utilityFiber:utility.fiber,utilityProps:utility.props,
  context:data(utility.props,'buildLocalContextForPrompt'),submit:data(componentProps,'onSubmitLocal')};
}`;
const TOSTRING='function(){return Function.prototype.toString.call(this)}';
const EQUAL='function(other){return this===other}';
function failure(reason){return {verified:false,reason,inputAuthorized:false};}
function ref(value){if(!value?.objectId)throw Error('descriptor-unavailable');return value.objectId;}
function nil(value){return value?.type==='undefined'||value?.subtype==='null';}
function primitive(value,expected){return value?.value===expected;}
function create({session,editor,guard,deadline,projectId,cwd,now=Date.now}) {
 let retained=null;
 const timely=async()=>{if(now()>=deadline||await guard()!==true||now()>=deadline)throw Error('deadline-or-owner');};
 const send=async(method,params)=>{
  await timely();let timer;
  try {const result=await Promise.race([session.send(method,params),new Promise((_,reject)=>{
   timer=setTimeout(()=>reject(Error('deadline-or-owner')),Math.max(1,deadline-now()));
  })]);await timely();if(result.exceptionDetails)throw Error('runtime-unavailable');return result;
  } finally {clearTimeout(timer);}
 };
 const properties=async(value)=>{
  const result=await send('Runtime.getProperties',{objectId:ref(value),ownProperties:true,generatePreview:false});
  if(!Array.isArray(result.result)||result.result.length>512)throw Error('descriptor-unavailable');
  const rows=new Map();for(const d of result.result){if(rows.has(d.name))throw Error('descriptor-unavailable');rows.set(d.name,d);}
  return {rows,internal:result.internalProperties??[]};
 };
 const get=(properties,key)=>{const d=properties.rows.get(key);if(!d||d.get||d.set||!d.value)throw Error('descriptor-unavailable');return d.value;};
 const optional=(properties,key)=>properties.rows.has(key)?get(properties,key):{type:'undefined'};
 const equal=async(a,b)=>{
  const response=await send('Runtime.callFunctionOn',{objectId:ref(a),functionDeclaration:EQUAL,
   arguments:[{objectId:ref(b)}],returnByValue:true,silent:true});return response.result?.value===true;
 };
 const pinned=async(value,kind)=>{
  const response=await send('Runtime.callFunctionOn',{objectId:ref(value),functionDeclaration:TOSTRING,returnByValue:true,silent:true});
  if(typeof response.result?.value!=='string'||response.result.value.length>65536
    ||crypto.createHash('sha256').update(response.result.value).digest('hex')!==pins[kind])throw Error('source-mismatch');
 };
 const scope=async(fn,label)=>{
  const props=await properties(fn),list=props.internal.find(p=>p.name==='[[Scopes]]')?.value;
  if(!list)throw Error('scope-unavailable');
  const entries=await properties(list),matches=[];
  for(const [key,d] of entries.rows){if(!/^(?:0|[1-9][0-9]*)$/.test(key))continue;
   if(d.get||!d.value)throw Error('descriptor-unavailable');
   if(d.value.description==='Closure ('+label+')')matches.push(d.value);
  }
  if(matches.length!==1)throw Error('scope-unavailable');return properties(matches[0]);
 };
 const roots=async(value)=>{
  const p=await properties(value);
  if(!primitive(get(p,'length'),1)||!primitive(get(p,'0'),cwd))throw Error('root-mismatch');
 };
 const nullish=(p,key,reason)=>{if(!nil(optional(p,key)))throw Error(reason);};
 const localProject=async(value)=>{const p=await properties(value);if(!primitive(get(p,'type'),'local')||!primitive(get(p,'projectId'),projectId))throw Error('project-mismatch');};
 const locate=async()=>{
  const result=await send('Runtime.callFunctionOn',{objectId:ref(editor),functionDeclaration:LOCATE,returnByValue:false,silent:true});
  return properties(result.result);
 };
 const sample=async(requireSelection)=>{
  const found=await locate();const component=get(found,'component'),componentType=get(found,'componentType'),
   componentProps=await properties(get(found,'componentProps')),utilityProps=await properties(get(found,'utilityProps')),
   context=get(found,'context'),submit=get(found,'submit');
  await pinned(componentType,'component');await pinned(context,'context');await pinned(submit,'submit');
  if(!primitive(get(utilityProps,'composerMode'),'local'))throw Error('mode-mismatch');
  nullish(utilityProps,'existingWorkspace','existing-workspace');nullish(componentProps,'homeRunLocationRemoteProject','remote-override');
  await localProject(get(componentProps,'selectedProject'));
  const placement=await properties(get(componentProps,'surfacePlacement'));
  if(!primitive(get(placement,'kind'),'home')||(!nil(optional(placement,'quickSend'))&&!primitive(optional(placement,'quickSend'),false)))throw Error('controller-mismatch');
  const target=await properties(get(utilityProps,'localRemoteExecutionTarget'));
  if(!primitive(get(target,'hostId'),'local')||!primitive(get(target,'cwd'),cwd))throw Error('host-or-cwd-mismatch');
  const closure=await scope(context,'mct'),settings=await properties(get(closure,'ic'));
  if(!primitive(get(settings,'localProjectId'),projectId))throw Error('project-mismatch');
  await roots(get(settings,'workspaceRoots'));
  for(const key of ['remoteProjectId','existingWorkspaceRoot','cloudThreadPrototype','aeonStartTarget','aeonDraftName','serverInitialization'])nullish(settings,key,'context-override');
  for(const key of ['ai','ui'])if(!primitive(get(closure,key),'local'))throw Error('host-or-cwd-mismatch');
  if(!primitive(get(closure,'dt'),cwd))throw Error('host-or-cwd-mismatch');
  const options=await properties(get(closure,'Ro'));
  if(!primitive(get(options,'hostId'),'local'))throw Error('host-or-cwd-mismatch');
  const optionRoots=optional(options,'workspaceRoots');if(!nil(optionRoots))await roots(optionRoots);
  const newScope=get(closure,'xe'),scopeProperties=await properties(newScope),getter=scopeProperties.rows.get('value')?.get;
  if(!getter)throw Error('scope-unavailable');await pinned(getter,'scopeGetter');
  const getterClosure=await scope(getter,'Ak'),config=await properties(get(getterClosure,'o'));
  if(!primitive(get(config,'kind'),'new')||!primitive(get(config,'entrypoint'),'home'))throw Error('controller-mismatch');
  for(const key of ['conversationId','routeConversationId','taskId','aeonStartTarget'])nullish(config,key,'controller-mismatch');
  const handler=await scope(submit,'_3e');
  nullish(handler,'c','follow-up');nullish(handler,'k','follow-up');
  if(!primitive(get(handler,'D'),'local')||!primitive(get(handler,'O'),false))throw Error('host-or-cwd-mismatch');
  if(!await equal(get(handler,'e'),newScope))throw Error('controller-mismatch');
  const prepare=get(handler,'T');await pinned(prepare,'prepare');const preparation=await scope(prepare,'_ct');
  nullish(preparation,'B','prepare-override');
  // _Z provides null whenever useAsbDirect!==true; the held handler requires O===false.
  if(get(preparation,'Zt').subtype!=='null')throw Error('prepare-override');
  const query=optional(preparation,'Qt');if(!nil(query)&&primitive(optional(await properties(query),'isError'),true))throw Error('prepare-override');
  const reservation=get(componentProps,'localThreadPrewarmReservation');
  if(!await equal(get(handler,'E'),reservation))throw Error('reservation-mismatch');
  const reserve=get(await properties(reservation),'reserve');await pinned(reserve,'reserve');
  const reserved=await scope(reserve,'h3e'),cached=get(reserved,'t');let selected=false;
  if(!nil(cached)){
   const p=await properties(cached);
   if(!primitive(get(p,'hostId'),'local')||!primitive(get(p,'projectId'),projectId))throw Error('reservation-mismatch');
   await roots(get(p,'workspaceRoots'));const promise=get(p,'selection');
   if(promise.subtype!=='promise')throw Error('reservation-unavailable');
   const internals=(await properties(promise)).internal;
   if(!primitive(internals.find(p=>p.name==='[[PromiseState]]')?.value,'fulfilled'))throw Error('reservation-pending');
   const selection=await properties(internals.find(p=>p.name==='[[PromiseResult]]')?.value);
   if(!primitive(get(selection,'cwd'),cwd))throw Error('root-mismatch');await roots(get(selection,'workspaceRoots'));
   const assignment=await properties(get(selection,'projectAssignment'));
   if(!primitive(get(assignment,'projectKind'),'local')||!primitive(get(assignment,'projectId'),projectId))throw Error('project-mismatch');
   nullish(selection,'projectlessOutputDirectory','reservation-mismatch');selected=true;
  } else if(requireSelection)throw Error('reservation-unavailable');
  const controller=get(closure,'hn');const view=await properties(get(await properties(controller),'view'));
  if(!await equal(get(view,'dom'),editor))throw Error('controller-mismatch');
  if(retained&&(!await equal(retained.scope,newScope)||!await equal(retained.controller,controller)||!await equal(retained.reservation,reservation)))throw Error('controller-mismatch');
  const after=await locate();
  for(const key of ['component','componentType','componentProps','utilityFiber','utilityProps','context','submit'])
   if(!await equal(get(found,key),get(after,key)))throw Error('render-changed');
  return {scope:newScope,controller,reservation,cached,component,selected};
 };
 return {async observe({requireSelection=false}={}){
  try {const first=await sample(requireSelection),second=await sample(requireSelection);
   if(!await equal(first.scope,second.scope)||!await equal(first.controller,second.controller)
    ||!await equal(first.reservation,second.reservation)||first.selected!==second.selected
    ||(!nil(first.cached)||!nil(second.cached))&&(!first.cached.objectId||!second.cached.objectId||!await equal(first.cached,second.cached)))throw Error('render-changed');
   retained??=first;
   return {verified:true,reason:'verified',sourcePinned:true,newHomeController:true,localContext:true,
    retainedProject:true,retainedRoot:true,prewarmResolvedSelection:first.selected,noPriorReservation:!first.selected,inputAuthorized:false};
  } catch(error){const reasons=new Set(['deadline-or-owner','runtime-unavailable','descriptor-unavailable','source-mismatch','scope-unavailable',
    'root-mismatch','project-mismatch','mode-mismatch','existing-workspace','remote-override','controller-mismatch','host-or-cwd-mismatch',
    'context-override','follow-up','prepare-override','reservation-mismatch','reservation-unavailable','reservation-pending','render-changed']);
   return failure(reasons.has(error.message)?error.message:'runtime-unavailable');
  }
 }};
}
module.exports={create,LOCATE};
