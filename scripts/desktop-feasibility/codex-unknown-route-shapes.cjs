'use strict';
// Descriptive only: no URL value or shape can grant renderer authority.
const protocols=['app','about','data','devtools','http','https','file','other','invalid'];
const entries=['index','detachedWindow','other'];
const routes=['absent','multiple','avatarOverlay','detachedWindow','hotkeyWindow',
  'globalDictation','debug','spotlight','spacePrewarm','other'];
const queries=['none','initialRouteOnly','initialRouteWithSandbox','other'];
const routeNames=new Map([['/avatar-overlay','avatarOverlay'],['/detached-window','detachedWindow'],
  ['/hotkey-window','hotkeyWindow'],['/global-dictation','globalDictation'],
  ['/debug','debug'],['/spotlight','spotlight']]);
const invalid=()=>({protocolKind:'invalid',ownedAppHost:false,entryKind:'other',
  initialRouteKind:'absent',queryShape:'other',fragmentPresent:false});
function initialRouteKind(values) {
  if(values.length===0)return 'absent';
  if(values.length!==1)return 'multiple';
  const value=values[0],known=routeNames.get(value);
  if(known)return known;
  // The frozen constructor uses local-page-${randomUUID()}, encoded as one
  // outer initialRoute value. Classify its fixed syntax without retaining IDs.
  if(value.startsWith('/space/local-page-')) {
    const nested=new URL(value,'app://-');
    const parameters=[...nested.searchParams];
    if(nested.protocol==='app:'&&nested.host==='-'&&!value.includes('#')
      &&value.split('?')[0]===nested.pathname
      &&/^\/space\/local-page-[^/]+$/.test(nested.pathname)&&parameters.length===2
      &&nested.searchParams.getAll('window').length===1&&nested.searchParams.get('window')==='page'
      &&nested.searchParams.getAll('prewarm').length===1&&nested.searchParams.get('prewarm')==='1')
      return 'spacePrewarm';
  }
  return 'other';
}
function shape(raw) {
  let url;
  try {if(typeof raw!=='string')return invalid();url=new URL(raw);}catch{return invalid();}
  const protocol=url.protocol.slice(0,-1),parameters=[...url.searchParams];
  const singleRoute=url.searchParams.getAll('initialRoute').length===1;
  const queryShape=parameters.length===0?'none':singleRoute&&parameters.length===1?'initialRouteOnly'
    :singleRoute&&parameters.length===2&&url.searchParams.getAll('mcpAppSandboxLocalDevelopment').length===1
      &&url.searchParams.get('mcpAppSandboxLocalDevelopment')==='1'?'initialRouteWithSandbox':'other';
  return {protocolKind:protocol!=='invalid'&&protocols.includes(protocol)?protocol:'other',
    ownedAppHost:protocol==='app'&&url.hostname==='-'&&!url.port&&!url.username&&!url.password,
    entryKind:url.pathname==='/index.html'?'index':url.pathname==='/detached-window.html'?'detachedWindow':'other',
    initialRouteKind:initialRouteKind(url.searchParams.getAll('initialRoute')),queryShape,
    fragmentPresent:raw.includes('#')};
}
function validShape(value) {
  if(!value||Object.keys(value).sort().join(',')!==
    'entryKind,fragmentPresent,initialRouteKind,ownedAppHost,protocolKind,queryShape')return false;
  return protocols.includes(value.protocolKind)&&typeof value.ownedAppHost==='boolean'
    &&(!value.ownedAppHost||value.protocolKind==='app')&&entries.includes(value.entryKind)
    &&routes.includes(value.initialRouteKind)&&queries.includes(value.queryShape)
    &&typeof value.fragmentPresent==='boolean'
    &&(value.protocolKind!=='invalid'||!value.ownedAppHost&&value.entryKind==='other'
      &&value.initialRouteKind==='absent'&&value.queryShape==='other'&&!value.fragmentPresent);
}
function validPair(value,context) {
  if(!value||!context||Object.keys(value).sort().join(',')!=='initial,settle')return false;
  const initial=context.initialRoute==='unknown',settle=context.settleRoute==='unknown';
  return (initial||settle)&&(initial?validShape(value.initial):value.initial===null)
    &&(settle?validShape(value.settle):value.settle===null);
}
const copyPair=value=>({initial:value.initial?{...value.initial}:null,settle:value.settle?{...value.settle}:null});
module.exports={shape,validPair,copyPair};
