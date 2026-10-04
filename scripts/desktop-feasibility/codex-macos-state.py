#!/usr/bin/env python3
"""Private descriptor-relative fixed-state transport. No app/API/UI operations."""
import base64,json,math,os,stat,sys,time
SUFFIXES=('','profile','profile/home','profile/config','profile/nanh',
          'profile/nanh/chatgpt-desktop','profile/nanh/chatgpt-desktop/profile','profile/codex-desktop')
PARENTS=(None,0,1,1,1,4,5,1)
LIMIT=1024*1024
class Rejected(Exception):pass

def snapshot(request, *, platform=sys.platform, clock=lambda:time.time()*1000):
    held=[]
    try:
        if platform!='darwin' or type(request) is not dict or set(request)!={'loan','deadline','caller'}:
            raise Rejected()
        deadline=request['deadline'];caller=request['caller'];loan=request['loan']
        if (type(deadline) not in (int,float) or not math.isfinite(deadline) or type(caller) is not int or caller<=1
                or type(loan) is not dict or set(loan)!={'schemaVersion','platform','directories','stateRootIndex','stateBasename','diagnosticsOnly'}
                or type(loan['schemaVersion']) is not int or loan['schemaVersion']!=1 or loan['platform']!='macos' or loan['stateRootIndex']!=6
                or type(loan['stateRootIndex']) is not int or loan['stateBasename']!='.codex-global-state.json' or loan['diagnosticsOnly'] is not True
                or type(loan['directories']) is not list or len(loan['directories'])!=8):raise Rejected()
        def alive():
            if clock()>=deadline or os.getppid()!=caller:raise Rejected()
        alive();workspace=loan['directories'][0]['path']
        if type(workspace) is not str or not os.path.isabs(workspace) or os.path.normpath(workspace)!=workspace:raise Rejected()
        flags=os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC
        def directory_matches(index):
            record=loan['directories'][index];fd=held[index]
            path=record['path'];named=os.stat(path,follow_symlinks=False);opened=os.fstat(fd)
            for value in (named,opened):
                if (not stat.S_ISDIR(value.st_mode) or stat.S_IMODE(value.st_mode)!=0o700
                        or value.st_uid!=os.geteuid() or str(value.st_dev)!=record['device']
                        or str(value.st_ino)!=record['inode']):raise Rejected()
            if os.path.realpath(path)!=path:raise Rejected()
        def verify():
            alive()
            for index in range(len(held)):directory_matches(index)
            alive()
        for index,record in enumerate(loan['directories']):
            if (type(record) is not dict or set(record)!={'path','device','inode','uid','mode'}
                    or record['path']!=os.path.join(workspace,SUFFIXES[index]).rstrip('/')
                    or type(record['uid']) is not int or record['uid']!=os.geteuid() or type(record['mode']) is not int or record['mode']!=0o700
                    or any(type(record[k]) is not str or not record[k].isascii() or not record[k].isdecimal() or len(record[k])>20 for k in ('device','inode'))):raise Rejected()
            verify()
            # Every descendant open is anchored to our independently retained
            # same-original parent, not a path reconstructed from /dev/fd.
            fd=(os.open(workspace,flags) if index==0 else
                os.open(os.path.basename(record['path']),flags,dir_fd=held[PARENTS[index]]))
            held.append(fd);verify()
        fd=os.open('.codex-global-state.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,dir_fd=held[6])
        try:
            before=os.fstat(fd)
            if (not stat.S_ISREG(before.st_mode) or stat.S_IMODE(before.st_mode)!=0o600
                    or before.st_uid!=os.geteuid() or before.st_nlink!=1 or not 0<=before.st_size<=LIMIT):raise Rejected()
            data=bytearray()
            while len(data)<=LIMIT:
                verify();part=os.read(fd,min(65536,LIMIT+1-len(data)))
                if not part:break
                data.extend(part)
            after=os.fstat(fd);named=os.stat('.codex-global-state.json',dir_fd=held[6],follow_symlinks=False)
            keys=('st_dev','st_ino','st_uid','st_mode','st_nlink','st_size','st_mtime_ns','st_ctime_ns')
            if len(data)!=before.st_size or any(getattr(before,k)!=getattr(value,k) for k in keys for value in (after,named)):raise Rejected()
            verify()
            return {'bytes':base64.b64encode(data).decode('ascii'),
                    'identity':{k:str(getattr(after,k)) for k in keys}}
        finally:os.close(fd)
    finally:
        for fd in held:os.close(fd)

if __name__=='__main__':
    try:
        raw=sys.stdin.buffer.read(16385)
        if len(raw)>16384:raise Rejected()
        value=snapshot(json.loads(raw))
        sys.stdout.write(json.dumps(value,separators=(',',':')))
    except Exception:
        sys.exit(18)
