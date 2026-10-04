'use strict';
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'codex-profile-state.cjs'),'utf8');
function fixture(){
  const suffixes=['','profile','profile/home','profile/config','profile/nanh','profile/nanh/chatgpt-desktop','profile/nanh/chatgpt-desktop/profile','profile/codex-desktop'];
  const nodes=new Map(),fds=new Map();let next=10,opens=0,reads=0,now=100,guard=true;
  const metadata=(ino,dir=false)=>({dev:1n,ino:BigInt(ino),uid:123n,mode:dir?448n:384n,size:0n,nlink:1n,mtimeNs:1n,ctimeNs:1n,isDirectory:()=>dir,isFile:()=>!dir,isSymbolicLink:()=>false});
  const loan={schemaVersion:1,platform:'linux',directories:[],stateRootIndex:6,stateBasename:'.codex-global-state.json',diagnosticsOnly:true};
  suffixes.forEach((suffix,i)=>{const p=path.join('/owned',suffix),m=metadata(i+1,true);nodes.set(p,{metadata:m});loan.directories.push({path:p,device:'1',inode:String(i+1),uid:123,mode:448});});
  const root=loan.directories[6].path,state=root+'/.codex-global-state.json';
  function setState(bytes){const m=metadata(20);m.size=BigInt(bytes.length);nodes.set(state,{metadata:m,bytes:Buffer.from(bytes)});}
  setState(Buffer.from('{"synthetic":"private"}'));
  const resolve=p=>{const match=/^\/proc\/self\/fd\/(\d+)\/(.*)$/.exec(p);return match?fds.get(+match[1]).path+'/'+match[2]:p;};
  const fake={constants:fs.constants,
    realpathSync:p=>{if(!nodes.has(p))throw Error();return p;},
    lstatSync:p=>{const n=nodes.get(resolve(p));if(!n)throw Error();return {...n.metadata};},
    fstatSync:fd=>({...fds.get(fd).node.metadata}),
    openSync:(p,flags)=>{opens++;p=resolve(p);const node=nodes.get(p);if(!node||node.metadata.isSymbolicLink())throw Error();
      assert(flags&fs.constants.O_NOFOLLOW);const fd=next++;fds.set(fd,{path:p,node,offset:0});return fd;},
    closeSync:fd=>{assert(fds.delete(fd));},
    readSync:(fd,buffer,start,len)=>{reads++;if(api.onRead)api.onRead();const h=fds.get(fd),data=h.node.bytes;const n=Math.min(len,data.length-h.offset);data.copy(buffer,start,h.offset,h.offset+n);h.offset+=n;return n;}};
  const sandbox={exports:{},require:name=>name==='node:fs'?fake:require(name),Buffer,TextDecoder,Date:{now:()=>now},process:{platform:'linux',getuid:()=>123}};
  vm.runInNewContext(source,sandbox);const api={loan,nodes,fds,root,state,setState,get opens(){return opens;},get reads(){return reads;},set now(v){now=v;},set guard(v){guard=v;}};
  api.make=()=>sandbox.exports.authority(loan,200,()=>guard);api.observe=page=>sandbox.exports.observe(page,()=>guard,200,loan,"/owned",{});return api;
}
let total=0;
function test(name,run){run();total++;console.log('PASS '+name);}
test('fixed managed state pair reads no other file',()=>{const f=fixture(),a=f.make();assert(a);assert(a.verify());const p=a.snapshotPair();assert.equal(p.first.value.synthetic,'private');assert.equal(f.opens,10);a.close();assert.equal(f.fds.size,0);assert(!a.verify());});
test('missing loan and malformed root deny without state read',()=>{for(const mutate of [f=>f.loan.stateRootIndex=2,f=>f.loan.stateBasename='backup.json',f=>f.loan.directories[6].inode='900',f=>f.loan.directories[6].path='/foreign',f=>f.loan.extra=true]){const f=fixture();mutate(f);assert.equal(f.make(),null);assert.equal(f.reads,0);assert.equal(f.fds.size,0);}});
test('replacement of retained parent revokes before read',()=>{const f=fixture(),a=f.make();f.nodes.get('/owned/profile/nanh').metadata.ino=99n;assert(!a.verify());assert.equal(a.snapshotPair(),null);assert.equal(f.reads,0);a.close();});
test('symlink FIFO hardlink permissions and oversized state reject',()=>{for(const mutate of [m=>m.isSymbolicLink=()=>true,m=>m.isFile=()=>false,m=>m.nlink=2n,m=>m.mode=420n,m=>m.uid=456n,m=>m.size=1048577n]){const f=fixture(),a=f.make();mutate(f.nodes.get(f.state).metadata);assert.equal(a.snapshotPair(),null);assert.equal(f.reads,0);a.close();}});
test('expiry and process guard loss revoke loan',()=>{for(const which of ['clock','guard']){const f=fixture(),a=f.make();if(which==='clock')f.now=200;else f.guard=false;assert.equal(a.snapshotPair(),null);assert.equal(f.reads,0);a.close();}});
test('same-inode mutation during read rejects snapshot',()=>{const f=fixture(),a=f.make();f.onRead=()=>{f.nodes.get(f.state).metadata.mtimeNs++;};assert.equal(a.snapshotPair(),null);a.close();});
test('atomic replacement between pair samples rejects',()=>{const f=fixture(),a=f.make();f.onRead=()=>{if(f.reads===2){const n=f.nodes.get(f.state);f.nodes.set(f.state,{metadata:{...n.metadata,ino:99n},bytes:n.bytes});}};assert.equal(a.snapshotPair(),null);a.close();});
test('malformed UTF8 JSON and byte overflow reject without fallback',()=>{for(const bytes of [Buffer.from('{'),Buffer.from([0xff]),Buffer.alloc(1048577,32)]){const f=fixture(),a=f.make();f.setState(bytes);assert.equal(a.snapshotPair(),null);a.close();}});
test('state values remain private and no input method exists',()=>{const f=fixture(),a=f.make();assert.deepEqual(Object.keys(a).sort(),['close','snapshotPair','verify']);a.close();});
const {project,projectFailure}=require('./codex-profile-state.cjs');
function projectFixture(){return {'selected-project':{type:'local',projectId:'fixture-id'},'local-projects':{'fixture-id':{id:'fixture-id',name:'same-basename',rootPaths:['/owned'],createdAt:1,updatedAt:2}}};}
test('frozen ordinary sole local project projects private ID and exact root',()=>{assert.deepEqual(project(projectFixture(),'/owned'),{projectId:'fixture-id',workspace:'/owned'});});
test('absent or null persisted selection supplies only a local candidate',()=>{for(const selected of [undefined,null]){const value=projectFixture();if(selected===undefined)delete value['selected-project'];else value['selected-project']=selected;assert.deepEqual(project(value,'/owned'),{projectId:'fixture-id',workspace:'/owned'});assert.equal(projectFailure(value,'/owned'),null);}});
test('projection failures expose fixed tags without private values',()=>{for(const [mutate,tag] of [[v=>delete v['local-projects'],'projects-shape'],[v=>v['local-projects']={},'projects-count'],[v=>v['local-projects']['fixture-id'].name=42,'record-identity'],[v=>v['local-projects']['fixture-id'].rootPaths=['/other'],'record-root'],[v=>v['selected-project'].projectId='other','stored-selection']]){const value=projectFixture();mutate(value);assert.equal(projectFailure(value,'/owned'),tag);assert.equal(project(value,'/owned'),null);}});
test('remote cloud missing root multiple roots backing and duplicate project deny',()=>{for(const mutate of [v=>v['selected-project'].type='remote',v=>v['selected-project'].projectId='g-p-fixture',v=>v['local-projects']['fixture-id'].rootPaths=[],v=>v['local-projects']['fixture-id'].rootPaths=['/owned','/other'],v=>v['local-projects']['fixture-id'].rootPaths=['/other'],v=>v['local-projects']['fixture-id'].chatGptBacking={},v=>v['local-projects']['other']={...v['local-projects']['fixture-id']}]){const v=projectFixture();mutate(v);assert.equal(project(v,'/owned'),null);}});

console.log(total+' synthetic fixture groups passed');

(async()=>{
  const observed={status:'observed',selectedIdCorrelated:true};
  for(const mode of ['stable','changed-state','guard-lost','wrong-check','absent-selection','absent-wrong-check']){
    const f=fixture(),value=projectFixture();if(mode.startsWith('absent'))delete value['selected-project'];f.setState(Buffer.from(JSON.stringify(value)));let samples=0;
    const page={evaluate:async()=>{samples++;
      if(mode==='changed-state')f.nodes.get(f.state).metadata.mtimeNs++;
      if(mode==='guard-lost')f.guard=false;
      return mode.endsWith('wrong-check')?{status:'blocked'}:observed;
    }};
    const result=await f.observe(page);
    assert.equal(result.status,['stable','absent-selection'].includes(mode)?'observed':'blocked');
    assert.equal(result.sendAuthorized,false);assert.equal(f.fds.size,0);
    assert.equal(JSON.stringify(result).includes('fixture-id'),false);
    assert.equal(JSON.stringify(result).includes('/owned'),false);
    if(mode==='stable')assert.equal(samples,2);
  }
  console.log('PASS paired private state brackets passive selected-check samples; mutation guard and wrong-check deny');
})().catch(e=>{console.error(e);process.exitCode=1;});
