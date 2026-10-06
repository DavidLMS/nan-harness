// Passive feasibility only. Geometry, URLs and CDP identities never enter facts.
const reasons=new Set(['measured','ax-limit-or-deadline','ax-query','ax-visibility-unavailable',
  'ax-webarea-geometry','ax-webarea-ambiguous','ax-webarea-missing','ax-webarea-changed',
  'native-focus-unavailable','viewport-dimensions-mismatch','native-point-not-clear',
  'point-occluded','stack-unavailable','metadata-invalid','held-window-missing','held-window-changed',
  'native-hit-window-unproved','renderer-webarea-correlation-unproved','mapping-observed',
  'held-identity-or-deadline']);
const stackCauses=new Set(['point-occluded','stack-unavailable','metadata-invalid','held-window-missing','held-window-changed']);
function parseNative(value) {
  if(typeof value!=='string'||value.length>1024||!value.endsWith('\n'))throw Error('observation wire rejected');
  const fields=value.slice(0,-1).split(' ');
  if(fields[0]!=='point-observation'||!reasons.has(fields[1])||![11,15].includes(fields.length))throw Error('observation wire rejected');
  const counts=fields.slice(2,4).map(Number),bits=fields.slice(4,11);
  if(!fields.slice(2,4).every(v=>/^[0-9]+$/.test(v))||!counts.every(v=>Number.isSafeInteger(v)&&v>=0&&v<=2)
    ||!bits.every(v=>v==='0'||v==='1'))throw Error('observation wire rejected');
  const [stable,focused,dimensions,pointClear,hitWindow,heldStable,urlMatched]=bits.map(v=>v==='1');
  if(stable!==(fields.length===15)||stable&&(counts[0]!==1||counts[1]!==1)
    ||!stable&&(focused||dimensions||pointClear||hitWindow||urlMatched)
    ||stackCauses.has(fields[1])&&!(stable&&focused&&dimensions&&!pointClear&&heldStable)
    ||fields[1]==='mapping-observed'&&!(stable&&focused&&dimensions&&pointClear&&hitWindow&&heldStable&&urlMatched))
    throw Error('observation wire rejected');
  let bounds=null;
  if(stable) {
    const tokens=fields.slice(11);
    if(!tokens.every(v=>/^-?\d+(?:\.\d+)?(?:e[+-]?\d+)?$/.test(v)))throw Error('observation wire rejected');
    bounds=tokens.map(Number);
    if(!bounds.every(Number.isFinite)||bounds[2]<=0||bounds[3]<=0)throw Error('observation wire rejected');
  }
  return {facts:{reason:fields[1],firstWebAreaCount:counts[0],secondWebAreaCount:counts[1],
    webAreaStable:stable,nativeFocused:focused,dimensionsMatched:dimensions,nativePointClear:pointClear,
    nativeHitWindowMatched:hitWindow,heldIdentityStable:heldStable,webAreaUrlMatched:urlMatched,
    mappingObserved:fields[1]==='mapping-observed',inputAuthorized:false},bounds};
}
function same(a,b) {
  return ['url','target','frame','loader','frameUrl','fragment'].every(key=>a[key]===b[key]);
}
async function observe({session,native,held,deadline,owner,now=Date.now}) {
  const failed=reason=>({reason,mappingObserved:false,inputAuthorized:false});
  const timely=()=>Number.isSafeInteger(deadline)&&now()<deadline&&owner()===true;
  let previous=null;
  const bounded=async(method,params)=>{
    if(!timely())throw Error('expired');
    let timer;
    try {
      const value=await Promise.race([session.send(method,params),new Promise((_,reject)=>{
        timer=setTimeout(()=>reject(Error('expired')),Math.max(1,deadline-now()));
      })]);
      if(!timely())throw Error('expired');return value;
    } finally {clearTimeout(timer);}
  };
  const sample=async()=>{
    const target=(await bounded('Target.getTargetInfo',{targetId:held.target})).targetInfo;
    const tree=(await bounded('Page.getFrameTree',{})).frameTree;
    const frame=tree?.frame;
    const identity={url:target?.url,target:target?.targetId,frame:frame?.id,loader:frame?.loaderId,
      frameUrl:frame?.url,fragment:frame?.urlFragment??''};
    if(!same(held,identity))return {reason:'cdp-identity-changed'};
    // Child frames make a unique top-level native web area insufficient.
    if(tree.childFrames?.length)return {reason:'cdp-child-frame-present'};
    const metrics=await bounded('Page.getLayoutMetrics',{});
    const layout=metrics.cssLayoutViewport,visual=metrics.cssVisualViewport;
    if(!layout||!visual)return {reason:'css-viewport-unavailable'};
    const values=[layout.clientWidth,layout.clientHeight,layout.pageX,layout.pageY,
      visual.clientWidth,visual.clientHeight,visual.pageX,visual.pageY,visual.offsetX,visual.offsetY,visual.scale];
    if(!values.every(Number.isFinite)||layout.clientWidth<1||layout.clientHeight<1
      ||layout.clientWidth>16384||layout.clientHeight>16384)return {reason:'css-viewport-invalid'};
    if(layout.pageX!==0||layout.pageY!==0||visual.pageX!==0||visual.pageY!==0
      ||visual.offsetX!==0||visual.offsetY!==0||visual.scale!==1
      ||layout.clientWidth!==visual.clientWidth||layout.clientHeight!==visual.clientHeight)
      return {reason:'css-viewport-transform-unproved'};
    if(previous&&JSON.stringify(previous)!==JSON.stringify(values))return {reason:'css-viewport-changed'};
    previous=values;return {identity,width:layout.clientWidth,height:layout.clientHeight};
  };
  try {
    if(!held||!['url','target','frame','loader','frameUrl','fragment'].every(k=>typeof held[k]==='string')
      ||held.frameUrl!==held.url||held.fragment!=='')return failed('cdp-held-identity-invalid');
    const first=await sample();if(first.reason)return failed(first.reason);
    // Center is a diagnostic candidate only, never a source action target.
    const css=[first.width,first.height,first.width/2,first.height/2];
    if(!timely())return failed('deadline-or-owner');
    const measurement=parseNative(native.pointObserve(css,held.frameUrl));
    if(!timely())return failed('deadline-or-owner');
    const after=await sample();if(after.reason)return failed(after.reason);
    if(!same(first.identity,after.identity)||!timely())return failed('cdp-identity-changed');
    return measurement.facts;
  } catch {return failed(now()>=deadline?'deadline':'observation-unavailable');}
}
module.exports={parseNative,observe};
