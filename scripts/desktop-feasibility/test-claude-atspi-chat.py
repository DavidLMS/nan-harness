import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('controller', Path(__file__).with_name('claude-atspi-chat.py'))
chat = importlib.util.module_from_spec(spec)
spec.loader.exec_module(chat)


class Adapter:
    def __init__(self, **changes):
        self.changes = changes
        self.focus = False
        self.value = changes.get('initial', '')
        self.focus_count = self.paste_count = self.send_count = self.copy_count = 0
        self.cleared = False
        self.now = 0
        self.nodes = ['root','frame','editor','mode','chat','send']
    def owner(self, node):
        return 8 if self.changes.get('foreign') else 7
    def identity(self, node):
        return {'root':(75,'Claude',''),'frame':(23,'Claude',''),
                'editor':(61,'Write your prompt to Claude',''),
                'mode':(39,'Mode',''),'chat':(43,'Chat',''),'send':(43,'Start task','')}[node]
    def children(self, node):
        if node == 'root':return ['frame']
        if node == 'frame':return ['editor','mode','send'] + (['editor'] if self.changes.get('duplicate') else [])
        return ['chat'] if node == 'mode' else []
    def parent(self, node):
        return {'editor':'root' if self.changes.get('detached') else 'frame','frame':'root','send':'frame','chat':'mode','mode':'frame'}[node]
    def state(self, node):
        bits = (1<<30)|(1<<25)|(1<<1)|(1<<7)|(1<<8)|(1<<24)
        return bits & ~(1<<8) if node == 'send' and self.changes.get('disabled') else bits
    def bounds(self,node):
        if node=='frame':return (0,0,800,600)
        return (0,0,900,50) if self.changes.get('outside') and node=='editor' else (10,10,100,40)
    def guard(self):
        return not (self.changes.get('guard_after_paste') and self.paste_count)
    def client_bounds(self):
        return (0,0,801,600) if self.changes.get('moved_client') else (0,0,800,600)
    def focused(self,node):
        return self.focus
    def attributes(self,node):
        return {'current':'page' if not self.changes.get('wrong_mode') else 'false'}
    def text(self,node):
        return self.value
    def grab_focus(self,node):
        self.focus_count += 1
        self.focus = not self.changes.get('pending_focus')
        return True
    def paste_once(self,prompt):
        self.paste_count += 1
        self.value = 'foreign value' if self.changes.get('wrong_value') else prompt
    def copy_input_once(self,node):
        self.copy_count += 1
        return 'wrong' if self.changes.get('clipboard_mismatch') else self.value
    def hit(self,node,frame):return not self.changes.get('foreign_hit')
    def actions(self,node):
        return ['click','click'] if self.changes.get('duplicate_action') else ['click']
    def invoke_once(self,node,index):
        self.send_count += 1
        if self.changes.get('invoke_error'):raise TimeoutError()
        return True
    def clear_clipboard(self):
        self.cleared = True
    def clock(self):return self.now
    def sleep(self,amount):self.now+=amount


class ControllerTests(unittest.TestCase):
    def run_case(self, **options):
        adapter=Adapter(**options)
        controller=chat.Controller(adapter,{'pid':7,'bus':'r','path':'root'},.1,adapter.clock,adapter.sleep)
        # Fixture endpoint is retained and private, no platform APIs.
        controller.root={'pid':7,'bus':'r','path':'root'}
        # Adapt source endpoint pair to our fixture strings at adapter boundary.
        for name in ['owner','identity','children','parent','state','bounds','focused','text','grab_focus','attributes','actions','invoke_once']:
            fn=getattr(adapter,name)
            def wrap(node,*args,fn=fn,name=name):
                value=fn('root' if node==('r','root') else node,*args)
                return ('r','root') if name=='parent' and value=='root' else value
            setattr(adapter,name,wrap)
        facts=controller.submit('private exact prompt')
        self.assertTrue(adapter.cleared)
        return adapter,controller,facts
    def test_one_verified_submission_never_qualifies_partial_controller(self):
        adapter,controller,facts=self.run_case()
        self.assertTrue(facts['inputVerified'])
        self.assertTrue(facts['sendForwarded'])
        self.assertFalse(facts['responseVerified'])
        self.assertEqual(facts['stage'],'sent')
        self.assertEqual((adapter.focus_count,adapter.paste_count,adapter.send_count),(1,1,1))
        controller.submit('another prompt')
        self.assertEqual((adapter.focus_count,adapter.paste_count,adapter.send_count),(1,1,1))
    def test_rejections_do_not_send(self):
        for options in [{'foreign':True},{'duplicate':True},{'detached':True},{'outside':True},
                        {'moved_client':True},{'wrong_mode':True},{'initial':'owned unknown'},
                        {'disabled':True},{'foreign_hit':True},{'duplicate_action':True},{'wrong_value':True},
                        {'clipboard_mismatch':True},{'guard_after_paste':True}]:
            with self.subTest(options=options):
                adapter,_,facts=self.run_case(**options)
                self.assertEqual(adapter.send_count,0)
                self.assertFalse(facts['sendForwarded'])
    def test_focus_deadline_has_no_paste_or_send(self):
        adapter,_,facts=self.run_case(pending_focus=True)
        self.assertEqual(facts['stage'],'deadline')
        self.assertEqual((adapter.focus_count,adapter.paste_count,adapter.send_count),(1,0,0))
    def test_uncertain_dispatch_consumed(self):
        adapter,controller,facts=self.run_case(invoke_error=True)
        self.assertEqual(adapter.send_count,1)
        self.assertTrue(facts['sendAttempted'])
        self.assertFalse(facts['sendForwarded'])
        controller.submit('private exact prompt')
        self.assertEqual(adapter.send_count,1)

class NativeGuardProtocolTests(unittest.TestCase):
    def test_same_snapshot_identity_foreground_and_occlusion(self):
        held=dict(pid=7,window=12,bounds=[0,0,800,600],name='436c61756465')
        good='FG 7 12\nDISPLAY 0 0 1024 768\nWIN 12 7 0 0 800 600 436c61756465\n'
        self.assertTrue(chat.snapshot_clear(good,held))
        self.assertTrue(chat.snapshot_clear(good.replace('800 600','800.000 600.000'),held))
        for bad in (good.replace('FG 7','FG 8'),good.replace('800 600','801 600'),
                    good.replace('436c61756465','foreign'),
                    good.replace('WIN 12','WIN 11'),
                    good.replace('WIN 12','WIN 15 9 100 100 20 20 foreign\nWIN 12'),
                    good.replace('WIN 12','WIN 15 7 900 650 20 20 same\nWIN 12')):
            with self.subTest(bad=bad):self.assertFalse(chat.snapshot_clear(bad,held))

class ResponseAdapter(Adapter):
    def __init__(self,**options):
        super().__init__(**options);self.clipboard='old';self.copy_actions=0
    def identity(self,node):
        if node=='row':return (39,'','')
        if node=='heading':return (83,'Claude responded: private-marker','')
        if node in ('copy','copy2'):return (43,'Copy','')
        if node=='title':return (83,'private-marker','')
        return super().identity(node)
    def children(self,node):
        if node=='frame':return ['editor','mode','send','row']+(['title'] if self.changes.get('title') else [])
        if node=='row':return ['heading','copy']+(['copy2'] if self.changes.get('duplicate_copy') else [])
        return super().children(node)
    def parent(self,node):
        if node in ('heading','copy','copy2'):return 'frame' if self.changes.get('global_copy') else 'row'
        if node in ('row','title'):return 'frame'
        return super().parent(node)
    def clipboard_sentinel(self):self.clipboard='fresh sentinel'
    def clipboard_read(self):return self.clipboard
    def invoke_once(self,node,index):
        if node=='copy':
            self.copy_actions+=1
            self.clipboard='wrong' if self.changes.get('wrong_copy') else 'private-marker'
            return True
        return super().invoke_once(node,index)

class ResponseTests(unittest.TestCase):
    def case(self,**options):
        adapter=ResponseAdapter(**options)
        for name in ['owner','identity','children','parent','state','bounds','focused','text','grab_focus','attributes','actions','invoke_once']:
            fn=getattr(adapter,name)
            def wrap(node,*args,fn=fn,name=name):
                value=fn('root' if node==('r','root') else node,*args)
                return ('r','root') if name=='parent' and value=='root' else value
            setattr(adapter,name,wrap)
        c=chat.Controller(adapter,dict(pid=7,bus='r',path='root'),1,adapter.clock,adapter.sleep)
        c.bind();return adapter,c,c.copy_response('private-marker')
    def test_exact_assistant_scope_copy(self):
        a,c,f=self.case(title=True);self.assertTrue(f['responseVerified']);self.assertEqual(a.copy_actions,1)
    def test_global_or_duplicate_or_mismatch_reject(self):
        for options in [dict(global_copy=True),dict(duplicate_copy=True),dict(wrong_copy=True)]:
            with self.subTest(options=options):
                a,c,f=self.case(**options);self.assertFalse(f['responseVerified']);self.assertLessEqual(a.copy_actions,1)

class BoundaryTests(unittest.TestCase):
    run_case = ControllerTests.run_case
    def test_same_failed_query_boundary_without_extra_actions(self):
        for options,boundary in [({'foreign':True},'source-owner'),({'duplicate':True},'tree'),
                ({'moved_client':True},'client'),({'wrong_mode':True},'mode'),
                ({'initial':'unknown owned text'},'input'),({'guard_after_paste':True},'native-window'),
                ({'invoke_error':True},'action')]:
            with self.subTest(options=options):
                adapter,controller,facts=self.run_case(**options)
                self.assertEqual(facts['failureBoundary'],boundary)
                self.assertLessEqual(adapter.send_count,1)
                self.assertFalse(facts['sendForwarded'])
                self.assertNotIn('unknown owned text',str(facts))
        adapter,controller,facts=self.run_case()
        self.assertNotIn('failureBoundary',facts)
    def test_request_rejection_is_closed_and_action_free(self):
        import io,json
        from contextlib import redirect_stdout
        from unittest.mock import patch
        class Input:
            buffer=io.BytesIO(b'{"PRIVATE":"value"}')
        out=io.StringIO()
        with patch('sys.stdin',Input()),redirect_stdout(out):
            chat.main()
        facts=json.loads(out.getvalue())['facts']
        self.assertEqual(facts['failureBoundary'],'request')
        self.assertNotIn('PRIVATE',out.getvalue())
        self.assertFalse(facts['sendAttempted'])

if __name__=='__main__':unittest.main()
