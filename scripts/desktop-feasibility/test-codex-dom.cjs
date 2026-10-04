const assert=require('node:assert/strict');
const vm=require('node:vm');
const {turnObservation,ordinaryClick,bindRecordedMain,validBinding,validRequest}=require('./codex-dom.cjs');
const prompt='Check this connection',marker='NAN_CHECK_SYNTHETIC_RESPONSE_NONCE';
function observed(options={}) {
  const visible=e=>Object.assign(e,{isConnected:true,getBoundingClientRect:()=>({width:100,height:20})});
  const turn=visible({contains:()=>true,querySelectorAll:()=>options.thoughtOnly?[]:headings});
  const bubble=visible({innerText:options.wrongUser?'different':prompt});
  const user=visible({querySelectorAll:()=>[bubble],closest:selector=>selector==='[data-turn-key]'&&!options.noTurn?turn:null});
  const units=Array.from({length:options.duplicateAssistant?2:1},()=>visible({
    querySelector:()=>options.userInsideAssistant?user:null,
    querySelectorAll:()=>[visible({innerText:options.nonmatching?'prefix '+marker:marker,closest:()=>null})]}));
  const headings=units.map(unit=>({closest:()=>unit}));
  const users=options.duplicateUser?[user,user]:[user];
  const callback=vm.runInNewContext(`(${turnObservation.toString()})`,{
    document:{querySelectorAll:()=>users},getComputedStyle:()=>({display:'block',visibility:'visible'})});
  return callback({prompt,marker});
}
assert.equal(observed().responseVerified,true);
for(const options of [{thoughtOnly:true},{duplicateUser:true},{duplicateAssistant:true},
  {userInsideAssistant:true},{nonmatching:true},{wrongUser:true},{noTurn:true}]) {
  assert.equal(observed(options).responseVerified,false);
}
assert(!JSON.stringify(observed()).includes(marker));
function semanticObserved(options={}) {
 const show=e=>Object.assign(e,{isConnected:true,getBoundingClientRect:()=>({width:100,height:20})});
 const conversation={contains:()=>true,querySelectorAll:()=>options.overflow?Array(4097).fill(userScope):[userScope,assistantScope]};
 const userScope={getAttribute:()=>options.malformed?'': 'PRIVATE_KEY',closest:()=>conversation,querySelectorAll:()=>[]};
 const assistantScope={getAttribute:()=>options.foreignKey?'OTHER':'PRIVATE_KEY',closest:()=>options.foreignConversation?{}:conversation,querySelectorAll:()=>headings};
 const unit={closest:selector=>selector==='[data-content-search-turn-key]'?assistantScope:conversation,querySelector:()=>options.userUnit?user:null,
  querySelectorAll:()=>[show({innerText:options.wrongNonce?'wrong':marker,closest:()=>null})]};
 const headings=Array.from({length:options.duplicate?2:1},(_,index)=>({closest:selector=>selector==='[data-content-search-turn-key]'?assistantScope:index===0?unit:{...unit}}));
 const user=show({querySelectorAll:()=>[show({innerText:prompt})],closest:selector=>selector==='[data-turn-key]'?{contains:()=>false,querySelectorAll:()=>[]}:selector==='[data-content-search-turn-key]'?userScope:conversation});
 const document={querySelectorAll:()=>options.duplicateUser?[user,user]:[user]};
 return vm.runInNewContext(`(${turnObservation})`,{document,getComputedStyle:()=>({display:'block',visibility:'visible'})})({prompt,marker});
}
assert.equal(semanticObserved().responseVerified,true,'shared semantic key across physical rows');
for(const key of ['foreignKey','foreignConversation','malformed','overflow','userUnit','wrongNonce','duplicate','duplicateUser'])assert.equal(semanticObserved({[key]:true}).responseVerified,false,key);
assert(!JSON.stringify(semanticObserved()).includes('PRIVATE_KEY'));

const request={connectionPath:'private',ownerPid:9,prompt,expectedMarker:marker,timeoutMs:45000,
  action:'submit',purpose:'response',mainBindingPath:'private'};
assert.equal(validRequest(request),true);
for(const change of [{timeoutMs:45001},{action:'unknown'},{prompt:'arbitrary'},
  {purpose:'failure'},{extra:'private'}])assert.equal(validRequest({...request,...change}),false);
(async()=>{
  const identity=target=>({url:target==='main'?'app://-/index.html':'app://-/index.html?initialRoute=%2Favatar-overlay',
    target,frame:'frame-'+target,loader:'loader-'+target,frameUrl:'app://-/index.html',fragment:''});
  const connection={launcherPid:11,port:4567};
  const binding={schemaVersion:1,main:identity('main'),auxiliary:identity('aux'),ownerPid:9,...connection};
  assert.equal(validBinding(binding,request,connection),true);
  assert.equal(validBinding({...binding,ownerPid:10},request,connection),false);
  assert.equal(validBinding({...binding,auxiliary:{...binding.auxiliary,url:'app://-/index.html?initialRoute=%2Fdebug'}},request,connection),false);
  let main={kind:'main',evaluate:async()=>true},aux={kind:'aux',evaluate:async()=>true};
  let pages=[main,aux],owned=true,alter=()=>{};
  const browser={contexts:()=>[{pages:()=>pages}]};
  const read=async page=>{const value=identity(page.kind);alter(value,page);return value;};
  const deadline=Date.now()+1000;
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,deadline,read),main);
  aux.evaluate=async()=>false;
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,deadline,read),null);
  aux.evaluate=async()=>true;
  alter=value=>{value.loader+='changed';};
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,deadline,read),null);
  alter=()=>{};pages=[main,aux,{}];
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,deadline,read),null);
  pages=[main,aux];owned=false;
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,deadline,read),null);
  owned=true;main.evaluate=async()=>false;
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,deadline,read),null);
  main.evaluate=async()=>true;
  assert.equal(await bindRecordedMain(browser,binding,()=>owned,Date.now(),read),null);
  let clicks=0,attempts=0,disposed=0,guards=0;
  const element={};
  const handle={evaluate:async()=>({rect:[0,0,10,10],points:[{x:5,y:5}]}),
    click:async options=>{assert.equal(options.force,undefined);clicks++;},dispose:async()=>{disposed++;}};
  const locator={count:async()=>1,isEnabled:async()=>true,elementHandle:async()=>handle,
    evaluate:async()=>true};
  const guard=async()=>{guards++;return true;};
  assert.equal(await ordinaryClick(locator,guard,Date.now()+1000,()=>{attempts++;}),true);
  assert.equal(clicks,1);assert.equal(attempts,1);assert.equal(disposed,1);
  guards=0;
  const losing=async()=>++guards<4;
  assert.equal(await ordinaryClick(locator,losing,Date.now()+1000,()=>{attempts++;}),false);
  assert.equal(clicks,1);assert.equal(attempts,1);assert.equal(disposed,2);
  const intercept={...locator,elementHandle:async()=>({...handle,evaluate:async()=>({blocked:'foreign-overlay'})})};
  assert.equal(await ordinaryClick(intercept,guard,Date.now()+1000,()=>{attempts++;}),false);
  assert.equal(clicks,1);
  console.log('PASS: Codex source-bound turn oracle, immutable binding and single ordinary action');
})().catch(error=>{console.error(error);process.exitCode=1;});
