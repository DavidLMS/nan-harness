#!/usr/bin/env python3
"""Private native first-turn candidate. Partial results never qualify a cell."""
import importlib.util
from pathlib import Path
import time


def load_visibility():
    spec = importlib.util.spec_from_file_location('claude_visibility',
        Path(__file__).with_name('claude-atspi-visibility.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def inside(rect, outer):
    return (len(rect) == len(outer) == 4 and all(type(v) is int for v in (*rect, *outer))
            and rect[2] > 0 and rect[3] > 0 and outer[2] > 0 and outer[3] > 0
            and rect[0] >= outer[0] and rect[1] >= outer[1]
            and rect[0] + rect[2] <= outer[0] + outer[2]
            and rect[1] + rect[3] <= outer[1] + outer[3])


# AT-SPI roles differ from ATK: section=85, grouping=99, comment=97.
RESPONSE_CONTAINER_ROLES = frozenset((39, 85, 99))

BOUNDARIES = frozenset(('request','policy','native-window','source-owner','tree','tree-cycle','tree-depth','tree-limit','tree-identity','tree-children','response-heading','response-row','response-row-role-limit','response-row-copy-absent','response-row-copy-ambiguous','response-row-headings','response-row-attachment','state',
    'frame','frame-active','frame-count','frame-client','client','mode','focus','input','input-mapping-state','input-mapping-changed','input-empty-state','input-empty-witness','clipboard','action','action-count','action-name','action-hit','response','transport'))
QUERY_BOUNDARIES = dict(owner='source-owner', identity='tree-identity', children='tree-children', parent='frame',
    state='state', bounds='frame', guard='native-window', client_bounds='client',
    attributes='mode', focused='focus', grab_focus='focus', text='input', paste_once='input',
    copy_input_once='clipboard', clear_clipboard='clipboard', clipboard_sentinel='clipboard',
    clipboard_read='clipboard', actions='action-name', hit='action-hit', invoke_once='action')

def action_names(adapter, node):
    try:
        count = adapter.call(node, 'Get', 'org.freedesktop.DBus.Properties',
                             'org.a11y.atspi.Action', 'NActions')
    except Exception as error:
        raise Rejected('action-count') from error
    # dbus.Boolean is an integer subclass; never interpret it as cardinality.
    if (not isinstance(count, int) or isinstance(count, bool)
            or type(count).__name__ == 'Boolean' or not 0 <= count <= 8):
        raise Rejected('action-count')
    names = []
    for index in range(count):
        try:
            name = adapter.call(node, 'GetName', 'org.a11y.atspi.Action', index)
        except Exception as error:
            raise Rejected('action-name') from error
        if not isinstance(name, str) or len(name) > 64:
            raise Rejected('action-name')
        names.append(name)
    return names


def unique_activation_index(actions):
    matches=[index for index,name in enumerate(actions) if name in ('click','press')]
    return matches[0] if len(matches)==1 else None


def input_shape(value):
    # Diagnostic only. These sets never grant replacement or submission authority.
    if type(value) is not str or not 0 < len(value) <= 4096:
        raise Rejected('input')
    return dict(charCount=len(value),onlyLineBreaks=all(c in '\r\n' for c in value),
                onlyWhitespace=value.isspace(),
                onlyZeroWidthMarkers=all(c in '\u200b\u200c\u200d\u2060\ufeff' for c in value),
                onlyObjectReplacement=all(c == '\ufffc' for c in value))


def hypertext_query(adapter,method,target,*args):
    """Typed AT-SPI D-Bus contract; never coerce missing/malformed values."""
    text_interface='org.a11y.atspi.Text'
    link_interface='org.a11y.atspi.Hyperlink'
    hyper_interface='org.a11y.atspi.Hypertext'
    def integer(value):
        if not isinstance(value,int) or isinstance(value,(bool,adapter.dbus.Boolean)):
            raise Rejected('input')
        return int(value)
    def reference(value):
        if not isinstance(value,(tuple,list)) or len(value)!=2 or any(not isinstance(v,str) for v in value):
            raise Rejected('input')
        return tuple(str(v) for v in value)
    def property_value(interface,name):
        return adapter.call(target,'Get','org.freedesktop.DBus.Properties',interface,name)
    if method in ['owner','state','children','parent']:
        return getattr(adapter,method)(target)
    if method=='role':
        return integer(adapter.call(target,'GetRole'))
    if method=='attributes':
        value=adapter.call(target,'GetAttributes')
        if (not isinstance(value,dict) or len(value)>64
                or any(not isinstance(k,str) or not isinstance(v,str) or len(k)>128 or len(v)>1024
                       for k,v in value.items())
                or sum(len(k.encode('utf8'))+len(v.encode('utf8')) for k,v in value.items())>4096):
            raise Rejected('input')
        return {str(k):str(v) for k,v in value.items()}
    if method=='count':
        return integer(property_value(text_interface,'CharacterCount'))
    if method=='text':
        value=adapter.call(target,'GetText',text_interface,0,args[0])
        if not isinstance(value,str):
            raise Rejected('input')
        return str(value)
    if method=='nlinks':
        return integer(adapter.call(target,'GetNLinks',hyper_interface))
    if method=='link-index':
        return integer(adapter.call(target,'GetLinkIndex',hyper_interface,args[0]))
    if method=='link':
        return reference(adapter.call(target,'GetLink',hyper_interface,args[0]))
    if method=='object':
        return reference(adapter.call(target,'GetObject',link_interface,0))
    if method=='valid':
        value=adapter.call(target,'IsValid',link_interface)
        if not isinstance(value,(bool,adapter.dbus.Boolean)):
            raise Rejected('input')
        return bool(value)
    names={'anchors':'NAnchors','start':'StartIndex','end':'EndIndex'}
    if method not in names:
        raise Rejected('input')
    return integer(property_value(link_interface,names[method]))


def input_text_inventory(root, children, resolved, links_used, read, result):
    """Private read-only counters; no text/attributes/nodes escape this query."""
    roles = {node:read('role',node) for node in children}
    attributes = {node:read('attributes',node) for node in children}
    if any(type(role) is not int or not 0 <= role <= 255 for role in roles.values()):
        raise Rejected('input')
    if any(type(attrs) is not dict or len(attrs)>64
        or any(type(k) is not str or type(v) is not str or len(k)>128 or len(v)>1024
               for k,v in attrs.items())
        or sum(len(k.encode())+len(v.encode()) for k,v in attrs.items())>4096
        for attrs in attributes.values()):
        raise Rejected('input')
    paragraphs = [node for node,role in roles.items() if role == 73]
    placeholders = {attrs[key] for attrs in attributes.values()
        for key in ('placeholder','placeholder-text','data-placeholder')
        if key in attrs and attrs[key]}
    source = dict(paragraphTagPCount=0,paragraphEmptyClassPairCount=0,
        paragraphDataPlaceholderCount=0,unresolvedTextLeafCount=0,
        unresolvedOtherRoleCount=0,unresolvedEmptyTextCount=0,
        unresolvedLfTextCount=0,unresolvedExactResultCount=0,
        unresolvedOtherTextCount=0)
    for node,attrs in attributes.items():
        if roles[node]==73:
            source['paragraphTagPCount']+=int(attrs.get('tag')=='p')
            source['paragraphEmptyClassPairCount']+=int(set(attrs.get('class','').split())=={'is-empty','is-editor-empty'})
            source['paragraphDataPlaceholderCount']+=int(bool(attrs.get('data-placeholder')))
        if node in resolved:
            continue
        if children[node] or roles[node] not in (29,61,116):
            source['unresolvedOtherRoleCount']+=1
            continue
        source['unresolvedTextLeafCount']+=1
        count=read('count',node)
        if type(count) is not int or not 0<=count<=4096:
            raise Rejected('input')
        text=read('text',node,count) if count else ''
        if type(text) is not str or len(text)!=count or len(text.encode())>4096:
            raise Rejected('input')
        kind=('unresolvedEmptyTextCount' if text=='' else
              'unresolvedLfTextCount' if text=='\n' else
              'unresolvedExactResultCount' if text==result else 'unresolvedOtherTextCount')
        source[kind]+=1
    root_text = read('text',root,read('count',root))
    return dict(sourceShape=source,nodeCount=len(children),resolvedNodeCount=len(resolved),
        paragraphCount=len(paragraphs),rootChildCount=len(children[root]),
        textLeafCount=sum(not children[n] and roles[n] in (29,61,116) for n in children),
        otherRoleCount=sum(role not in (29,61,73,78,79,116) for role in roles.values()),
        objectLinkCount=links_used,completeTextCoverage=resolved==set(children),
        rootSingleParagraph=(len(children[root])==1 and children[root][0] in paragraphs),
        rootOnlyObjects=bool(root_text) and all(c=='\ufffc' for c in root_text),
        placeholderAttributeMatch=result in placeholders,
        placeholderAttributeLfMatch=any(result==value+'\n' for value in placeholders))


def flatten_hypertext(root, query, pid, budget, observation=None, inventory=None, capability=None):
    """Resolve embedded Text objects; never treat unknown objects as empty."""
    records, parents, children = [], {}, {}
    def read(method,node,*args):
        budget()
        value=query(method,node,*args)
        budget()
        if len(records)>=2048:
            raise Rejected('input')
        records.append((method,node,args,value.copy() if type(value) in (list,dict) else value))
        return value
    def endpoint(node):
        if (type(node) is not tuple or len(node)!=2 or any(type(v) is not str or not v for v in node)
                or node[0]!=root[0] or not node[1].startswith('/') or node[1]=='/org/a11y/atspi/null'):
            raise Rejected('input')
    pending=[(root,0)]
    while pending:
        node,depth=pending.pop()
        endpoint(node)
        if node in children or len(children)>=64 or depth>16 or read('owner',node)!=pid:
            raise Rejected('input')
        bits=read('state',node)
        if type(bits) is not int or not 0<=bits<2**64 or bits&(1<<6):
            raise Rejected('input')
        descendants=read('children',node)
        if type(descendants) is not list or len(descendants)>64-len(children):
            raise Rejected('input')
        children[node]=descendants
        for child in descendants:
            endpoint(child)
            if child in parents or child==root or read('parent',child)!=node:
                raise Rejected('input')
            parents[child]=node
            pending.append((child,depth+1))
    resolved=set()
    links_used=0
    def text(node):
        nonlocal links_used
        if node in resolved:
            raise Rejected('input')
        resolved.add(node)
        count=read('count',node)
        if type(count) is not int or not 0<=count<=4096:
            raise Rejected('input')
        # CharacterCount zero is positive Text-interface proof, not a missing value.
        value=read('text',node,count) if count else ''
        if type(value) is not str or len(value)!=count or len(value.encode('utf8'))>4096:
            raise Rejected('input')
        offsets=[i for i,c in enumerate(value) if c=='\ufffc']
        if not offsets:
            return value
        nlinks=read('nlinks',node)
        if type(nlinks) is not int or nlinks!=len(offsets) or links_used+nlinks>64:
            raise Rejected('input')
        links_used+=nlinks
        indices=set();pieces=[];last=0
        for offset in offsets:
            index=read('link-index',node,offset)
            if type(index) is not int or not 0<=index<nlinks or index in indices:
                raise Rejected('input')
            indices.add(index)
            link=read('link',node,index);endpoint(link)
            if read('owner',link)!=pid or read('valid',link) is not True:
                raise Rejected('input')
            anchors,start,end=(read(method,link) for method in ['anchors','start','end'])
            if (any(type(v) is not int for v in [anchors,start,end])
                    or anchors!=1 or start!=offset or end!=offset+1):
                raise Rejected('input')
            child=read('object',link);endpoint(child)
            if child not in children[node] or parents.get(child)!=node:
                raise Rejected('input')
            pieces.extend([value[last:offset],text(child)]);last=offset+1
        pieces.append(value[last:]);result=''.join(pieces)
        if len(result.encode('utf8'))>4096:
            raise Rejected('input')
        return result
    result=text(root)
    shape=None
    if observation is not None and result:
        shape=dict(nodeCount=len(children),paragraphCount=0,literalLfLeafCount=0,
                   brLfLeafCount=0,exactFillerLfLeafCount=0)
        for node,descendants in children.items():
            role=read('role',node)
            attributes=read('attributes',node)
            if (type(role) is not int or not 0<=role<=255 or type(attributes) is not dict
                    or len(attributes)>64 or any(type(k) is not str or type(v) is not str
                        or len(k)>128 or len(v)>1024 for k,v in attributes.items())
                    or sum(len(k.encode('utf8'))+len(v.encode('utf8')) for k,v in attributes.items())>4096):
                raise Rejected('input')
            shape['paragraphCount']+=int(role==73)
            # Chromium exposes line breaks as static text. Other leaf roles are
            # still owned/reproved, but cannot establish a literal LF text leaf.
            if descendants or role not in (29,61,116):
                continue
            count=read('count',node)
            if type(count) is not int or not 0<=count<=4096:
                raise Rejected('input')
            value=read('text',node,count) if count else ''
            if type(value) is not str or len(value)!=count or len(value.encode('utf8'))>4096:
                raise Rejected('input')
            if value!='\n':
                continue
            shape['literalLfLeafCount']+=1
            if attributes.get('tag')=='br':
                shape['brLfLeafCount']+=1
                if attributes.get('class','').split()==['ProseMirror-trailingBreak']:
                    shape['exactFillerLfLeafCount']+=1
    detail = (input_text_inventory(root,children,resolved,links_used,read,result)
              if inventory is not None else None)
    empty_witness=None
    if capability is not None and detail is not None:
        source=detail['sourceShape']
        # Exact frozen dO decoration implies Gt(entire model doc) with
        # ignoreWhitespace=false. Every unmapped native node is still sealed;
        # no descendant is silently assumed empty or dropped from reproof.
        if (detail['rootSingleParagraph'] and detail['paragraphCount']==1
                and source['paragraphTagPCount']==1
                and source['paragraphEmptyClassPairCount']==1
                and source['unresolvedOtherRoleCount']==0
                and detail['rootOnlyObjects'] and 0<len(result.encode())<=4096):
            empty_witness=(root,result,tuple(records))
    # Reprove the entire owned attachment/text/link mapping, including zero-length leaves.
    for method,node,args,value in records:
        budget()
        if query(method,node,*args)!=value:
            raise Rejected('input-mapping-state' if method=='state' else 'input-mapping-changed')
        budget()
    if shape is not None:
        observation.update(shape)
    if detail is not None:
        inventory.update(detail)
    if capability is not None:
        capability['witness']=empty_witness
    return result


class Rejected(Exception):
    def __init__(self, boundary=None):
        self.boundary = boundary if boundary in BOUNDARIES else None
        super().__init__()


class ProfileCustody:
    """Private retained directory identities; never exported as diagnostic facts."""
    def __init__(self, records, deadline, clock=time.monotonic):
        import os
        from pathlib import Path
        self.handles, self.records, self.deadline, self.clock = [], records, deadline, clock
        try:
            if type(records) is not list or len(records) != 7:
                raise Rejected('policy')
            workspace = records[0]['path']
            expected = [workspace, workspace+'/profile', workspace+'/profile/home',
                workspace+'/profile/config', workspace+'/profile/nanh',
                workspace+'/profile/config/Claude', workspace+'/profile/config/Claude-3p']
            for record, path in zip(records, expected):
                if (type(record) is not dict or set(record) != {'path','device','inode','uid'}
                    or record['path'] != path or not Path(path).is_absolute()
                    or any(type(record[key]) is not int or record[key] < 0 for key in ('device','inode','uid'))
                    or record['uid'] != os.getuid() or self.clock() >= deadline):
                    raise Rejected('policy')
                self.handles.append(os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW))
            if not self.verify():
                raise Rejected('policy')
        except Exception:
            self.close()
            raise
    def verify(self):
        import os, stat
        if self.clock() >= self.deadline or len(self.handles) != 7:
            return False
        try:
            for record, handle in zip(self.records, self.handles):
                if self.clock() >= self.deadline or os.path.realpath(record['path']) != record['path']:
                    return False
                current, held = os.lstat(record['path']), os.fstat(handle)
                expected = (record['device'],record['inode'],record['uid'])
                if (not stat.S_ISDIR(current.st_mode) or current.st_mode & 0o077
                    or (current.st_dev,current.st_ino,current.st_uid) != expected
                    or (held.st_dev,held.st_ino,held.st_uid) != expected):
                    return False
            return self.clock() < self.deadline
        except OSError:
            return False
    def close(self):
        import os
        for handle in self.handles:
            os.close(handle)
        self.handles.clear()


class Controller:
    def __init__(self, adapter, root, deadline, clock=time.monotonic, sleep=time.sleep):
        self.adapter, self.root, self.deadline = adapter, root, deadline
        self.clock, self.sleep = clock, sleep
        self.editor = self.frame = self.send = None
        self.chat = self.mode = None
        self.sealed_editor = self.sealed_frame = None
        self.boundary = 'request'
        self.restored = False
        self.copy_attempted = False
        self.focus_attempted = self.paste_attempted = self.send_attempted = False
        self.facts = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
            stage='source', inputVerified=False, pasteAttempted=False,
            sendAttempted=False, sendForwarded=False, responseVerified=False,
            toolVerified=False, recoveryVerified=False)

    def query(self, method, *args):
        self.boundary = QUERY_BOUNDARIES.get(method, 'request')
        if self.clock() >= self.deadline:
            raise TimeoutError()
        custody = getattr(self.adapter, 'profile_guard', None)
        if custody is not None and not custody():
            raise Rejected('policy')
        result = getattr(self.adapter, method)(*args)
        if custody is not None and not custody():
            raise Rejected('policy')
        if self.clock() >= self.deadline:
            raise TimeoutError()
        return result

    def failure(self, error):
        self.facts['failureBoundary'] = (error.boundary if isinstance(error, Rejected)
            and error.boundary is not None else self.boundary)

    def owned(self, node):
        if self.query('owner', node) != self.root['pid']:
            raise Rejected('source-owner')

    def tree(self, start=None):
        root = start if start is not None else (self.root['bus'], self.root['path'])
        pending, seen, nodes = [(root, 0)], set(), []
        while pending:
            node, depth = pending.pop()
            if node in seen:
                raise Rejected('tree-cycle')
            if depth > 32:
                raise Rejected('tree-depth')
            if len(seen) >= 1024:
                raise Rejected('tree-limit')
            seen.add(node)
            self.owned(node)
            identity = self.query('identity', node)
            if type(identity) is not tuple or len(identity) != 3:
                raise Rejected('tree-identity')
            nodes.append((node, identity))
            children = self.query('children', node)
            if type(children) is not list or len(children) > 1024 - len(seen):
                raise Rejected('tree-children')
            pending.extend((child, depth + 1) for child in children)
        return nodes

    def state(self, node, editable=False, frame=False):
        self.owned(node)
        bits = self.query('state', node)
        if (type(bits) is not int or not 0 <= bits < 2**64 or bits & (1 << 6)
                or not bits & (1 << 30) or not bits & (1 << 25)
                or editable and not bits & (1 << 7)):
            raise Rejected('input' if editable else 'frame' if frame else 'state')
        return bits

    def bind(self):
        while True:
            if not self.query('guard'):
                raise Rejected('native-window')
            nodes = self.tree()
            editors = [node for node, identity in nodes if identity[0] in (61, 78, 79)
                       and 'Write your prompt to Claude' in identity[1:]]
            # Only complete, owned trees with no source editor may be mounting.
            # Duplicate editors and every query/ownership failure remain terminal.
            if len(editors) > 1:
                raise Rejected('tree')
            if not self.query('guard'):
                raise Rejected('native-window')
            if editors:
                break
            self.boundary = 'tree'
            remaining = self.deadline - self.clock()
            if remaining <= 0:
                raise TimeoutError()
            self.sleep(min(0.05, remaining))
        self.editor = editors[0]
        self.adapter.key_guard = lambda: self.proof(focused=True)
        self.state(self.editor, editable=True)
        self.sealed_editor = (self.query('identity', self.editor), self.query('bounds', self.editor))
        node, seen, frames = self.editor, set(), []
        for _ in range(32):
            if node in seen:
                raise Rejected()
            seen.add(node)
            self.owned(node)
            identity = self.query('identity', node)
            if node == (self.root['bus'], self.root['path']):
                # Application is the held ownership root, not a drawn component.
                # VISIBLE/SHOWING apply to the editor and its visual ancestors.
                if identity[0] != 75:
                    raise Rejected('source-owner')
                break
            self.state(node)
            if identity[0] == 23:  # Public AT-SPI Frame role.
                self.state(node, frame=True)
                frames.append(node)
            node = self.query('parent', node)
        else:
            raise Rejected()
        if len(frames) != 1:
            raise Rejected('frame-count')
        self.frame = frames[0]
        self.sealed_frame = (self.query('identity', self.frame), self.query('bounds', self.frame))
        self.proof()

    def proof(self, focused=False, pending=False):
        if not self.query('guard'):
            raise Rejected('native-window')
        if self.chat is not None:
            self.owned(self.chat)
            self.owned(self.mode)
            self.state(self.chat)
            self.state(self.mode)
            if self.chat not in self.query('children', self.mode):
                raise Rejected()
            if self.query('identity',self.chat)!=(43,'Chat','') or self.query('attributes',self.chat).get('current')!='page':
                raise Rejected()
        if getattr(self, "response_only", False):
            self.response_frame_proof()
            return True
        if getattr(self,'next_input',False):
            if self.next_consumed and focused:
                raise Rejected('policy')
            if not self.next_consumed:
                self.next_input_proof()
                self.empty_class_proof()
        self.state(self.editor, editable=True)
        # The exact retained native X11 foreground/client/clear-stack proof above
        # supplies window activation authority. AT-SPI ACTIVE is not required:
        # Chromium may export a visible Frame without that duplicate state.
        # Its owned identity, visibility, attachment and exact client bounds remain required.
        self.state(self.frame, frame=True)
        if (self.query('identity', self.editor), self.query('bounds', self.editor)) != self.sealed_editor:
            raise Rejected()
        if (self.query('identity', self.frame), self.query('bounds', self.frame)) != self.sealed_frame:
            raise Rejected()
        client = self.query('client_bounds')
        if self.sealed_frame[1] != client or not inside(self.sealed_editor[1], client):
            raise Rejected('frame-client')
        # Revalidate attachment, not merely a detached retained node with same PID.
        node, seen = self.editor, set()
        for _ in range(32):
            if node in seen:
                raise Rejected()
            seen.add(node)
            self.owned(node)
            self.state(node)
            if node == self.frame:
                break
            node = self.query('parent', node)
        else:
            raise Rejected()
        focused_now = not focused or self.query('focused', self.editor)
        if not self.query('guard'):
            raise Rejected('native-window')
        if not focused_now:
            if pending:
                return False
            raise Rejected()
        return True

    def settle_focus(self):
        while not self.proof(focused=True, pending=True):
            remaining = self.deadline - self.clock()
            if remaining <= 0:
                raise TimeoutError()
            self.sleep(min(.02, remaining))

    def current_chat(self, nodes):
        modes = [node for node, identity in nodes if identity[0] in RESPONSE_CONTAINER_ROLES
                 and 'Mode' in identity[1:]]
        if len(modes) != 1:
            raise Rejected('mode')
        chat = [node for node in self.query('children', modes[0])
                if self.query('identity', node) == (43, 'Chat', '')]
        if len(chat) != 1 or self.query('attributes', chat[0]).get('current') != 'page':
            raise Rejected('mode')
        self.state(chat[0])
        node,seen=chat[0],set()
        for _ in range(32):
            if node in seen:
                raise Rejected()
            seen.add(node)
            self.owned(node)
            if node==self.frame:
                break
            node=self.query('parent',node)
        else:
            raise Rejected()
        self.chat,self.mode=chat[0],modes[0]

    def submit(self, prompt, replace_owned=False):
        try:
            if getattr(self, 'response_only', False):
                raise Rejected('policy')
            if self.focus_attempted or self.paste_attempted or self.send_attempted:
                raise Rejected()
            if type(prompt) is not str or not prompt or len(prompt.encode()) > 4096:
                raise Rejected()
            self.proof() if self.restored else self.bind()
            nodes = self.tree()
            self.current_chat(nodes)
            if replace_owned:
                if self.restored or getattr(self.adapter, 'profile_guard', None) is None:
                    raise Rejected('policy')
                # A fresh profile cannot contain prior source conversation turns.
                if any(identity[0] == 83 and identity[1].startswith(('You said:', 'Claude responded:'))
                       for _, identity in nodes):
                    raise Rejected('policy')
            # Ordinary input requires emptiness; only the first owned fresh-profile
            # operation may replace a stable draft, with exact post-paste readback.
            initial = self.query('text', self.editor)
            if type(initial) is not str or len(initial.encode()) > 4096:
                raise Rejected('input')
            if initial != '' and not replace_owned and getattr(self,'empty_class_witness',None) is None:
                self.facts['inputShape'] = input_shape(initial)
                embedded=getattr(self.adapter,'embedded_text_observation',None)
                if embedded is not None:
                    self.facts['embeddedTextObservation']=embedded
                self.facts['stage'] = 'input-not-empty'
                self.facts['failureBoundary'] = 'input'
                return self.facts
            self.facts['stage'] = 'focus'
            self.proof()
            self.focus_attempted = True
            if self.query('grab_focus', self.editor) is not True:
                raise Rejected()
            self.settle_focus()
            if self.query('text', self.editor) != initial:
                raise Rejected()
            self.facts['stage'] = 'paste'
            self.proof(focused=True)
            if not self.query('focused', self.editor):
                raise Rejected()
            self.paste_attempted = True
            self.facts['pasteAttempted'] = True
            if replace_owned:
                self.query('select_all_once')
                self.proof(focused=True)
            if getattr(self,'empty_class_witness',None) is not None:
                self.adapter.before_paste=self.before_empty_class_paste
            self.query('paste_once', prompt)
            while True:
                self.settle_focus()
                value = self.query('text', self.editor)
                if type(value) is not str or len(value.encode()) > 4096:
                    raise Rejected()
                if value == prompt:
                    break
                if value and not prompt.startswith(value) and not ((replace_owned or getattr(self,'empty_class_dispatched',False)) and value == initial):
                    raise Rejected()
                self.sleep(min(.02, max(0, self.deadline - self.clock())))
            self.facts['stage'] = 'readback'
            self.settle_focus()
            if self.query('copy_input_once', self.editor) != prompt:
                raise Rejected()
            self.proof(focused=True)
            self.facts['inputVerified'] = True
            nodes = self.tree()
            self.current_chat(nodes)
            sends = [node for node, identity in nodes if identity[0] == 43
                     and identity[1] in ('Start task', 'Send message')]
            if len(sends) != 1:
                raise Rejected()
            self.send = sends[0]
            sealed = (self.query('identity', self.send), self.query('bounds', self.send))
            sealed_actions = None
            for _ in range(2):
                self.proof(focused=True)
                self.state(self.send)
                bits = self.query('state', self.send)
                if not bits & (1 << 8) or not bits & (1 << 24):
                    raise Rejected()
                if sealed != (self.query('identity', self.send), self.query('bounds', self.send)):
                    raise Rejected()
                if not inside(sealed[1], self.sealed_frame[1]):
                    raise Rejected()
                actions = self.query('actions', self.send)
                self.facts['sendActionClass'] = ('click' if actions == ['click'] else
                    'press' if actions == ['press'] else 'none' if not actions else
                    'multiple' if len(actions) > 1 else 'other')
                matches=[index for index,name in enumerate(actions) if name in ('click','press')]
                selected_index=unique_activation_index(actions)
                self.facts['sendActionObservation']=dict(actionCount=len(actions),activationMatchCount=len(matches),
                    selectedIndex=selected_index,activationClass=actions[selected_index] if selected_index is not None
                    else 'none' if not matches else 'ambiguous')
                if selected_index is None:
                    raise Rejected('action-name')
                if sealed_actions is not None and actions != sealed_actions:
                    raise Rejected('action-name')
                sealed_actions = actions
                if not self.query('hit', self.send, self.frame):
                    raise Rejected('action-hit')
            self.proof(focused=True)
            if getattr(self,'next_input',False):
                self.next_consumed = True
            self.facts['stage'] = 'send'
            self.send_attempted = True
            self.facts['sendAttempted'] = True
            if self.query('invoke_once', self.send, selected_index) is not True:
                raise Rejected()
            self.facts['sendForwarded'] = True
            self.proof()
            # Assistant Copy is a separate operation; no response is claimed by Send.
            self.facts['stage'] = 'sent'
        except TimeoutError as error:
            self.failure(error)
            self.facts['stage'] = 'deadline'
        except Exception as error:
            self.failure(error)
            self.facts['stage'] = 'action-uncertain' if self.send_attempted else 'blocked'
        finally:
            try:
                self.adapter.clear_clipboard()
            except Exception:
                self.facts['failureBoundary'] = 'clipboard'
                self.facts['stage'] = 'clipboard-cleanup'
            if self.facts['stage']!='input-not-empty':
                self.facts.pop('embeddedTextObservation',None)
                self.facts.pop('ownedInputObservation',None)
        return self.facts


    def binding(self):
        return dict(editor=self.editor,frame=self.frame,
            editorIdentity=self.sealed_editor[0],editorBounds=self.sealed_editor[1],
            frameIdentity=self.sealed_frame[0],frameBounds=self.sealed_frame[1])

    def restore(self, binding, response=False):
        if type(binding) is not dict or set(binding) != {'editor','frame','editorIdentity',
                'editorBounds','frameIdentity','frameBounds'}:
            raise Rejected()
        self.editor = tuple(binding['editor'])
        self.frame = tuple(binding['frame'])
        self.sealed_editor = (tuple(binding['editorIdentity']),tuple(binding['editorBounds']))
        self.sealed_frame = (tuple(binding['frameIdentity']),tuple(binding['frameBounds']))
        self.restored = True
        if response:
            self.restore_response_editor()
        else:
            self.adapter.key_guard = lambda: self.proof(focused=True)
            self.proof()

    def response_frame_proof(self):
        if not self.query('guard'):
            raise Rejected('native-window')
        self.state(self.frame, frame=True)
        if (self.query('identity',self.frame),self.query('bounds',self.frame)) != self.sealed_frame:
            raise Rejected('frame')
        if self.query('client_bounds') != self.sealed_frame[1]:
            raise Rejected('frame-client')
        node,seen=self.frame,set()
        for _ in range(32):
            if node in seen:
                raise Rejected('frame')
            seen.add(node);self.owned(node)
            identity=self.query('identity',node)
            if node==(self.root['bus'],self.root['path']):
                if identity[0]!=75:
                    raise Rejected('source-owner')
                break
            self.state(node)
            if node!=self.frame and identity[0] in (23,69):
                raise Rejected('frame')
            node=self.query('parent',node)
        else:
            raise Rejected('frame')
        if not self.query('guard'):
            raise Rejected('native-window')

    def restore_response_editor(self):
        # Copy uses the original frame, not a draft editor replaced by Send.
        # This capability never refreshes the input binding or allows keys.
        self.response_only = True
        self.response_frame_proof()
        nodes=self.tree(self.frame)
        if any(node!=self.frame and identity[0] in (23,69) for node,identity in nodes):
            raise Rejected('frame')
        self.current_chat(nodes)
        def no_keys():
            raise Rejected('policy')
        self.adapter.key_guard = no_keys
        self.proof()

    def next_history_scope(self, history):
        # Read-only observable authority: exact owned prompts plus independently
        # copied nonces. Tree traversal order never establishes chronology.
        self.response_frame_proof()
        nodes = self.tree(self.frame)
        if any(node != self.frame and identity[0] in (16,23,69) for node,identity in nodes):
            raise Rejected('frame')
        self.current_chat(nodes)
        headings = [(node,identity) for node,identity in nodes if identity[0] == 83
            and identity[1].startswith(('You said:', 'Claude responded:'))]
        if len(headings) != 2*len(history):
            raise Rejected('response-heading')
        matched = set()
        witness = []
        for prompt,marker in history:
            users = [(node,identity) for node,identity in headings
                if identity[1] == 'You said: '+prompt]
            assistants = [(node,identity) for node,identity in headings
                if identity[1].startswith('Claude responded: ') and marker in identity[1]]
            if len(users) != 1 or len(assistants) != 1:
                raise Rejected('response-heading')
            for node,identity in users+assistants:
                if node in matched:
                    raise Rejected('response-heading')
                matched.add(node)
                self.state(node)
                chain = []
                child,seen = node,set()
                for _ in range(32):
                    if child in seen:
                        raise Rejected('response-row-attachment')
                    seen.add(child);self.owned(child)
                    if child == self.frame:
                        break
                    parent = self.query('parent',child)
                    self.owned(parent)
                    if child not in self.query('children',parent):
                        raise Rejected('response-row-attachment')
                    chain.append((child,parent,self.query('identity',parent)))
                    child = parent
                else:
                    raise Rejected('response-row-attachment')
                witness.append((node,identity,tuple(chain)))
        scope = self.response_scope(history[-1][1])
        if scope is None:
            raise Rejected('response')
        self.response_frame_proof()
        return tuple(witness),scope,(self.mode,self.chat)

    def restore_next_input(self, binding, records):
        # A distinct one-use capability. Ordinary restore retains its exact
        # editor/bounds contract; Copy remains frame-only and rejects keys.
        if (getattr(self,'next_input',False) or self.restored
                or getattr(self.adapter,'profile_guard',None) is None
                or type(records) is not list or not 1 <= len(records) <= 2):
            raise Rejected('policy')
        history = []
        for record in records:
            if (type(record) is not dict or set(record) != {'prompt','marker'}
                    or any(type(record[k]) is not str or not record[k]
                        or len(record[k].encode()) > 4096 for k in ('prompt','marker'))):
                raise Rejected('policy')
            history.append((record['prompt'],record['marker']))
        if len({p for p,_ in history}) != len(history) or len({m for _,m in history}) != len(history):
            raise Rejected('policy')
        # Load the original frame without querying or adopting the old editor.
        self.restore(binding,response=True)
        before = self.next_history_scope(history)
        nodes = self.tree(self.frame)
        editors = [node for node,identity in nodes if identity[0] in (61,78,79)
            and 'Write your prompt to Claude' in identity[1:]]
        if len(editors) != 1:
            raise Rejected('tree')
        editor = editors[0]
        self.state(editor,editable=True)
        sealed = (self.query('identity',editor),self.query('bounds',editor))
        if not inside(sealed[1],self.sealed_frame[1]):
            raise Rejected('frame')
        initial = self.query('text',editor)
        inventory = getattr(self.adapter,'input_text_inventory',None)
        witness=getattr(self.adapter,'empty_class_witness',None)
        allow_empty=getattr(self,'empty_class_opt_in',False) and witness is not None
        if initial != '' and not allow_empty:
            if self.next_history_scope(history) != before:
                raise Rejected('response')
            self.state(editor,editable=True)
            if (self.query('identity',editor),self.query('bounds',editor)) != sealed:
                raise Rejected('input')
            if (self.query('text',editor) != initial
                    or getattr(self.adapter,'input_text_inventory',None) != inventory):
                raise Rejected('input')
            if self.next_history_scope(history) != before:
                raise Rejected('response')
            if inventory is not None:
                self.facts['ownedInputObservation'] = dict(inventory,
                    knownPromptMatchCount=sum(initial==prompt for prompt,_ in history),
                    latestPromptMatches=initial==history[-1][0])
            self.facts['stage'] = 'input-not-empty'
            self.facts['inputShape'] = input_shape(initial)
            embedded = getattr(self.adapter,'embedded_text_observation',None)
            if embedded is not None:
                self.facts['embeddedTextObservation'] = embedded
            raise Rejected('input')
        if self.next_history_scope(history) != before:
            raise Rejected('response')
        self.editor,self.sealed_editor = editor,sealed
        self.next_history,self.next_witness = history,before
        self.empty_class_witness=witness if allow_empty else None
        self.empty_class_dispatched=False
        self.next_input = True
        self.next_consumed = False
        self.response_only = False
        self.adapter.key_guard = lambda: self.proof(focused=True)
        self.proof()

    def empty_class_proof(self):
        if self.empty_class_witness is None or self.empty_class_dispatched:
            return
        self.query('text',self.editor)
        current=getattr(self.adapter,'empty_class_witness',None)
        if current!=self.empty_class_witness:
            original=self.empty_class_witness
            state_only=(type(current) is tuple and type(original) is tuple
                and len(current)==len(original)==3 and current[:2]==original[:2]
                and len(current[2])==len(original[2])
                and all(a==b or a[:3]==b[:3] and a[0]=='state'
                    for a,b in zip(current[2],original[2])))
            raise Rejected('input-empty-state' if state_only else 'input-empty-witness')

    def before_empty_class_paste(self):
        if self.empty_class_witness is None or self.empty_class_dispatched:
            raise Rejected('policy')
        self.proof(focused=True)
        self.empty_class_proof()
        self.empty_class_dispatched=True

    def next_input_proof(self):
        if self.next_history_scope(self.next_history) != self.next_witness:
            raise Rejected('response')
        nodes = self.tree(self.frame)
        if any(node != self.frame and identity[0] in (16,23,69) for node,identity in nodes):
            raise Rejected('frame')
        editors = [node for node,identity in nodes if identity[0] in (61,78,79)
            and 'Write your prompt to Claude' in identity[1:]]
        if editors != [self.editor]:
            raise Rejected('tree')
        child,seen = self.editor,set()
        for _ in range(32):
            if child in seen:
                raise Rejected('tree')
            seen.add(child);self.owned(child);self.state(child)
            if child == self.frame:
                break
            parent = self.query('parent',child)
            self.owned(parent)
            if child not in self.query('children',parent):
                raise Rejected('tree')
            child = parent
        else:
            raise Rejected('tree')
        if self.next_history_scope(self.next_history) != self.next_witness:
            raise Rejected('response')

    def response_scope(self, marker):
        nodes = self.tree(self.frame)
        if any(node!=self.frame and identity[0] in (23,69) for node,identity in nodes):
            raise Rejected('frame')
        headings = [node for node,identity in nodes if identity[0] == 83
            and identity[1].startswith('Claude responded: ') and marker in identity[1]]
        if not headings:
            return None
        if len(headings) != 1:
            raise Rejected('response-heading')
        heading, parent, seen = headings[0], self.query('parent',headings[0]), set()
        row_failure = None
        for _ in range(6):
            if parent in seen:
                raise Rejected('response-row-attachment')
            if parent == self.frame:
                raise Rejected(row_failure or 'response-row-role-limit')
            seen.add(parent)
            self.owned(parent)
            identity = self.query('identity',parent)
            if identity[0] not in RESPONSE_CONTAINER_ROLES:
                # Wrappers do not grant row authority. Continue only along the
                # owned, bounded chain to a panel/section/grouping with the exact heading.
                parent = self.query('parent',parent)
                continue
            pending, subtree = [parent], []
            visited = set()
            while pending:
                node = pending.pop()
                if node in visited or len(visited) >= 256:
                    raise Rejected('response-row')
                visited.add(node)
                self.owned(node)
                item = self.query('identity',node)
                subtree.append((node,item))
                pending.extend(self.query('children',node))
            scopes = [node for node,item in subtree if item[0] == 83]
            copies = [node for node,item in subtree if item[0] == 43 and item[1] == 'Copy']
            if scopes == [heading] and len(copies) == 1:
                # A child enumeration alone may be stale during rendering.
                # The retained Copy node must still attach to this exact row.
                copy, ancestors = copies[0], set()
                for _ in range(6):
                    if copy in ancestors or copy == self.frame:
                        raise Rejected('response-row-attachment')
                    ancestors.add(copy);self.owned(copy)
                    if copy == parent:
                        return parent,heading,copies[0]
                    copy = self.query('parent',copy)
                raise Rejected('response-row-attachment')
            if row_failure is None:
                row_failure = ('response-row-headings' if scopes != [heading]
                    else 'response-row-copy-absent' if not copies
                    else 'response-row-copy-ambiguous')
            parent = self.query('parent',parent)
        raise Rejected(row_failure or 'response-row-role-limit')

    def copy_response(self, marker):
        try:
            if self.copy_attempted:
                raise Rejected()
            self.proof()
            self.current_chat(self.tree(self.frame))
            scope = self.response_scope(marker)
            if scope is None:
                self.facts['stage'] = 'response-pending'
                return self.facts
            row,heading,button = scope
            sealed = (self.query('identity',button),self.query('bounds',button))
            sealed_actions = None
            selected_index = None
            for _ in range(2):
                self.proof()
                if self.response_scope(marker) != scope:
                    raise Rejected()
                self.state(button)
                bits = self.query('state',button)
                if not bits & (1<<8) or not bits & (1<<24):
                    raise Rejected()
                if (self.query('identity',button),self.query('bounds',button)) != sealed:
                    raise Rejected()
                if not inside(sealed[1],self.sealed_frame[1]):
                    raise Rejected()
                actions=self.query('actions',button)
                selected_index=unique_activation_index(actions)
                if selected_index is None or sealed_actions is not None and actions != sealed_actions:
                    raise Rejected('action-name')
                sealed_actions=actions
                if not self.query('hit',button,self.frame):
                    raise Rejected()
            self.query('clipboard_sentinel')
            self.proof()
            if (self.response_scope(marker) != scope
                    or (self.query('identity',button),self.query('bounds',button)) != sealed
                    or self.query('actions',button) != sealed_actions
                    or not self.query('hit',button,self.frame)):
                raise Rejected()
            bits=self.state(button)
            if not bits & (1<<8) or not bits & (1<<24):
                raise Rejected('state')
            self.copy_attempted = True
            if self.query('invoke_once',button,selected_index) is not True:
                raise Rejected()
            self.proof()
            value = self.query('clipboard_read')
            if value != marker:
                self.facts['stage'] = 'response-mismatch'
                self.facts['failureBoundary'] = 'clipboard'
                return self.facts
            self.facts['responseVerified'] = True
            self.facts['stage'] = 'copied'
        except TimeoutError as error:
            self.failure(error)
            self.facts['stage'] = 'deadline'
        except Exception as error:
            self.failure(error)
            self.facts['stage'] = 'action-uncertain' if self.copy_attempted else 'blocked'
        return self.facts


def snapshot_clear(text, held):
    """Fixed native --windows protocol; payload stays private in memory."""
    lines = text.splitlines()
    if not lines or len(lines) > 1057:
        return False
    first = lines.pop(0).split()
    if len(first) != 3 or first[0] != 'FG' or int(first[1]) != held['pid']:
        return False
    def coordinate(value):
        import math
        number = float(value)
        if not math.isfinite(number) or not number.is_integer():
            raise ValueError('bounds')
        return int(number)
    displays, windows = [], []
    for line in lines:
        fields = line.split()
        if len(fields) == 5 and fields[0] == 'DISPLAY':
            displays.append(tuple(int(value) for value in fields[1:]))
        elif len(fields) == 8 and fields[0] == 'WIN':
            windows.append(dict(id=int(fields[1]),pid=int(fields[2]),
                bounds=tuple(coordinate(value) for value in fields[3:7]),name=fields[7]))
        else:
            return False
    if len(displays) > 32 or len(windows) > 1024:
        return False
    matches = [i for i, window in enumerate(windows)
               if window['id'] == held['window'] and window['pid'] == held['pid']]
    if len(matches) != 1:
        return False
    index = matches[0]
    current = windows[index]
    if current['bounds'] != tuple(held['bounds']) or current['name'] != held['name']:
        return False
    if not any(inside(current['bounds'], display) for display in displays):
        return False
    def overlaps(a,b):
        return (max(a[0],b[0]) < min(a[0]+a[2],b[0]+b[2])
                and max(a[1],b[1]) < min(a[1]+a[3],b[1]+b[3]))
    return not any(window['pid'] == held['pid'] or overlaps(window['bounds'],current['bounds'])
                   for window in windows[:index])


def native_adapter(request, deadline):
    import os
    import subprocess
    import runpy
    scope = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
        NANH_CLAUDE_LINUX_SOURCE_POLICY='official-2.9939.4',
        NANH_CLAUDE_LINUX_NATIVE_CHAT='first-turn', NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline')
    if any(os.environ.get(key) != value for key,value in scope.items()):
        raise Rejected('policy')
    executable = Path(request['nativeExecutable'])
    if not executable.is_absolute() or executable.is_symlink() or not executable.is_file():
        raise Rejected('policy')
    if not time.monotonic() < deadline <= time.monotonic() + 15:
        raise Rejected()
    try:
        visibility = load_visibility()
    except Exception:
        raise Rejected('transport') from None
    try:
        adapter = visibility.Adapter(deadline)
    except Exception:
        raise Rejected('source-owner') from None
    try:
        driver = runpy.run_path(str(Path(__file__).with_name('zed-input-x11.py')))
    except Exception:
        raise Rejected('transport') from None
    def run(argv, payload=None, output=False):
        custody = getattr(adapter, 'profile_guard', None)
        if custody is None or not custody():
            raise Rejected('policy')
        remaining = deadline - time.monotonic()
        if remaining <= 0 or os.getppid() != request['checkerPid']:
            raise TimeoutError()
        result = subprocess.run(argv, input=payload, stdout=subprocess.PIPE if output else subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, timeout=remaining, check=True)
        if time.monotonic() >= deadline or os.getppid() != request['checkerPid']:
            raise TimeoutError()
        if output and len(result.stdout) > 131072:
            raise Rejected()
        return result.stdout if output else None
    def guarded():
        custody = getattr(adapter, 'profile_guard', None)
        if custody is None or not custody():
            return False
        text = run([request['nativeExecutable'],'--windows'],output=True).decode('ascii')
        if not snapshot_clear(text, request):
            return False
        active = int(run(['/usr/bin/xdotool','getactivewindow'],output=True).strip())
        if not driver['owned_frame'](active,request['window']):
            return False
        first = driver['independent_client_snapshot'](active)
        second = driver['independent_client_snapshot'](active)
        if first != second or time.monotonic() >= deadline:
            return False
        adapter.client = (*first[0],*first[2])
        return custody()
    def focus(node):
        return bool(adapter.state(node)&(1<<12))
    def text(node):
        if not guarded():
            raise Rejected('native-window')
        def budget():
            adapter.remaining()
            if os.getppid()!=request['checkerPid']:
                raise Rejected('source-owner')
        observation={}
        inventory={}
        capability={}
        adapter.empty_class_witness=None
        adapter.embedded_text_observation=None
        adapter.input_text_inventory=None
        result=flatten_hypertext(node,lambda method,target,*args:hypertext_query(adapter,method,target,*args),request['pid'],budget,observation,
            inventory if request['mode'] in ('input-next-correlated','input-next-empty-class') else None,
            capability if request['mode']=='input-next-empty-class' else None)
        adapter.empty_class_witness=capability.get('witness')
        adapter.input_text_inventory=inventory or None
        adapter.embedded_text_observation=observation or None
        if not guarded():
            raise Rejected('native-window')
        return result
    def clipboard_write(value):
        run(['/usr/bin/xclip','-selection','clipboard'],value.encode())
    def key(value):
        if not adapter.key_guard() or not guarded():
            raise Rejected()
        run(['/usr/bin/xdotool','key','--clearmodifiers',value])
        if not guarded() or not adapter.key_guard():
            raise Rejected()
    def paste(prompt):
        clipboard_write(prompt)
        callback=getattr(adapter,'before_paste',None)
        if callback is None:
            key('ctrl+v')
            return
        if not adapter.key_guard() or not guarded():
            raise Rejected()
        callback()
        adapter.before_paste=None
        run(['/usr/bin/xdotool','key','--clearmodifiers','ctrl+v'])
        if not guarded() or not adapter.key_guard():
            raise Rejected()
    def readback(node):
        import secrets
        clipboard_write(secrets.token_hex(16))
        key('ctrl+a')
        if not focus(node):
            raise Rejected()
        key('ctrl+c')
        value = run(['/usr/bin/xclip','-o','-selection','clipboard'],output=True).decode('utf8')
        if len(value.encode()) > 4096:
            raise Rejected()
        if not focus(node):
            raise Rejected()
        key('Right')
        return value
    def hit(node, frame):
        bounds = adapter.bounds(node)
        found = tuple(str(value) for value in adapter.call(frame,'GetAccessibleAtPoint',
            'org.a11y.atspi.Component',bounds[0]+bounds[2]//2,bounds[1]+bounds[3]//2,
            adapter.dbus.UInt32(0)))
        seen = set()
        for _ in range(6):
            if found in seen or adapter.owner(found) != request['pid']:
                return False
            seen.add(found)
            if found == node:
                return True
            found = adapter.parent(found)
        return False
    adapter.guard = guarded
    adapter.client_bounds = lambda: adapter.client
    adapter.focused = focus
    adapter.text = text
    adapter.attributes = lambda node: dict(adapter.call(node,'GetAttributes'))
    adapter.grab_focus = lambda node: bool(adapter.call(node,'GrabFocus','org.a11y.atspi.Component'))
    adapter.paste_once = paste
    adapter.select_all_once = lambda: key('ctrl+a')
    adapter.copy_input_once = readback
    adapter.actions = lambda node: action_names(adapter, node)
    adapter.hit = hit
    adapter.invoke_once = lambda node,index: bool(adapter.call(node,'DoAction','org.a11y.atspi.Action',index))
    import secrets
    adapter.clipboard_sentinel = lambda: clipboard_write(secrets.token_hex(16))
    adapter.clipboard_read = lambda: run(['/usr/bin/xclip','-o','-selection','clipboard'],output=True).decode('utf8')
    adapter.clear_clipboard = lambda: clipboard_write('')
    return adapter


def main():
    import json
    import sys
    facts = dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
        stage='blocked',inputVerified=False,pasteAttempted=False,sendAttempted=False,
        sendForwarded=False,responseVerified=False,toolVerified=False,recoveryVerified=False)
    binding = None
    controller = None
    custody = None
    boundary = 'request'
    try:
        raw = sys.stdin.buffer.read(32769)
        if len(raw)>32768:
            raise Rejected()
        request = json.loads(raw)
        required = {'pid','bus','path','checkerPid','window','bounds','name','nativeExecutable',
            'deadline','mode','value','binding','profileAuthority','history'}
        if type(request) is not dict or set(request)!=required or request['mode'] not in ('input','input-first-owned','input-next-correlated','input-next-empty-class','copy'):
            raise Rejected()
        if type(request['value']) is not str or len(request['value'].encode())>4096:
            raise Rejected()
        deadline = request['deadline']
        if type(deadline) not in (float,int):
            raise Rejected()
        boundary = 'policy'
        custody = ProfileCustody(request['profileAuthority'], deadline)
        adapter = native_adapter(request,deadline)
        adapter.profile_guard = custody.verify
        controller = Controller(adapter,request,deadline)
        if request['mode'] in ('input-next-correlated','input-next-empty-class'):
            if request['binding'] is None:
                raise Rejected('policy')
            controller.empty_class_opt_in=request['mode']=='input-next-empty-class'
            controller.restore_next_input(request['binding'],request['history'])
        else:
            if request['history'] != []:
                raise Rejected('policy')
            if request['binding'] is not None:
                controller.restore(request['binding'], response=request['mode']=='copy')
        if request['mode'] in ('input','input-first-owned','input-next-correlated','input-next-empty-class'):
            facts = controller.submit(request['value'], request['mode']=='input-first-owned')
        else:
            if request['binding'] is None:
                raise Rejected()
            facts = controller.copy_response(request['value'])
        if controller.editor is not None and controller.frame is not None:
            binding = controller.binding()
    except Exception as error:
        if controller is not None:
            facts = controller.facts
            if facts['stage'] == 'source':
                facts['stage'] = 'blocked'
            controller.failure(error)
        else:
            facts['failureBoundary'] = error.boundary if isinstance(error, Rejected) and error.boundary else boundary
    if custody is not None:
        custody.close()
    # This is private supervisor stdout, not a qualification artifact.
    print(json.dumps(dict(facts=facts,binding=binding),separators=(',',':')))

if __name__=='__main__':
    main()
