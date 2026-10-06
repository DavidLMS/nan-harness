const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const {parseMove}=require('./codex-owned-move-wire.cjs');
const old='42 100 10 20 600 400 500 0',next='42 100 30 40 600 400 500 0';
const success='owned-move moved-point-observed 1 1 0 1 1 1 1 1 1 1 1 '+next+'\n';
assert.equal(parseMove(success,old).tokens,next);assert.equal(parseMove(success,old).facts.inputAuthorized,false);
for(const raw of [success.replace('100 30','101 30'),success.replace('600 400','601 400'),success.replace('moved-point-observed','unknown'),success.replace('1 1 0 1','10 1 0 1'),success.trim(),success.replace(next,old),success.replace(' 1 1 '+next,' 1 0 '+next)])assert.throws(()=>parseMove(raw,old));
for(const index of [0,1,4,5,6,7]){const altered=next.split(' ');altered[index]=String(Number(altered[index])+1);assert.throws(()=>parseMove(success.replace(next,altered.join(' ')),old));}
assert.equal(parseMove('owned-move full-workarea-bounds-blocker 0 1 1 0 0 0 0 0 0 0 0\n',old).facts.fullWorkareaBoundsBlocker,true);
assert.equal(parseMove('owned-move no-clear-candidate 9 1 0 0 0 0 0 0 0 0 0\n',old).facts.candidateFound,false);
assert.equal(parseMove('owned-move write-uncertain 1 1 0 1 1 0 0 0 0 0 0\n',old).facts.moveAttempted,true);
const env={GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',RUNNER_OS:'macOS',NANH_CODEX_OWNED_MOVE:'source-point',NANH_CODEX_PUBLIC_ONBOARDING:'engineering',NANH_CODEX_PROJECT_POLICY:'open-project',NANH_CODEX_PROJECT_ARTIFACT_SHA256:'f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7'};
function controller(scenario){const context={module:{exports:{}},process:{pid:30,platform:'darwin',env},Buffer,require(key){if(key==='node:fs')return {realpathSync:x=>x,lstatSync:()=>({isFile:()=>true,isSymbolicLink:()=>false})};if(key==='node:path')return path;if(key==='node:child_process')return {};if(key==='./codex-owned-move-wire.cjs')return {parseMove};throw Error(key)}};
 vm.runInNewContext(fs.readFileSync(require.resolve('./codex-native-activation.cjs'),'utf8'),context);const calls=[];
 const c=context.module.exports.controller({helper:'/helper',executable:'/Codex',cutoffNanos:'9000000000'},20,25,2000,(_h,_a,o)=>{
  const tokens=o.input.trim().split(' ');calls.push(tokens);const phase=tokens[0];
  if(phase==='prepare')return old+' 1000000000\n';if(phase==='activate')return 'activated\n';
  if(phase==='move-owned'){if(scenario==='uncertain')throw Error('private failure');return scenario==='full'?'owned-move full-workarea-bounds-blocker 0 1 1 0 0 0 0 0 0 0 0\n':success;}
  if(phase==='point-observe')return 'private-observation';if(phase==='verify')return 'pending-external-stack\n';throw Error(phase);
 },()=>1000);c.prepare();c.activate();return {c,calls};}
for(const scenario of ['success','full','uncertain']){const {c,calls}=controller(scenario);if(scenario==='uncertain')assert.throws(()=>c.moveOwned([600,400,100,100],'app://source','onboarding-engineering'));else c.moveOwned([600,400,100,100],'app://source','onboarding-engineering');
 assert.throws(()=>c.moveOwned([600,400,100,100],'app://source','onboarding-engineering'));assert.equal(calls.filter(x=>x[0]==='move-owned').length,1);
 const cutoff=calls[1][4];assert.equal(calls[2][4],cutoff);assert.equal(c.verify(),false);assert.equal(calls[3][4],cutoff);
 if(scenario==='success')assert.deepEqual(calls[3].slice(6,14),next.split(' '));else assert.deepEqual(calls[3].slice(6,14),old.split(' '));
}
console.log('Wire identity/schema, full/no candidate, one-command uncertain no-retry and cutoff fixtures passed');
