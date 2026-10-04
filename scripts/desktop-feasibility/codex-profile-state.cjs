'use strict';
// PRIVATE retained profile loan. Fixed read-only state; no input capability.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),cp=require('node:child_process');
const C=fs.constants,LIMIT=1024*1024;
const SUFFIXES=['','profile','profile/home','profile/config','profile/nanh','profile/nanh/chatgpt-desktop','profile/nanh/chatgpt-desktop/profile','profile/codex-desktop'];
const same=(a,b)=>a.dev===b.dev&&a.ino===b.ino&&a.uid===b.uid;
const stable=(a,b)=>same(a,b)&&a.size===b.size&&a.mtimeNs===b.mtimeNs&&a.ctimeNs===b.ctimeNs&&a.mode===b.mode&&a.nlink===b.nlink;
function exactKeys(v,keys){return !!v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join('\0')===keys.slice().sort().join('\0');}
function reject(){throw new Error('profile-state-rejected');}
function authority(loan,deadline,guard=()=>true){
  const held=[];let closed=false;
  const close=()=>{if(!closed){closed=true;for(const d of held)try{fs.closeSync(d.fd);}catch{}}};
  const alive=()=>!closed&&Date.now()<deadline&&guard()===true&&Date.now()<deadline;
  function verify(){
    try{return alive()&&held.length===8&&held.every(d=>{
      const m=fs.lstatSync(d.path,{bigint:true}),h=fs.fstatSync(d.fd,{bigint:true});
      return m.isDirectory()&&!m.isSymbolicLink()&&m.mode%4096n===448n&&m.uid===BigInt(process.getuid())&&same(m,h)&&same(m,d.original)&&fs.realpathSync(d.path)===d.path;
    })&&alive();}catch{return false;}
  }
  try{
    if(!['linux','darwin'].includes(process.platform)||!exactKeys(loan,['schemaVersion','platform','directories','stateRootIndex','stateBasename','diagnosticsOnly'])
      ||loan.schemaVersion!==1||loan.platform!==(process.platform==='darwin'?'macos':'linux')||loan.stateRootIndex!==6||loan.stateBasename!=='.codex-global-state.json'||loan.diagnosticsOnly!==true
      ||!Array.isArray(loan.directories)||loan.directories.length!==8||!alive())reject();
    const workspace=loan.directories[0]?.path;
    if(typeof workspace!=='string'||!path.isAbsolute(workspace)||path.normalize(workspace)!==workspace||fs.realpathSync(workspace)!==workspace)reject();
    for(let i=0;i<8;i++){
      const r=loan.directories[i],expected=path.join(workspace,SUFFIXES[i]);
      if(!alive()||!exactKeys(r,['path','device','inode','uid','mode'])||r.path!==expected||r.uid!==process.getuid()||r.mode!==448
        ||typeof r.device!=='string'||typeof r.inode!=='string'||!/^\d+$/.test(r.device)||!/^\d+$/.test(r.inode))reject();
      const fd=fs.openSync(expected,C.O_RDONLY|C.O_DIRECTORY|C.O_NOFOLLOW|C.O_NONBLOCK);
      const m=fs.fstatSync(fd,{bigint:true});held.push({path:expected,fd,original:m});
      if(m.dev!==BigInt(r.device)||m.ino!==BigInt(r.inode)||m.uid!==BigInt(r.uid))reject();
    }
    if(!verify())reject();
  }catch{close();return null;}
  function macSnapshot(){
    if(!verify())reject();
    const remaining=deadline-Date.now();if(remaining<=0)reject();
    const result=cp.spawnSync('/usr/bin/python3',[path.join(__dirname,'codex-macos-state.py')],{
      input:JSON.stringify({loan,deadline,caller:process.pid}),encoding:'utf8',
      timeout:Math.ceil(remaining),maxBuffer:2*LIMIT,windowsHide:true});
    if(result.error||result.status!==0||result.signal||!verify())reject();
    let wire;try{wire=JSON.parse(result.stdout);}catch{reject();}
    const fields=['st_dev','st_ino','st_uid','st_mode','st_nlink','st_size','st_mtime_ns','st_ctime_ns'];
    if(!exactKeys(wire,['bytes','identity'])||!exactKeys(wire.identity,fields)
      ||typeof wire.bytes!=='string'||wire.bytes.length>Math.ceil(LIMIT/3)*4
      ||fields.some(k=>typeof wire.identity[k]!=='string'||!/^\d{1,20}$/.test(wire.identity[k])))reject();
    const bytes=Buffer.from(wire.bytes,'base64');if(bytes.length>LIMIT||bytes.toString('base64')!==wire.bytes)reject();
    const raw=wire.identity,identity={dev:BigInt(raw.st_dev),ino:BigInt(raw.st_ino),uid:BigInt(raw.st_uid),
      mode:BigInt(raw.st_mode),nlink:BigInt(raw.st_nlink),size:BigInt(raw.st_size),mtimeNs:BigInt(raw.st_mtime_ns),ctimeNs:BigInt(raw.st_ctime_ns)};
    if(identity.uid!==BigInt(process.getuid())||identity.mode%4096n!==384n||identity.nlink!==1n||identity.size!==BigInt(bytes.length))reject();
    let value;try{value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes));}catch{reject();}
    if(!verify())reject();return {value,identity,digest:crypto.createHash('sha256').update(bytes).digest('hex')};
  }
  function snapshot(){
    if(process.platform==='darwin')return macSnapshot();
    let fd;
    try{
      if(!verify())reject();
      // A held original directory anchors this open, even during a pathname
      // replacement. No fallback, backup or caller-selected state path exists.
      const state=`/proc/self/fd/${held[6].fd}/.codex-global-state.json`;
      fd=fs.openSync(state,C.O_RDONLY|C.O_NOFOLLOW|C.O_NONBLOCK);
      const before=fs.fstatSync(fd,{bigint:true});
      if(!before.isFile()||before.mode%4096n!==384n||before.uid!==BigInt(process.getuid())||before.nlink!==1n||before.size>BigInt(LIMIT))reject();
      const buffer=Buffer.alloc(LIMIT+1);let size=0;
      while(size<buffer.length){if(!verify())reject();const n=fs.readSync(fd,buffer,size,buffer.length-size,null);if(n===0)break;size+=n;}
      const after=fs.fstatSync(fd,{bigint:true}),named=fs.lstatSync(state,{bigint:true});
      if(size>LIMIT||BigInt(size)!==before.size||!stable(before,after)||!stable(after,named)||!verify())reject();
      const bytes=buffer.subarray(0,size);let value;
      try{value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes));}catch{reject();}
      return {value,identity:after,digest:crypto.createHash('sha256').update(bytes).digest('hex')};
    }finally{if(fd!==undefined)fs.closeSync(fd);}
  }
  function pair(){
    try{const a=snapshot(),b=snapshot();if(!stable(a.identity,b.identity)||a.digest!==b.digest||!verify())reject();
      // Values and digests stay private. Native renderer/context corroboration
      // remains mandatory before any selected-project claim can be made.
      return {first:a,second:b};
    }catch{return null;}
  }
  return {verify,snapshotPair:pair,close};
}
exports.authority=authority;
// Exact frozen public source: src-C1dW0Du8.js/Rl+Bl and shared/eEt+JTt.
// This stricter projection observes one ordinary local project; it does not
// infer current renderer selection from the persisted SELECTED_PROJECT key.
function projectFailure(value,workspace){
  if(!value||typeof value!=='object'||Array.isArray(value)||typeof workspace!=='string')return 'container';
  const projects=value['local-projects'];
  if(!projects||typeof projects!=='object'||Array.isArray(projects))return 'projects-shape';
  const ids=Object.keys(projects);if(ids.length!==1)return 'projects-count';
  const id=ids[0];if(!id||id.startsWith('g-p-'))return 'project-namespace';
  const p=projects[id];
  if(!exactKeys(p,['id','name','rootPaths','createdAt','updatedAt']))return 'record-shape';
  if(p.id!==id||typeof p.name!=='string')return 'record-identity';
  if(!Number.isFinite(p.createdAt)||!Number.isFinite(p.updatedAt))return 'record-time';
  if(!Array.isArray(p.rootPaths)||p.rootPaths.length!==1||p.rootPaths[0]!==workspace)return 'record-root';
  const selected=value['selected-project'];
  // main/ete initializes an absent stored selection from the first local project.
  // This projection supplies only a candidate ID; the actual menu check is mandatory.
  if(selected!=null&&(!exactKeys(selected,['type','projectId'])||selected.type!=='local'||selected.projectId!==id))return 'stored-selection';
  return null;
}
function project(value,workspace){
  if(projectFailure(value,workspace)!==null)return null;
  return {projectId:Object.keys(value['local-projects'])[0],workspace};
}
exports.project=project;
exports.projectFailure=projectFailure;
async function observe(page,guard,deadline,loan,workspace,menu,endpoint=async()=>guard()===true,contextObservation=null,selectionTransition=null){
  const facts={status:'blocked',diagnosticsOnly:true,statePairStable:false,ordinaryLocalProjectObserved:false,
    selectedIdCorrelated:false,sendAuthorized:false};
  const a=authority(loan,deadline,guard);
  if(!a)return {...facts,reason:'profile-custody'};
  try{
    if(workspace!==loan.directories[0].path)return {...facts,reason:'workspace'};
    const before=a.snapshotPair();if(!before)return {...facts,reason:'state'};
    const selected=project(before.first.value,workspace);if(!selected)return {...facts,reason:'project',projectFailure:projectFailure(before.first.value,workspace)};
    facts.ordinaryLocalProjectObserved=true;
    const originalRecord=JSON.stringify(before.first.value['local-projects'][selected.projectId]);
    let expected=before,transitioned=false;
    const sample=require('./codex-selected-project.cjs').sample;
    if(selectionTransition){
      if(!a.verify()||!await endpoint())return {...facts,reason:'guard'};
      const initial=await page.evaluate(sample,{menu,projectId:selected.projectId});
      if(!a.verify()||!await endpoint())return {...facts,reason:'guard'};
      if(initial.reason==='selected-id'&&initial.selectedItemCount===0&&initial.matchingItemCount===1){
        const sealed=a.snapshotPair();
        if(!sealed||!stable(before.first.identity,sealed.second.identity)||before.first.digest!==sealed.second.digest
          ||!a.verify()||!await endpoint())return {...facts,reason:'state-changed'};
        const transition=await selectionTransition(selected,()=>a.verify());
        if(!transition||!transition.menu||transition.completed!==true||!a.verify()||!await endpoint())return {...facts,reason:'selection-transition'};
        menu=transition.menu;
        expected=a.snapshotPair();
        if(!expected||!a.verify()||!await endpoint())return {...facts,reason:'state-changed'};
        const value=expected.first.value,p=project(value,workspace);
        if(!p||p.projectId!==selected.projectId||JSON.stringify(value['local-projects'][selected.projectId])!==originalRecord
          ||!exactKeys(value['selected-project'],['type','projectId'])||value['selected-project'].type!=='local'
          ||value['selected-project'].projectId!==selected.projectId)return {...facts,reason:'selection-state'};
        transitioned=true;
      }
    }
    for(let i=0;i<2;i++){
      if(!a.verify()||!await endpoint())return {...facts,reason:'guard'};
      const result=await page.evaluate(sample,{menu,projectId:selected.projectId});
      if(!a.verify()||!await endpoint())return {...facts,reason:'guard'};
      if(result.status!=='observed'||result.selectedIdCorrelated!==true){
        const selectedProjectObservation={reason:result.reason};
        if(result.reason==='selected-id')Object.assign(selectedProjectObservation,
          {selectedItemCount:result.selectedItemCount,matchingItemCount:result.matchingItemCount});
        return {...facts,reason:'selected-id',selectedProjectObservation};
      }
    }
    const after=a.snapshotPair();
    if(!after||!stable(expected.first.identity,after.second.identity)||expected.first.digest!==after.second.digest||!a.verify()||!await endpoint())return {...facts,reason:'state-changed'};
    const prewarmContext=contextObservation?await contextObservation(selected):undefined;
    if(contextObservation){
      const final=a.snapshotPair();
      if(!final||!stable(expected.first.identity,final.second.identity)||expected.first.digest!==final.second.digest||!a.verify()||!await endpoint())return {...facts,reason:'state-changed'};
    }
    return {...facts,status:'observed',statePairStable:true,selectedIdCorrelated:true,...(transitioned?{ordinarySelectionCompleted:true}:{}),...(prewarmContext?{prewarmContext}:{})};
  }catch{return {...facts,reason:Date.now()>=deadline?'deadline':'query'};}
  finally{a.close();}
}
exports.observe=observe;
