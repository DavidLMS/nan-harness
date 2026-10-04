// Private native binding transport. Nothing returned here enters public facts.
const fs=require('node:fs');
const path=require('node:path');
const child=require('node:child_process');
function binding(output) {
  if(typeof output!=='string'||output.length>4096||!output.endsWith('\n'))throw Error('native binding rejected');
  const parts=output.trimEnd().split(' ');
  if(parts.length!==9||parts.some(value=>!/^(?:-?\d+(?:\.\d+)?(?:e[+-]?\d+)?)$/.test(value)))throw Error('native binding rejected');
  const values=parts.slice(0,8).map(Number);
  if(!values.every(Number.isFinite)||!Number.isSafeInteger(values[0])||values[0]<1
    ||!Number.isSafeInteger(values[1])||values[1]<2||values[4]<300||values[5]<200
    ||!Number.isSafeInteger(values[6])||values[6]<1||!Number.isSafeInteger(values[7])||values[7]<0
    ||!/^[0-9]+$/.test(parts[8])||BigInt(parts[8])<1n)throw Error('native binding rejected');
  return {tokens:parts.slice(0,8).join(' '),clock:BigInt(parts[8])};
}
function controller(config,owner,launcher,deadline,run=child.execFileSync,now=Date.now) {
  if(!config||Object.keys(config).sort().join(',')!=='cutoffNanos,executable,helper'
    ||!Number.isSafeInteger(owner)||owner<2||!Number.isSafeInteger(launcher)||launcher<2
    ||!Number.isSafeInteger(deadline)||typeof config.cutoffNanos!=='string'
    ||!/^[0-9]+$/.test(config.cutoffNanos)||BigInt(config.cutoffNanos)<1n)throw Error('native activation rejected');
  for(const file of [config.helper,config.executable]) {
    if(typeof file!=='string'||!path.isAbsolute(file)||fs.realpathSync(file)!==file
      ||!fs.lstatSync(file).isFile()||fs.lstatSync(file).isSymbolicLink())throw Error('native activation rejected');
  }
  let held=null,cutoff=BigInt(config.cutoffNanos),attempted=false,nativeBoundary=null,nativeInventoryFailure=null;
  const boundaries=new Set(['request','cg-inventory-before','ax-main-before','cg-inventory-after','ax-main-after','identity','trust']);
  const execute=phase=>{
    const remaining=deadline-now();
    if(remaining<=0)throw Error('native activation expired');
    const input=[phase,process.pid,owner,launcher,cutoff.toString(),Buffer.from(config.executable).toString('hex'),
      ...(phase==='prepare'?[]:[held.tokens])].join(' ')+'\n';
    let output;
    try {
      output=run(config.helper,['--codex-activate-main'],{input,encoding:'utf8',timeout:remaining,
        maxBuffer:4096,stdio:['pipe','pipe','ignore']});
    } catch(error) {
      if(phase==='prepare'&&typeof error?.stdout==='string'&&error.stdout.length<=128) {
        const match=/^activation-rejected ([a-z-]+)(?: ([a-z-]+) ([0-9]+) ([0-9]+) ([0-9]+))?\n$/.exec(error.stdout);
        if(match&&boundaries.has(match[1])) {
          if(!match[2])nativeBoundary=match[1];
          else {
            const counts=match.slice(3).map(Number);
            if(['cg-inventory-before','cg-inventory-after'].includes(match[1])
              &&['inventory-unavailable','limit','metadata','geometry','process-identity','identity','candidates-missing','candidates-ambiguous','other-owned-normal','overlapping-ahead','off-display','deadline'].includes(match[2])&&counts.every(v=>Number.isSafeInteger(v)&&v>=0&&v<=1024)
              &&counts.reduce((a,b)=>a+b,0)<=1024) {
              nativeBoundary=match[1];
              nativeInventoryFailure={reason:match[2],candidateCount:counts[0],
                executableRejectedCount:counts[1],ancestryRejectedCount:counts[2]};
            }
          }
        }
      }
      throw error;
    }
    if(now()>=deadline)throw Error('native activation expired');
    return output;
  };
  return {
    failure:()=>nativeBoundary,
    inventoryFailure:()=>nativeInventoryFailure,
    prepare() {
      if(held||attempted)throw Error('native activation consumed');
      held=binding(execute('prepare'));
      // The native anchor precedes receipt. This conversion shortens the caller
      // wall deadline; it cannot add IPC/startup time to an action's cutoff.
      const clipped=held.clock+BigInt(Math.max(0,deadline-now()-2))*1000000n;
      if(clipped<cutoff)cutoff=clipped;
    },
    activate() {
      if(!held||attempted)throw Error('native activation consumed');
      attempted=true;
      if(execute('activate')!=='activated\n')throw Error('native activation uncertain');
    },
    verify() {
      if(!held||!attempted)return false;
      return execute('verify')==='verified\n';
    },
  };
}
module.exports={controller,binding};
