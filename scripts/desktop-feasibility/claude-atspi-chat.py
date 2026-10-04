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


BOUNDARIES = frozenset(('request','policy','native-window','source-owner','tree','state',
    'frame','client','mode','focus','input','clipboard','action','response','transport'))
QUERY_BOUNDARIES = dict(owner='source-owner', identity='tree', children='tree', parent='frame',
    state='state', bounds='frame', guard='native-window', client_bounds='client',
    attributes='mode', focused='focus', grab_focus='focus', text='input', paste_once='input',
    copy_input_once='clipboard', clear_clipboard='clipboard', clipboard_sentinel='clipboard',
    clipboard_read='clipboard', actions='action', hit='action', invoke_once='action')

class Rejected(Exception):
    def __init__(self, boundary=None):
        self.boundary = boundary if boundary in BOUNDARIES else None
        super().__init__()


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
        result = getattr(self.adapter, method)(*args)
        if self.clock() >= self.deadline:
            raise TimeoutError()
        return result

    def failure(self, error):
        self.facts['failureBoundary'] = (error.boundary if isinstance(error, Rejected)
            and error.boundary is not None else self.boundary)

    def owned(self, node):
        if self.query('owner', node) != self.root['pid']:
            raise Rejected('source-owner')

    def tree(self):
        root = (self.root['bus'], self.root['path'])
        pending, seen, nodes = [(root, 0)], set(), []
        while pending:
            node, depth = pending.pop()
            if node in seen or depth > 32 or len(seen) >= 1024:
                raise Rejected()
            seen.add(node)
            self.owned(node)
            identity = self.query('identity', node)
            if type(identity) is not tuple or len(identity) != 3:
                raise Rejected()
            nodes.append((node, identity))
            children = self.query('children', node)
            if type(children) is not list or len(children) > 1024 - len(seen):
                raise Rejected()
            pending.extend((child, depth + 1) for child in children)
        return nodes

    def state(self, node, editable=False, active=False):
        self.owned(node)
        bits = self.query('state', node)
        if (type(bits) is not int or not 0 <= bits < 2**64 or bits & (1 << 6)
                or not bits & (1 << 30) or not bits & (1 << 25)
                or editable and not bits & (1 << 7) or active and not bits & (1 << 1)):
            raise Rejected('input' if editable else 'frame' if active else 'state')
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
                self.state(node, active=True)
                frames.append(node)
            node = self.query('parent', node)
        else:
            raise Rejected()
        if len(frames) != 1:
            raise Rejected('frame')
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
        self.state(self.editor, editable=True)
        self.state(self.frame, active=True)
        if (self.query('identity', self.editor), self.query('bounds', self.editor)) != self.sealed_editor:
            raise Rejected()
        if (self.query('identity', self.frame), self.query('bounds', self.frame)) != self.sealed_frame:
            raise Rejected()
        client = self.query('client_bounds')
        if self.sealed_frame[1] != client or not inside(self.sealed_editor[1], client):
            raise Rejected('client')
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
        modes = [node for node, identity in nodes if identity[0] in (39, 97)
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

    def submit(self, prompt):
        try:
            if self.focus_attempted or self.paste_attempted or self.send_attempted:
                raise Rejected()
            if type(prompt) is not str or not prompt or len(prompt.encode()) > 4096:
                raise Rejected()
            self.proof() if self.restored else self.bind()
            self.current_chat(self.tree())
            # This initial candidate has NO replacement authority for nonempty input.
            if self.query('text', self.editor) != '':
                self.facts['stage'] = 'input-not-empty'
                self.facts['failureBoundary'] = 'input'
                return self.facts
            self.facts['stage'] = 'focus'
            self.proof()
            self.focus_attempted = True
            if self.query('grab_focus', self.editor) is not True:
                raise Rejected()
            self.settle_focus()
            if self.query('text', self.editor) != '':
                raise Rejected()
            self.facts['stage'] = 'paste'
            self.proof(focused=True)
            if not self.query('focused', self.editor):
                raise Rejected()
            self.paste_attempted = True
            self.facts['pasteAttempted'] = True
            self.query('paste_once', prompt)
            while True:
                self.settle_focus()
                value = self.query('text', self.editor)
                if type(value) is not str or len(value.encode()) > 4096:
                    raise Rejected()
                if value == prompt:
                    break
                if value and not prompt.startswith(value):
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
                if self.query('actions', self.send) != ['click'] or not self.query('hit', self.send, self.frame):
                    raise Rejected()
            self.facts['stage'] = 'send'
            self.send_attempted = True
            self.facts['sendAttempted'] = True
            if self.query('invoke_once', self.send, 0) is not True:
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
        return self.facts


    def binding(self):
        return dict(editor=self.editor,frame=self.frame,
            editorIdentity=self.sealed_editor[0],editorBounds=self.sealed_editor[1],
            frameIdentity=self.sealed_frame[0],frameBounds=self.sealed_frame[1])

    def restore(self, binding):
        if type(binding) is not dict or set(binding) != {'editor','frame','editorIdentity',
                'editorBounds','frameIdentity','frameBounds'}:
            raise Rejected()
        self.editor = tuple(binding['editor'])
        self.frame = tuple(binding['frame'])
        self.sealed_editor = (tuple(binding['editorIdentity']),tuple(binding['editorBounds']))
        self.sealed_frame = (tuple(binding['frameIdentity']),tuple(binding['frameBounds']))
        self.restored = True
        self.adapter.key_guard = lambda: self.proof(focused=True)
        self.proof()

    def response_scope(self, marker):
        nodes = self.tree()
        headings = [node for node,identity in nodes if identity[0] == 83
            and identity[1].startswith('Claude responded: ') and marker in identity[1]]
        if not headings:
            return None
        if len(headings) != 1:
            raise Rejected()
        heading, parent, seen = headings[0], self.query('parent',headings[0]), set()
        for _ in range(6):
            if parent in seen or parent == self.frame:
                raise Rejected()
            seen.add(parent)
            self.owned(parent)
            identity = self.query('identity',parent)
            if identity[0] not in (39,97):
                raise Rejected()
            pending, subtree = [parent], []
            visited = set()
            while pending:
                node = pending.pop()
                if node in visited or len(visited) >= 256:
                    raise Rejected()
                visited.add(node)
                self.owned(node)
                item = self.query('identity',node)
                subtree.append((node,item))
                pending.extend(self.query('children',node))
            scopes = [node for node,item in subtree if item[0] == 83]
            copies = [node for node,item in subtree if item[0] == 43 and item[1] == 'Copy']
            if scopes == [heading] and len(copies) == 1:
                return parent,heading,copies[0]
            parent = self.query('parent',parent)
        raise Rejected()

    def copy_response(self, marker):
        try:
            if self.copy_attempted:
                raise Rejected()
            self.proof()
            self.current_chat(self.tree())
            scope = self.response_scope(marker)
            if scope is None:
                self.facts['stage'] = 'response-pending'
                return self.facts
            row,heading,button = scope
            sealed = (self.query('identity',button),self.query('bounds',button))
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
                if not inside(sealed[1],self.sealed_frame[1]) or self.query('actions',button) != ['click']:
                    raise Rejected()
                if not self.query('hit',button,self.frame):
                    raise Rejected()
            self.query('clipboard_sentinel')
            self.proof()
            if (self.response_scope(marker) != scope
                    or (self.query('identity',button),self.query('bounds',button)) != sealed
                    or self.query('actions',button) != ['click']
                    or not self.query('hit',button,self.frame)):
                raise Rejected()
            self.state(button)
            self.copy_attempted = True
            if self.query('invoke_once',button,0) is not True:
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
        return True
    def focus(node):
        return bool(adapter.state(node)&(1<<12))
    def text(node):
        count = int(adapter.call(node,'Get','org.freedesktop.DBus.Properties',
            'org.a11y.atspi.Text','CharacterCount'))
        if not 0 <= count <= 4096:
            raise Rejected()
        value = str(adapter.call(node,'GetText','org.a11y.atspi.Text',0,count))
        if len(value.encode()) > 4096:
            raise Rejected()
        return value
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
        key('ctrl+v')
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
    adapter.copy_input_once = readback
    def actions(node):
        count = int(adapter.call(node,'Get','org.freedesktop.DBus.Properties',
            'org.a11y.atspi.Action','NActions'))
        if not 0 <= count <= 8:
            raise Rejected()
        return [str(adapter.call(node,'GetName','org.a11y.atspi.Action',i)) for i in range(count)]
    adapter.actions = actions
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
    boundary = 'request'
    try:
        raw = sys.stdin.buffer.read(32769)
        if len(raw)>32768:
            raise Rejected()
        request = json.loads(raw)
        required = {'pid','bus','path','checkerPid','window','bounds','name','nativeExecutable',
            'deadline','mode','value','binding'}
        if type(request) is not dict or set(request)!=required or request['mode'] not in ('input','copy'):
            raise Rejected()
        if type(request['value']) is not str or len(request['value'].encode())>4096:
            raise Rejected()
        deadline = request['deadline']
        if type(deadline) not in (float,int):
            raise Rejected()
        boundary = 'policy'
        adapter = native_adapter(request,deadline)
        controller = Controller(adapter,request,deadline)
        if request['binding'] is not None:
            controller.restore(request['binding'])
        if request['mode']=='input':
            facts = controller.submit(request['value'])
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
    # This is private supervisor stdout, not a qualification artifact.
    print(json.dumps(dict(facts=facts,binding=binding),separators=(',',':')))

if __name__=='__main__':
    main()
