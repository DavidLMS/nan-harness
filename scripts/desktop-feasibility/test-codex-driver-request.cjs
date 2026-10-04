const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os'),vm=require('node:vm');
const {validRequest,validBinding,readPrivate}=require('./codex-dom.cjs');
const source=fs.readFileSync(__dirname+'/codex-dom.cjs','utf8');
const begin=source.indexOf('async function main()'),end=source.indexOf('exports.bindRecordedMain=',begin);
(async()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'codex-driver-request-'));
 try {
  for(const scenario of ['request-json','request-policy','connection-read','binding-read','connection-schema','binding-schema','factory-throw','valid']) {
   const dir=path.join(root,scenario);fs.mkdirSync(dir,{mode:0o700});
   const connectionPath=path.join(dir,'connection-9.json'),mainBindingPath=path.join(dir,'main-binding-9.private');
   const request={connectionPath,mainBindingPath,ownerPid:9,prompt:'Check this connection',expectedMarker:'synthetic-nonce',timeoutMs:45000,action:'submit',purpose:'response'};
   const connection={schemaVersion:scenario==='connection-schema'?2:1,launcherPid:11,port:4567};
   const binding={schemaVersion:1,ownerPid:scenario==='binding-schema'?10:9,launcherPid:11,port:4567,auxiliary:null,
    main:{url:'app://-/index.html',target:'target',frame:'frame',loader:'loader',frameUrl:'app://-/index.html',fragment:''}};
   if(scenario!=='connection-read')fs.writeFileSync(connectionPath,JSON.stringify(connection),{mode:0o600});
   if(scenario!=='binding-read')fs.writeFileSync(mainBindingPath,JSON.stringify(binding),{mode:0o600});
   if(scenario==='request-policy')request.action='unknown';
   const input=path.join(dir,'request.private'),output=path.join(dir,'facts.json');
   fs.writeFileSync(input,scenario==='request-json'?'{':JSON.stringify(request),{mode:0o600});
   let ownershipQueries=0;
   const process={argv:['synthetic-node','synthetic-driver','--qualify',input,output],env:{GITHUB_ACTIONS:'true',RUNNER_ENVIRONMENT:'github-hosted',NANH_DESKTOP_RENDERER_APP:'chatgpt-desktop'}};
   const entry=vm.runInNewContext(source.slice(begin,end)+';main',{validRequest,validBinding,readPrivate,process,
    require:name=>name==='./endpoint-ownership.cjs'?{proof:()=>{if(scenario==='factory-throw')throw Error('PRIVATE factory error');return {descendant:()=>{ownershipQueries++;return false;},ownedEndpoint:()=>{ownershipQueries++;return false;}};}}:require(name)});
   await entry();const facts=JSON.parse(fs.readFileSync(output,'utf8'));
   assert.equal(facts.errorCategory,['valid','factory-throw'].includes(scenario)?'ownership-lost':'invalid-request',scenario);
   assert.equal(facts.preAttachFailure,['valid','factory-throw'].includes(scenario)?undefined:scenario,scenario);
   assert.equal(ownershipQueries,scenario==='valid'?1:0,scenario);
   assert.equal(facts.attached,false);assert.equal(facts.inputSubmitted,false);
   assert(!JSON.stringify(facts).includes(root));
  }
 } finally {fs.rmSync(root,{recursive:true,force:true});}
 console.log('PASS: real private request/binding transport, exact closed preattach boundary and no native/app calls');
})().catch(e=>{console.error(e);process.exitCode=1});
