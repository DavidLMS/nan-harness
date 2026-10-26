'use strict';
// Frozen MessageRenderBoundary can replace a turn's children while preserving
// its keyed group. ChatRoutesSurface replaces ChatView on contributed/page
// routes, so the exact surviving native surface/root AND private session URL
// are required together; route syntax alone grants nothing. Native Retry uses
// createSessionRpcDispatcher -> requestForSessionProfile for its recorded
// session owner, independently of the active profile's presentation state.
// This helper neither selects a profile nor invokes any application RPC.
const SOURCE='0.17.6:f97608f178d1ffeca59860195ab7da295f7c8e5f';
function policy(platform,env,qualify,action) {
  return platform==='win32'&&qualify===true&&action==='retry'
    &&env.GITHUB_ACTIONS==='true'&&env.RUNNER_ENVIRONMENT==='github-hosted'
    &&env.RUNNER_OS==='Windows'&&env.FEASIBILITY_HERMES_RETRY_SUBTREE_SOURCE===SOURCE;
}
// Standalone browser callback. The original ready tuple remains authoritative
// when this narrow frozen-source policy cannot establish its extra anchors.
function captureInitialScope(alert,input) {
  if(alert.ownerDocument!==input.originalDocument||input.originalDocument!==document)return {state:'invalid'};
  const assistant=alert.closest('[data-role="assistant"][data-slot="aui_assistant-message-root"]');
  const pair=assistant?.closest('[data-slot="aui_turn-pair"]');
  const group=pair?.closest('[data-slot="aui_message-group"]');
  if(!alert.isConnected||!assistant?.isConnected||!pair?.isConnected||!group?.isConnected)return {state:'pending'};
  const users=pair.querySelectorAll('[data-role="user"]');
  if(users.length===0)return {state:'pending'};
  if(users.length!==1||alert.closest('[data-slot="aui_turn-pair"]')!==pair
    ||users[0].innerText.trim()!==input.prompt||users[0].closest('[data-slot="aui_turn-pair"]')!==pair
    ||input.button.ownerDocument!==document)return {state:'invalid'};
  if(!users[0].isConnected||!input.button.isConnected)return {state:'pending'};
  if(input.button.closest('[role="alert"]')!==alert
    ||input.button.closest('[data-slot="aui_turn-pair"]')!==pair)return {state:'invalid'};
  let anchors=null;
  if(input.subtreePolicy)try {
    const url=new URL(location.href),path=url.hash.slice(1).split(/[?#]/,1)[0];
    const reserved=['/','/new','/settings','/command-center','/session-import','/capabilities','/messaging',
      '/webhooks','/artifacts','/cron','/profiles','/agents','/starmap'];
    const id=path.startsWith('/')?decodeURIComponent(path.slice(1)):'';
    const surface=group.closest('[data-chat-surface]'),parent=group.parentElement;
    const viewport=group.closest('[data-slot="aui_thread-viewport"]');
    if(url.protocol==='file:'&&url.search===''&&id&&!id.includes('/')&&!reserved.includes(path)
      &&surface?.isConnected&&parent?.isConnected&&viewport?.isConnected
      &&surface.getAttribute('data-session-anchor')==='workspace'
      &&surface.getAttribute('data-composer-target')==='main'
      &&viewport.closest('[data-chat-surface]')===surface&&viewport.contains(parent))
      anchors={surface,parent,viewport,group,href:location.href,hash:location.hash};
  } catch { /* Missing public session/root proof leaves legacy admission unchanged. */ }
  return {state:'ready',alert,pair,group,user:users[0],document,button:input.button,
    anchors,prompt:input.prompt};
}
// Inspect only the retained root. No application store, callback or RPC is read.
function inspectSubtree(held) {
  const a=held.anchors;
  if(!a)return {state:'unavailable'};
  if(held.document!==document||location.href!==a.href||location.hash!==a.hash
    ||![a.group,a.parent,a.viewport,a.surface].every(e=>e.isConnected&&e.ownerDocument===document)
    ||a.group.parentElement!==a.parent||a.group.closest('[data-slot="aui_thread-viewport"]')!==a.viewport
    ||a.group.closest('[data-chat-surface]')!==a.surface||!a.viewport.contains(a.parent)
    ||a.viewport.closest('[data-chat-surface]')!==a.surface
    ||a.surface.getAttribute('data-session-anchor')!=='workspace'
    ||a.surface.getAttribute('data-composer-target')!=='main')return {state:'root-lost'};
  const connected=[held.pair,held.user,held.alert,held.button].map(e=>e.isConnected);
  if(connected.every(Boolean))return {state:held.pair.closest('[data-slot="aui_message-group"]')===a.group
    ?'unchanged':'root-lost'};
  if(connected.some(Boolean))return {state:connected[0]&&connected[1]&&connected[2]&&!connected[3]
    ?'child-only':'partial'};
  const pairs=a.group.querySelectorAll('[data-slot="aui_turn-pair"]');
  if(pairs.length===0)return {state:'pending'};
  if(pairs.length!==1)return {state:'invalid'};
  const pair=pairs[0],users=pair.querySelectorAll('[data-role="user"]');
  const alerts=a.group.querySelectorAll('[data-role="assistant"][data-slot="aui_assistant-message-root"] [role="alert"]');
  if(users.length>1||alerts.length>1)return {state:'invalid'};
  if(users.length===0||alerts.length===0)return {state:'pending'};
  const user=users[0],alert=alerts[0];
  const buttons=[...alert.querySelectorAll('button')].filter(e=>
    (e.getAttribute('aria-label')??e.innerText).trim()==='Retry');
  if(buttons.length===0)return {state:'pending'};
  if(buttons.length!==1)return {state:'invalid'};
  const button=buttons[0],assistant=alert.closest('[data-role="assistant"][data-slot="aui_assistant-message-root"]');
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);
    return e.isConnected&&e.ownerDocument===document&&r.width>0&&r.height>0
      &&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  if(![pair,user,alert,button].every(visible)||pair.closest('[data-slot="aui_message-group"]')!==a.group
    ||user.innerText.trim()!==held.prompt||user.closest('[data-slot="aui_turn-pair"]')!==pair
    ||assistant?.closest('[data-slot="aui_turn-pair"]')!==pair
    ||alert.closest('[data-slot="aui_turn-pair"]')!==pair||button.closest('[role="alert"]')!==alert
    ||button.closest('[data-slot="aui_turn-pair"]')!==pair||button.type!=='button'
    ||button.disabled||button.getAttribute('aria-disabled')==='true')return {state:'invalid'};
  return {state:'ready',document,pair,group:a.group,user,alert,button,anchors:a,prompt:held.prompt};
}
function sameTuple(next,previous) {
  return ['document','pair','group','user','alert','button'].every(key=>next[key]===previous[key]);
}
function create({page,scope,owned,deadline,claim,publish,now=Date.now,
  pause=ms=>new Promise(resolve=>setTimeout(resolve,ms))}) {
  let current=scope,settled=false,attempted=false,disabled=false,failed=false;
  const report=status=>{try {publish(status);}catch {}};
  const reject=()=>{failed=true;report('rejected');return {state:'rejected'};};
  const bounded=async work=>{let timer;
    try{return await Promise.race([work,new Promise((_,reject)=>{
      timer=setTimeout(()=>reject(Error('deadline')),Math.max(1,deadline-now()));
    })]);}finally{clearTimeout(timer);}
  };
  const inspect=async(bracket=true)=>{
    if(now()>=deadline||bracket&&!owned()||now()>=deadline)throw Error('ownership');
    let accepted=false;
    const pending=page.evaluateHandle(inspectSubtree,current).then(async handle=>{
      if(failed||now()>=deadline){try{await handle.dispose();}catch {}throw Error('deadline');}
      return handle;
    });
    const handle=await bounded(pending);
    try {
      const state=await bounded(handle.evaluate(value=>value.state));
      if(now()>=deadline||bracket&&!owned()||now()>=deadline)throw Error('ownership');
      accepted=true;return {state,handle};
    }finally{if(!accepted)try{await handle.dispose();}catch {}}
  };
  return {async ensure() {
    if(failed)return {state:'rejected'};
    if(disabled)return {state:'unchanged'};
    let sample,previous;
    try {
      // The caller's existing reproof has just checked native ownership. Avoid
      // another process proof on the normal unchanged path; a candidate's
      // first read is bracketed by that check and the fresh check below.
      sample=await inspect(false);
      if(sample.state==='unavailable'){disabled=true;return {state:'unchanged'};}
      if(sample.state==='unchanged')return {state:'unchanged'};
      if(settled||sample.state==='root-lost')return reject();
      if(sample.state==='child-only')return {state:'unchanged'};
      if(sample.state==='partial')return reject();
      if(now()>=deadline||!owned()||now()>=deadline)throw Error('ownership');
      if(!claim())return reject();
      attempted=true;
      for(;;) {
        if(!['pending','ready'].includes(sample.state))throw Error('replacement');
        if(sample.state==='ready') {
          if(previous) {
            if(!await bounded(sample.handle.evaluate(sameTuple,previous)))throw Error('second replacement');
            const button=await bounded(sample.handle.getProperty('button'));
            const element=button.asElement();
            if(!element)throw Error('button');
            current=sample.handle;sample=null;settled=true;report('completed');
            return {state:'completed',scope:current,button:element};
          }
          previous=sample.handle;sample=null;
        } else if(previous)throw Error('second replacement');
        if(sample){await sample.handle.dispose();sample=null;}
        await pause(Math.min(100,Math.max(0,deadline-now())));
        sample=await inspect();
      }
    }catch {failed=true;if(attempted||settled)report('rejected');return {state:'rejected'};}
    finally {
      for(const handle of [sample?.handle,previous])if(handle)try{await handle.dispose();}catch {}
    }
  }};
}
module.exports={policy,captureInitialScope,inspectSubtree,create};
