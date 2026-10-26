'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const helperSource=fs.readFileSync(`${__dirname}/hermes-retry-subtree.cjs`,'utf8');
const helper=require('./hermes-retry-subtree.cjs');
const env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'Windows',
  FEASIBILITY_HERMES_RETRY_SUBTREE_SOURCE:'0.17.6:f97608f178d1ffeca59860195ab7da295f7c8e5f'};
assert.equal(helper.policy('win32',env,true,'retry'),true);
for(const key of Object.keys(env))assert.equal(helper.policy('win32',{...env,[key]:'wrong'},true,'retry'),false);
for(const values of [['linux',env,true,'retry'],['win32',env,false,'retry'],['win32',env,true,'submit']])
  assert.equal(helper.policy(...values),false);
function fixture(route={}) {
  let clock=0,owned=true,tickets=0,proofs=0,queries=0,disposed=0,attempts=0,afterPause=()=>{};
  const doc={},location={href:'file:///synthetic/index.html#/PRIVATE-session',hash:'#/PRIVATE-session',...route};
  const node=(slot,parent=null,attrs={})=>({slot,parentElement:parent,isConnected:true,ownerDocument:doc,
    innerText:'',type:'button',disabled:false,
    getAttribute(key){return key==='data-slot'?this.slot:attrs[key]??null;},
    getBoundingClientRect(){return {width:100,height:30};},
    contains(other){for(let n=other;n;n=n.parentElement)if(n===this)return true;return false;},
    closest(selector){for(let n=this;n;n=n.parentElement){
      if(selector===`[data-slot="${n.slot}"]`||selector==='[data-chat-surface]'&&n.slot==='surface'
        ||selector==='[role="alert"]'&&n.slot==='alert'
        ||selector==='[data-role="assistant"][data-slot="aui_assistant-message-root"]'&&n.slot==='assistant')return n;
    }return null;},querySelectorAll(selector){
      const list=descendants(this);
      return list.filter(n=>selector==='[data-slot="aui_turn-pair"]'?n.slot==='aui_turn-pair'
        :selector==='[data-role="user"]'?n.slot==='user'
        :selector==='button'?n.slot==='button'
        :selector==='[data-role="assistant"][data-slot="aui_assistant-message-root"] [role="alert"]'?n.slot==='alert':false);
    }});
  let nodes=[];
  const descendants=root=>nodes.filter(n=>n!==root&&root.contains(n));
  const surface=node('surface',null,{'data-session-anchor':'workspace','data-composer-target':'main'});
  const viewport=node('aui_thread-viewport',surface),parent=node('parent',viewport),group=node('aui_message-group',parent);
  nodes=[surface,viewport,parent,group];
  function tuple() {
    const pair=node('aui_turn-pair',group),user=node('user',pair),assistant=node('assistant',pair);
    const alert=node('alert',assistant),button=node('button',alert);
    user.innerText='PRIVATE-prompt';button.innerText='Retry';nodes.push(pair,user,assistant,alert,button);
    return {pair,user,assistant,alert,button};
  }
  const original=tuple();
  const context=vm.createContext({module:{exports:{}},URL,document:doc,location,
    getComputedStyle:()=>({display:'block',visibility:'visible'}),setTimeout,clearTimeout});
  vm.runInContext(helperSource,context);const source=context.module.exports;
  const handle=value=>({value,async evaluate(callback,arg){return callback(value,arg?.value??arg);},
    async getProperty(key){return {asElement:()=>value[key]};},async dispose(){disposed++;}});
  const scope=handle(source.captureInitialScope(original.alert,{prompt:'PRIVATE-prompt',
    originalDocument:doc,button:original.button,subtreePolicy:true}));
  assert.equal(scope.value.state,'ready');
  const statuses=[];
  const page={async evaluateHandle(callback,arg){queries++;return handle(callback(arg.value));}};
  const capability=source.create({page,
    scope,owned:()=>{proofs++;return owned;},deadline:1000,claim:()=>{attempts++;if(tickets)return false;tickets++;return true;},
    publish:value=>statuses.push(value),now:()=>clock,pause:async ms=>{clock+=ms;afterPause();}});
  return {capability,scope,original,surface,viewport,parent,group,location,doc,statuses,nodes,node,tuple,page,
    detach(){for(const n of [original.pair,original.user,original.assistant,original.alert,original.button])n.isConnected=false;
      original.pair.parentElement=null;},
    replace(){this.detach();return tuple();},loseOwner(){owned=false;},expire(){clock=1000;},
    reserve(){tickets=1;},onPause(fn){afterPause=fn;},tickets:()=>tickets,attempts:()=>attempts,
    advance(value){clock=value;},
    proofs:()=>proofs,queries:()=>queries,disposed:()=>disposed};
}
(async()=>{
  const f=fixture(),replacement=f.replace();
  const result=await f.capability.ensure();
  assert.equal(result.state,'completed');assert.equal(result.button,replacement.button);
  assert.equal(f.tickets(),1);assert.equal(f.queries(),2);assert.equal(f.proofs(),3);
  assert.deepEqual(f.statuses,['completed']);assert.equal((await f.capability.ensure()).state,'unchanged');
  assert.ok(!JSON.stringify(f.statuses).includes('PRIVATE'));
  for(const hash of ['','#/','#/new','#/settings','#/one/two','#/%ZZ']) {
    const x=fixture({hash,href:'file:///synthetic/index.html'+hash});x.replace();
    assert.equal((await x.capability.ensure()).state,'unchanged','no source session '+hash);
    assert.equal(x.attempts(),0);assert.equal(x.scope.value.anchors,null);
  }
  for(const [name,alter] of [
    ['route',x=>{x.location.href+='/foreign';x.location.hash+='/foreign';}],
    ['document',x=>{x.scope.value.document={};}],['group',x=>{x.group.isConnected=false;}],
    ['parent',x=>{x.group.parentElement=x.node('foreign',x.viewport);}],
    ['viewport',x=>{x.viewport.isConnected=false;}],['surface',x=>{x.surface.isConnected=false;}],
    ['owner',x=>x.loseOwner()],['deadline',x=>x.expire()],['used ticket',x=>x.reserve()],
  ]) {
    const x=fixture();x.replace();alter(x);
    assert.equal((await x.capability.ensure()).state,'rejected',name);
  }
  for(const key of ['pair','user','alert','button']) {
    const x=fixture();x.replace();x.original[key].isConnected=true;
    assert.equal((await x.capability.ensure()).state,'rejected','partial old '+key);
    assert.equal(x.attempts(),0);
  }
  const childOnly=fixture();childOnly.original.button.isConnected=false;
  assert.equal((await childOnly.capability.ensure()).state,'unchanged','existing child-only path remains available');
  assert.equal(childOnly.attempts(),0);
  for(const [name,alter] of [
    ['duplicate pair',(x,r)=>x.tuple()],['foreign user',(x,r)=>{r.user.innerText='foreign';}],
    ['duplicate user',(x,r)=>{x.nodes.push(x.node('user',r.pair));}],
    ['duplicate alert',(x,r)=>{x.nodes.push(x.node('alert',r.assistant));}],
    ['duplicate Retry',(x,r)=>{const b=x.node('button',r.alert);b.innerText='Retry';x.nodes.push(b);}],
    ['disabled',(x,r)=>{r.button.disabled=true;}],['foreign Retry',(x,r)=>{r.button.parentElement=x.group;}],
    ['second churn',(x,r)=>x.onPause(()=>{r.pair.parentElement=null;x.tuple();})],
    ['loss between samples',(x,r)=>x.onPause(()=>x.loseOwner())],
  ]) {
    const x=fixture(),r=x.replace();alter(x,r);
    assert.equal((await x.capability.ensure()).state,'rejected',name);
    assert.equal(x.attempts(),1,name);assert.deepEqual(x.statuses,['rejected'],name);
  }
  const pending=fixture();pending.detach();let mounted=false;
  pending.onPause(()=>{if(!mounted){pending.tuple();mounted=true;}});
  assert.equal((await pending.capability.ensure()).state,'completed');
  assert.equal(pending.queries(),3,'empty boundary then two stable samples');
  const changed=fixture(),first=changed.replace();assert.equal((await changed.capability.ensure()).state,'completed');
  for(const n of [first.pair,first.user,first.alert,first.button])n.isConnected=false;
  first.pair.parentElement=null;changed.tuple();
  assert.equal((await changed.capability.ensure()).state,'rejected');assert.equal(changed.attempts(),1);
  const rejected=fixture(),wrong=rejected.replace();wrong.user.innerText='foreign';
  assert.equal((await rejected.capability.ensure()).state,'rejected');
  const reads=rejected.queries();
  for(const node of Object.values(rejected.original))node.isConnected=true;
  rejected.original.pair.parentElement=rejected.group;
  assert.equal((await rejected.capability.ensure()).state,'rejected','reconnected old tuple never reopens capability');
  assert.equal(rejected.queries(),reads);assert.equal(rejected.attempts(),1);
  const late=fixture();late.advance(999);let release,lateDisposed=false;
  late.page.evaluateHandle=()=>new Promise(resolve=>{release=resolve;});
  assert.equal((await late.capability.ensure()).state,'rejected');
  release({async dispose(){lateDisposed=true;}});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(lateDisposed,true,'late handle is disposed after rejection even before mocked deadline');
  assert.equal((await late.capability.ensure()).state,'rejected');
  console.log('Frozen Hermes private row subtree capability: synthetic cases passed');
})().catch(error=>{console.error(error.stack);process.exitCode=1;});
