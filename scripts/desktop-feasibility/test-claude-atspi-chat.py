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
        self.frame_reads = 0
        self.nodes = ['root','frame','editor','mode','chat','send']
    def owner(self, node):
        return 8 if self.changes.get('foreign') else 7
    def identity(self, node):
        if node == 'frame':
            self.frame_reads += 1
            if self.changes.get('changed_frame') and self.frame_reads >= 4:
                return (23,'changed held frame','')
        return {'root':(23 if self.changes.get('wrong_root_role') else 75,'Claude',''),'frame':(69 if self.changes.get('window_role') else 23,'Claude',''),
                'editor':(61,'Write your prompt to Claude',''),
                'mode':(39,'Mode',''),'chat':(43,'Chat',''),'send':(43,'Start task','')}[node]
    def children(self, node):
        if node == 'root':return ['frame']
        if node == 'frame':return ['editor','mode','send'] + (['editor'] if self.changes.get('duplicate') else [])
        return ['chat'] if node == 'mode' else []
    def parent(self, node):
        return {'editor':'root' if self.changes.get('detached') else 'frame','frame':'root','send':'frame','chat':'mode','mode':'frame'}[node]
    def state(self, node):
        if node == 'root':return 0
        if node == 'frame' and self.changes.get('hidden_frame'):return 0
        bits = (1<<30)|(1<<25)|(1<<1)|(1<<7)|(1<<8)|(1<<24)
        if node == 'frame' and self.changes.get('inactive_frame'):bits &= ~(1<<1)
        return bits & ~(1<<8) if node == 'send' and self.changes.get('disabled') else bits
    def bounds(self,node):
        if node=='frame':return (0,0,800,600)
        return (0,0,900,50) if self.changes.get('outside') and node=='editor' else (10,10,100,40)
    def guard(self):
        return not self.changes.get('guard_before') and not (self.changes.get('guard_after_paste') and self.paste_count)
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


class InputShapeTests(unittest.TestCase):
    def test_work_cutoff_reserves_receipt_time_inside_original_deadline(self):
        self.assertEqual(chat.work_cutoff(15, 0), 14.5)
        self.assertEqual(chat.work_cutoff(10, 9), 9.5)
        with self.assertRaises(TimeoutError):
            chat.work_cutoff(10, 9.6)
        for deadline in (0, 16):
            with self.assertRaises(chat.Rejected):
                chat.work_cutoff(deadline, 0)

    def test_closed_shapes_and_private_mixed_content(self):
        for value,expected in [ ('\n\r\n',(3,True,True,False)),
                (' \t',(2,False,True,False)), ('\u200b\ufeff',(2,False,False,True)),
                ('PRIVATE nonce\n',(14,False,False,False)), ('\n\u200b',(2,False,False,False)) ]:
            shape=chat.input_shape(value)
            self.assertEqual(tuple(shape[key] for key in ['charCount','onlyLineBreaks','onlyWhitespace','onlyZeroWidthMarkers']),expected)
            self.assertFalse(shape['onlyObjectReplacement'])
            self.assertEqual(set(shape),{'charCount','onlyLineBreaks','onlyWhitespace','onlyZeroWidthMarkers','onlyObjectReplacement'})
            self.assertNotIn(value,str(shape))
        for value in ['\ufffc','\ufffc\ufffc']:
            shape=chat.input_shape(value)
            self.assertTrue(shape['onlyObjectReplacement'])
            self.assertFalse(any(shape[key] for key in ['onlyLineBreaks','onlyWhitespace','onlyZeroWidthMarkers']))
        for value in ['\ufffcx','\ufffc\n','\ufffc\u200b']:
            self.assertFalse(chat.input_shape(value)['onlyObjectReplacement'])
        for value in ['',True,None,'x'*4097]:
            with self.assertRaises(chat.Rejected):chat.input_shape(value)

class HypertextWireTests(unittest.TestCase):
    def test_exact_dbus_methods_properties_and_reference_signatures(self):
        class Adapter:
            dbus=type('Types',(),{'Boolean':bool})
            def __init__(self):self.calls=[];self.value=None
            def call(self,node,method,interface,*args):
                self.calls.append((node,method,interface,args));return self.value
        a=Adapter();node=('owned','/editor');props='org.freedesktop.DBus.Properties'
        methods=[('count',(),1,'Get',props,('org.a11y.atspi.Text','CharacterCount')),
            ('nlinks',(),1,'GetNLinks','org.a11y.atspi.Hypertext',()),
            ('link-index',(2,),0,'GetLinkIndex','org.a11y.atspi.Hypertext',(2,)),
            ('link',(0,),('owned','/link'),'GetLink','org.a11y.atspi.Hypertext',(0,)),
            ('object',(),('owned','/paragraph'),'GetObject','org.a11y.atspi.Hyperlink',(0,)),
            ('valid',(),True,'IsValid','org.a11y.atspi.Hyperlink',()),
            ('anchors',(),1,'Get',props,('org.a11y.atspi.Hyperlink','NAnchors')),
            ('start',(),2,'Get',props,('org.a11y.atspi.Hyperlink','StartIndex')),
            ('end',(),3,'Get',props,('org.a11y.atspi.Hyperlink','EndIndex')),
            ('text',(3,),'abc','GetText','org.a11y.atspi.Text',(0,3))]
        for logical,args,value,method,interface,wire_args in methods:
            a.value=value
            self.assertEqual(chat.hypertext_query(a,logical,node,*args),value)
            self.assertEqual(a.calls[-1],(node,method,interface,wire_args))
        for logical,args,invalids in [('link',(0,),['/path',None,('owned',3),('owned',)]),
                ('object',(),['/path',None]),('count',(),['0',True,None]),
                ('valid',(),['false',0,None]),('text',(1,),[None,1,['x']])]:
            for value in invalids:
                a.value=value
                with self.assertRaises(chat.Rejected):chat.hypertext_query(a,logical,node,*args)


class EmbeddedAttributeWireTests(unittest.TestCase):
    def test_attributes_preserve_bounded_private_values_without_coercion(self):
        class Dbus:
            Boolean=bool
        class Adapter:
            dbus=Dbus
            def call(self,*args):return self.value
        adapter=Adapter();node=('owned','/leaf')
        adapter.value={'tag':'br','class':'ProseMirror-trailingBreak'}
        self.assertEqual(chat.hypertext_query(adapter,'attributes',node),adapter.value)
        for value in [None,[],{'tag':None},{'tag':True},{'tag':'x'*1025},
                      {str(i):'x' for i in range(65)},{str(i):'x'*1024 for i in range(5)}]:
            adapter.value=value
            with self.assertRaises(chat.Rejected):chat.hypertext_query(adapter,'attributes',node)


class HypertextTests(unittest.TestCase):
    def fixture(self, value='\ufffc', child=''):
        root,leaf,link=('owned','/editor'),('owned','/paragraph'),('owned','/link')
        records={('owner',root):7,('owner',leaf):7,('owner',link):7,
            ('state',root):0,('state',leaf):0,('children',root):[leaf],('children',leaf):[],
            ('parent',leaf):root,('count',root):len(value),('text',root,len(value)):value,
            ('count',leaf):len(child),('text',leaf,len(child)):child,('nlinks',root):1,
            ('link-index',root,value.index('\ufffc')):0,('link',root,0):link,
            ('valid',link):True,('anchors',link):1,('start',link):value.index('\ufffc'),
            ('end',link):value.index('\ufffc')+1,('object',link):leaf}
        return root,leaf,link,records

    def test_owned_empty_and_exact_payload_are_resolved_without_trimming(self):
        for value,child,expected in [('\ufffc','',''),('a\ufffc\n',' private \n','a private \n\n'),
                ('😀\ufffc','nonce','😀nonce')]:
            root,leaf,link,records=self.fixture(value,child)
            query=lambda method,node,*args:records[(method,node,*args)]
            self.assertEqual(chat.flatten_hypertext(root,query,7,lambda:None),expected)

    def test_embedded_lf_shape_is_advisory_and_retains_literal_content(self):
        root,paragraph,link,records=self.fixture(child='\n')
        leaf=('owned','/break')
        records.update({('children',paragraph):[leaf],('parent',leaf):paragraph,
            ('owner',leaf):7,('state',leaf):0,('children',leaf):[],
            ('count',leaf):1,('text',leaf,1):'\n',
            ('role',root):61,('role',paragraph):73,('role',leaf):116,
            ('attributes',root):{'tag':'div'},('attributes',paragraph):{'tag':'p'},
            ('attributes',leaf):{'tag':'br','class':'ProseMirror-trailingBreak'}})
        for attributes,expected in [({'tag':'br','class':'ProseMirror-trailingBreak'},1),
                ({'tag':'br','class':'ProseMirror-trailingBreak extra'},0),
                ({'tag':'span','class':'ProseMirror-trailingBreak'},0),
                ({'tag':'br','class':'ProseMirror-trailingBreak ProseMirror-trailingBreak'},0)]:
            records[('attributes',leaf)]=attributes
            observation={}
            result=chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,lambda:None,observation)
            self.assertEqual(result,'\n')
            self.assertEqual(observation,dict(nodeCount=3,paragraphCount=1,literalLfLeafCount=1,
                brLfLeafCount=int(attributes['tag']=='br'),exactFillerLfLeafCount=expected))
        records[('attributes',leaf)]={'tag':'br','class':'ProseMirror-trailingBreak'}
        reads=0
        def changing(method,node,*args):
            nonlocal reads
            value=records[(method,node,*args)]
            if method=='attributes' and node==leaf:
                reads+=1
                if reads>1:return {'tag':'br','class':'changed'}
            return value
        observation={}
        with self.assertRaises(chat.Rejected):chat.flatten_hypertext(root,changing,7,lambda:None,observation)
        self.assertEqual(observation,{})
        for invalid in ({'tag':None},{'tag':'br','class':'x'*1025}):
            records[('attributes',leaf)]=invalid
            with self.assertRaises(chat.Rejected):
                chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,lambda:None,{})

    def test_foreign_detached_cycle_missing_interface_and_changed_tree_fail(self):
        for method,change in [('owner',8),('state',1<<6),('parent',('owned','/foreign')),
                             ('count',-1)]:
            root,leaf,link,records=self.fixture()
            records[(method,leaf)]=change
            with self.assertRaises(chat.Rejected):
                chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,lambda:None)
        root,leaf,link,records=self.fixture()
        for key,change in [(('object',link),root),(('children',leaf),[root]),
                (('anchors',link),True),(('valid',link),False),(('nlinks',root),2),
                (('object',link),('foreign','/paragraph'))]:
            altered={**records,key:change}
            with self.assertRaises(chat.Rejected):
                chat.flatten_hypertext(root,lambda m,n,*a:altered[(m,n,*a)],7,lambda:None)
        missing=dict(records);missing.pop(('count',leaf))
        with self.assertRaises(KeyError):
            chat.flatten_hypertext(root,lambda m,n,*a:missing[(m,n,*a)],7,lambda:None)
        reads={}
        def changing(method,node,*args):
            key=(method,node,*args);reads[key]=reads.get(key,0)+1
            return 8 if key==('owner',leaf) and reads[key]>1 else records[key]
        with self.assertRaises(chat.Rejected):chat.flatten_hypertext(root,changing,7,lambda:None)

    def test_deadline_oversize_and_duplicate_embedded_mapping_fail_without_actions(self):
        root,leaf,link,records=self.fixture(child='x'*4097)
        with self.assertRaises(chat.Rejected):
            chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,lambda:None)
        def expired():raise TimeoutError()
        with self.assertRaises(TimeoutError):
            chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,expired)
        root,leaf,link,records=self.fixture('\ufffc\ufffc')
        records[('nlinks',root)]=2;records[('link-index',root,1)]=0
        with self.assertRaises(chat.Rejected):
            chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,lambda:None)


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
    def test_nonempty_shape_is_same_read_and_never_authorizes_input(self):
        for initial in ['\n',' \t','\u200b','PRIVATE nonce']:
            adapter, controller, facts = self.run_case(initial=initial)
            self.assertEqual(facts['stage'],'input-not-empty')
            self.assertEqual(facts['inputShape'],chat.input_shape(initial))
            self.assertEqual(adapter.focus_count,0)
            self.assertEqual(adapter.paste_count,0)
            self.assertEqual(adapter.send_count,0)

    def test_native_frame_authority_does_not_require_duplicate_active_state(self):
        adapter, controller, facts = self.run_case(inactive_frame=True)
        self.assertTrue(facts['inputVerified'])
        self.assertTrue(facts['sendForwarded'])
        self.assertEqual(adapter.focus_count,1)
        self.assertEqual(adapter.paste_count,1)
        self.assertEqual(adapter.send_count,1)

    def test_inactive_frame_still_requires_every_independent_proof(self):
        for changed in [{'guard_before':True}, {'foreign':True}, {'window_role':True},
                        {'hidden_frame':True}, {'moved_client':True}, {'outside':True},
                        {'detached':True}, {'changed_frame':True}, {'wrong_root_role':True}]:
            with self.subTest(changed=changed):
                adapter, controller, facts = self.run_case(inactive_frame=True, **changed)
                self.assertFalse(facts['inputVerified'])
                self.assertFalse(facts['sendForwarded'])
                self.assertEqual(adapter.focus_count,0)
                self.assertEqual(adapter.paste_count,0)
                self.assertEqual(adapter.send_count,0)

    def test_frame_rejections_are_precise_without_action(self):
        for options, boundary in [({'guard_before':True},'native-window'),
                                  ({'window_role':True},'frame-count'),
                                  ({'moved_client':True},'frame-client')]:
            adapter, controller, facts = self.run_case(**options)
            self.assertEqual(facts['failureBoundary'],boundary)
            self.assertEqual(adapter.focus_count,0)
            self.assertEqual(adapter.paste_count,0)
            self.assertEqual(adapter.send_count,0)

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
        for options in [{'foreign':True},{'wrong_root_role':True},{'hidden_frame':True},{'duplicate':True},{'detached':True},{'outside':True},
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
        if node=='wrapper':return (80,'PRIVATE wrapper','')
        if node=='row':return (97 if self.changes.get('row_wrapper') else self.changes.get('row_role',39),'','')
        if node=='heading':return (83,'Claude responded: private-marker','')
        if node in ('copy','copy2'):return (43,'Copy','')
        if node=='title':return (83,'private-marker','')
        return super().identity(node)
    def children(self,node):
        if node=='frame':return ['editor','mode','send','row']+(['title'] if self.changes.get('title') else [])
        if node=='wrapper':return ['heading']
        if node=='row':return ['wrapper' if self.changes.get('wrapper') else 'heading']+([] if self.changes.get('absent_copy') else ['copy'])+(['copy2'] if self.changes.get('duplicate_copy') else [])
        return super().children(node)
    def parent(self,node):
        if node=='wrapper':return 'wrapper' if self.changes.get('wrapper_cycle') else 'frame' if self.changes.get('wrapper_global') else 'row'
        if node=='heading' and self.changes.get('wrapper'):return 'wrapper'
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

class TreeRejectionTests(unittest.TestCase):
    def test_closed_rejection_does_not_export_native_identity(self):
        for kind,expected in [('cycle','tree-cycle'),('depth','tree-depth'),
                ('limit','tree-limit'),('identity','tree-identity'),('children','tree-children')]:
            class TreeAdapter:
                def owner(self,node):return 7
                def identity(self,node):return [] if kind=='identity' else (39,'PRIVATE','')
                def children(self,node):
                    if kind=='cycle':return [node]
                    if kind=='children':return 'PRIVATE'
                    if kind=='limit':
                        if node==0:return list(range(1,1024))
                        return list(range(1024,2046)) if node==1023 else []
                    return [node+1]
            controller=chat.Controller(TreeAdapter(),dict(pid=7,bus='r',path='root'),1,clock=lambda:0)
            with self.assertRaises(chat.Rejected) as rejected:controller.tree(0)
            self.assertEqual(rejected.exception.boundary,expected)
            self.assertNotIn('PRIVATE',str(rejected.exception))

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
    def test_owned_wrapper_is_traversed_without_becoming_row_authority(self):
        adapter,controller,facts=self.case(wrapper=True)
        self.assertTrue(facts['responseVerified']);self.assertEqual(adapter.copy_actions,1)
        self.assertNotIn('PRIVATE',str(facts))
        for options in [dict(wrapper=True,wrapper_cycle=True),dict(wrapper=True,wrapper_global=True),
                dict(wrapper=True,duplicate_copy=True),dict(wrapper=True,global_copy=True)]:
            adapter,controller,facts=self.case(**options)
            self.assertFalse(facts['responseVerified']);self.assertEqual(adapter.copy_actions,0)
    def test_response_row_failures_identify_closed_structural_causes(self):
        for options,reason in [(dict(absent_copy=True),'response-row-copy-absent'),
                (dict(duplicate_copy=True),'response-row-copy-ambiguous'),
                (dict(row_wrapper=True),'response-row-role-limit'),
                (dict(wrapper=True,global_copy=True),'response-row-attachment')]:
            adapter,controller,facts=self.case(**options)
            self.assertEqual(facts['failureBoundary'],reason)
            self.assertEqual(adapter.copy_actions,0)
    def test_atspi_section_and_grouping_are_response_containers(self):
        for role in (39,85,99):
            adapter,controller,facts=self.case(row_role=role)
            self.assertTrue(facts['responseVerified']);self.assertEqual(adapter.copy_actions,1)
        adapter,controller,facts=self.case(row_role=97)
        self.assertFalse(facts['responseVerified']);self.assertEqual(adapter.copy_actions,0)
    def test_global_or_duplicate_or_mismatch_reject(self):
        for options in [dict(global_copy=True),dict(duplicate_copy=True),dict(wrong_copy=True)]:
            with self.subTest(options=options):
                a,c,f=self.case(**options);self.assertFalse(f['responseVerified']);self.assertLessEqual(a.copy_actions,1)

class BoundaryTests(unittest.TestCase):
    run_case = ControllerTests.run_case
    def test_same_failed_query_boundary_without_extra_actions(self):
        for options,boundary in [({'foreign':True},'source-owner'),({'duplicate':True},'tree-cycle'),
                ({'moved_client':True},'frame-client'),({'wrong_mode':True},'mode'),
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



class MountReadinessTests(unittest.TestCase):
    def case(self, *, mounts=True, loses_guard=False, duplicate=False, malformed=False):
        adapter = Adapter()
        adapter.scans = 0
        children = adapter.children
        def ready_children(node):
            if node == 'root':
                adapter.scans += 1
            if node == 'frame':
                if malformed:
                    return None
                if adapter.scans < 3 or not mounts:
                    return ['mode', 'send']
                if duplicate:
                    return ['editor', 'editor', 'mode', 'send']
            return children(node)
        adapter.children = ready_children
        adapter.guard = lambda: not (loses_guard and adapter.scans >= 1)
        for name in ['owner','identity','children','parent','state','bounds','focused','text','grab_focus','attributes','actions','invoke_once']:
            fn = getattr(adapter, name)
            def wrap(node, *args, fn=fn, name=name):
                value = fn('root' if node == ('r','root') else node, *args)
                return ('r','root') if name == 'parent' and value == 'root' else value
            setattr(adapter, name, wrap)
        controller = chat.Controller(adapter, {'pid':7,'bus':'r','path':'root'}, .2,
                                     adapter.clock, adapter.sleep)
        return adapter, controller

    def test_mounting_owned_tree_can_bind_without_input_or_deadline_reset(self):
        adapter, controller = self.case()
        controller.bind()
        self.assertEqual(controller.editor, 'editor')
        self.assertEqual(adapter.scans, 3)
        self.assertEqual(adapter.now, .1)
        self.assertEqual((adapter.focus_count, adapter.paste_count, adapter.send_count), (0,0,0))

    def test_loss_expiry_duplicate_or_malformed_tree_never_act(self):
        for options, error in [({'loses_guard':True}, chat.Rejected),
                               ({'mounts':False}, TimeoutError),
                               ({'duplicate':True}, chat.Rejected),
                               ({'malformed':True}, chat.Rejected)]:
            with self.subTest(options=options):
                adapter, controller = self.case(**options)
                with self.assertRaises(error): controller.bind()
                self.assertLessEqual(adapter.now, controller.deadline)
                self.assertEqual((adapter.focus_count, adapter.paste_count, adapter.send_count), (0,0,0))

class FreshProfileTests(unittest.TestCase):
    def controller(self, adapter):
        for name in ['owner','identity','children','parent','state','bounds','focused','text','grab_focus','attributes','actions','invoke_once']:
            fn=getattr(adapter,name)
            def wrap(node,*args,fn=fn,name=name):
                result=fn('root' if node == ('r','root') else node,*args)
                return ('r','root') if name=='parent' and result=='root' else result
            setattr(adapter,name,wrap)
        return chat.Controller(adapter,dict(pid=7,bus='r',path='root'),1,adapter.clock,adapter.sleep)

    def test_private_retained_roots_replacement_permissions_and_expiry(self):
        import os, tempfile
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary).resolve()
            paths=[root,root/'profile',root/'profile/home',root/'profile/config',
                root/'profile/nanh',root/'profile/config/Claude',root/'profile/config/Claude-3p']
            records=[]
            for path in paths:
                path.mkdir(mode=0o700,exist_ok=True);path.chmod(0o700)
                m=path.stat();records.append(dict(path=str(path),device=m.st_dev,inode=m.st_ino,uid=m.st_uid))
            clock=[0];custody=chat.ProfileCustody(records,1,lambda:clock[0])
            self.assertTrue(custody.verify())
            old=paths[-1].with_name('retained-old');paths[-1].rename(old);paths[-1].mkdir(mode=0o700)
            self.assertFalse(custody.verify())
            paths[-1].rmdir();old.rename(paths[-1]);self.assertTrue(custody.verify())
            paths[3].chmod(0o755);self.assertFalse(custody.verify());paths[3].chmod(0o700)
            clock[0]=1;self.assertFalse(custody.verify());custody.close();self.assertEqual(custody.handles,[])
            with self.assertRaises(chat.Rejected):chat.ProfileCustody(records,1,lambda:1)
    def test_one_owned_replacement_exact_verification_and_no_replay(self):
        adapter=Adapter(initial='\n');adapter.profile_guard=lambda:True
        adapter.select_count=0
        def select():adapter.select_count+=1
        adapter.select_all_once=select
        c=self.controller(adapter)
        facts=c.submit('private-prompt',True)
        self.assertTrue(facts['inputVerified']);self.assertTrue(facts['sendForwarded'])
        self.assertEqual((adapter.select_count,adapter.paste_count,adapter.send_count),(1,1,1))
        c.submit('second',True)
        self.assertEqual((adapter.select_count,adapter.paste_count,adapter.send_count),(1,1,1))
    def test_missing_authority_and_loss_after_selection_never_paste(self):
        for missing in (True,False):
            adapter=Adapter(initial='private disposable draft');adapter.select_count=0
            if not missing:
                adapter.profile_guard=lambda:adapter.select_count==0
            def select():adapter.select_count+=1
            adapter.select_all_once=select
            c=self.controller(adapter)
            c.submit('private-prompt',True)
            self.assertEqual((adapter.paste_count,adapter.send_count),(0,0))
    def test_owned_replacement_never_trims_prompt_lf(self):
        adapter=Adapter(initial='\n');adapter.profile_guard=lambda:True;adapter.select_all_once=lambda:None
        def paste(prompt):adapter.paste_count+=1;adapter.value=prompt+'\n'
        adapter.paste_once=paste
        c=self.controller(adapter)
        facts=c.submit('private-prompt',True)
        self.assertFalse(facts['inputVerified']);self.assertEqual(adapter.send_count,0)


    def test_prior_source_turn_forbids_replacement_before_any_focus(self):
        adapter=Adapter(initial='owned');adapter.profile_guard=lambda:True;adapter.select_all_once=lambda:None
        children,identity=adapter.children,adapter.identity
        adapter.children=lambda node:children(node)+(['prior'] if node=='frame' else [])
        adapter.identity=lambda node:(83,'You said: prior private turn','') if node=='prior' else identity(node)
        c=self.controller(adapter);c.submit('private-prompt',True)
        self.assertEqual((adapter.focus_count,adapter.paste_count,adapter.send_count),(0,0,0))




class SendActionBoundaryTests(unittest.TestCase):
    run_case = ControllerTests.run_case
    def test_advertised_press_and_missing_hit_never_dispatch(self):
        for method,value,boundary in [('actions',['unknown-private'],'action-name'),('actions',['click','click'],'action-name'),('hit',False,'action-hit')]:
            original=getattr(Adapter,method)
            try:
                setattr(Adapter,method,lambda self,*args: value)
                adapter,controller,facts=self.run_case()
                self.assertTrue(facts['inputVerified'])
                self.assertEqual(facts['failureBoundary'],boundary)
                self.assertEqual(adapter.send_count,0)
                self.assertFalse(facts['sendAttempted'])
            finally:
                setattr(Adapter,method,original)

    def test_exact_press_uses_single_same_action(self):
        original=Adapter.actions
        try:
            Adapter.actions=lambda self,node: ['press']
            adapter,controller,facts=self.run_case()
            self.assertTrue(facts['sendForwarded'])
            self.assertEqual(facts['sendActionClass'],'press')
            self.assertEqual(adapter.send_count,1)
        finally:
            Adapter.actions=original

    def test_press_uncertain_never_replays(self):
        original=Adapter.actions
        try:
            Adapter.actions=lambda self,node: ['press']
            adapter,controller,facts=self.run_case(invoke_error=True)
            self.assertEqual(adapter.send_count,1)
            self.assertTrue(facts['sendAttempted'])
            self.assertFalse(facts['sendForwarded'])
            self.assertEqual(facts['stage'],'deadline')
        finally:
            Adapter.actions=original

class ActionProtocolTests(unittest.TestCase):
    def test_exact_wire_calls_and_typed_values(self):
        class Wire:
            def __init__(self,count,names):self.count=count;self.names=names;self.calls=[]
            def call(self,node,method,interface,*args):
                self.calls.append((node,method,interface,args))
                if method=='Get':return self.count
                return self.names[args[0]]
        for names in (['click'],['press'],['private-unknown'],['click','press']):
            wire=Wire(len(names),names)
            self.assertEqual(chat.action_names(wire,'held'),names)
            self.assertEqual(wire.calls[0],('held','Get','org.freedesktop.DBus.Properties',('org.a11y.atspi.Action','NActions')))
            for index,call in enumerate(wire.calls[1:]):
                self.assertEqual(call,('held','GetName','org.a11y.atspi.Action',(index,)))
        Boolean=type('Boolean',(int,),{})
        for count,names,boundary in [(True,[],'action-count'),(Boolean(1),['click'],'action-count'),
                (1.0,['click'],'action-count'),(None,[],'action-count'),(9,[],'action-count'),
                (1,[object()],'action-name'),(1,[True],'action-name'),(1,['x'*65],'action-name')]:
            wire=Wire(count,names)
            with self.assertRaises(chat.Rejected) as error:chat.action_names(wire,'held')
            self.assertEqual(error.exception.boundary,boundary)
            self.assertLessEqual(len(wire.calls),2)

    def test_action_class_drift_is_not_dispatched(self):
        original=Adapter.actions
        try:
            calls=[]
            def actions(self,node):
                calls.append(node);return ['click'] if len(calls)==1 else ['press']
            Adapter.actions=actions
            adapter,controller,facts=ControllerTests.run_case(self)
            self.assertEqual(facts['failureBoundary'],'action-name')
            self.assertEqual(adapter.send_count,0)
            self.assertFalse(facts['sendAttempted'])
        finally:Adapter.actions=original

class MultiActionTests(unittest.TestCase):
    def test_unique_activation_at_advertised_index_and_uncertainty(self):
        original_actions=Adapter.actions;original_invoke=Adapter.invoke_once
        try:
            for names in (['press','focus','scrollToMakeVisible'],['focus','click','showContextMenu']):
                invoked=[]
                Adapter.actions=lambda self,node: names
                def invoke(self,node,index):invoked.append(index);return original_invoke(self,node,index)
                Adapter.invoke_once=invoke
                adapter,controller,facts=ControllerTests.run_case(self)
                self.assertTrue(facts['sendForwarded']);self.assertEqual(adapter.send_count,1)
                index=names.index('press' if 'press' in names else 'click')
                self.assertEqual(invoked,[index]);self.assertEqual(facts['sendActionClass'],'multiple')
                self.assertEqual(facts['sendActionObservation']['selectedIndex'],index)
                adapter,controller,facts=ControllerTests.run_case(self,invoke_error=True)
                self.assertEqual(adapter.send_count,1);self.assertFalse(facts['sendForwarded'])
        finally:Adapter.actions=original_actions;Adapter.invoke_once=original_invoke

    def test_ambiguous_absent_and_drifting_full_list_no_action(self):
        original=Adapter.actions
        try:
            for first,second in [(['click','press'],None),(['press','press'],None),
                    (['focus'],None),(['press','focus'],['press','scrollToMakeVisible'])]:
                calls=[]
                def actions(self,node):calls.append(node);return second if second and len(calls)>1 else first
                Adapter.actions=actions
                adapter,controller,facts=ControllerTests.run_case(self)
                self.assertEqual(adapter.send_count,0);self.assertFalse(facts['sendAttempted'])
                self.assertEqual(facts['failureBoundary'],'action-name')
        finally:Adapter.actions=original

class CopyMultiActionTests(unittest.TestCase):
    case=ResponseTests.case
    def test_copy_unique_activation_and_no_replay(self):
        original_actions=ResponseAdapter.actions;original_invoke=ResponseAdapter.invoke_once
        try:
            for names in (['press','focus'],['focus','click','showContextMenu']):
                invoked=[]
                ResponseAdapter.actions=lambda self,node:names
                def invoke(self,node,index):
                    invoked.append(index);return original_invoke(self,node,index)
                ResponseAdapter.invoke_once=invoke
                adapter,controller,facts=self.case()
                self.assertTrue(facts['responseVerified']);self.assertEqual(adapter.copy_actions,1)
                self.assertEqual(invoked,[names.index('press' if 'press' in names else 'click')])
            def uncertain(self,node,index):self.copy_actions+=1;raise TimeoutError()
            ResponseAdapter.invoke_once=uncertain
            adapter,controller,facts=self.case()
            self.assertEqual(adapter.copy_actions,1);self.assertFalse(facts['responseVerified'])
        finally:ResponseAdapter.actions=original_actions;ResponseAdapter.invoke_once=original_invoke

    def test_copy_full_list_drift_or_ambiguity_has_zero_actions(self):
        original=ResponseAdapter.actions
        try:
            for first,third in [(['click','press'],None),(['press','press'],None),(['focus'],None),
                    (['press','focus'],['press','showContextMenu'])]:
                calls=[]
                def actions(self,node):calls.append(node);return third if third and len(calls)>=3 else first
                ResponseAdapter.actions=actions
                adapter,controller,facts=self.case()
                self.assertEqual(adapter.copy_actions,0);self.assertFalse(facts['responseVerified'])
        finally:ResponseAdapter.actions=original

    def test_copy_disabled_or_covered_has_zero_actions(self):
        original=ResponseAdapter.state
        try:
            ResponseAdapter.state=lambda self,node: original(self,node)&~(1<<8) if node=='copy' else original(self,node)
            adapter,controller,facts=self.case()
            self.assertEqual(adapter.copy_actions,0);self.assertFalse(facts['responseVerified'])
        finally:ResponseAdapter.state=original
        adapter,controller,facts=self.case(foreign_hit=True)
        self.assertEqual(adapter.copy_actions,0);self.assertFalse(facts['responseVerified'])

class NativeAdapterWiringTests(unittest.TestCase):
    def test_reserved_interval_only_clears_private_clipboard_under_original_custody(self):
        import os
        from types import SimpleNamespace
        from unittest.mock import patch
        now=[100.0];calls=[]
        adapter=SimpleNamespace()
        scope=dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='Linux',
            NANH_CLAUDE_LINUX_SOURCE_POLICY='official-2.9939.4',
            NANH_CLAUDE_LINUX_NATIVE_CHAT='first-turn',NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline')
        request=dict(pid=7,checkerPid=9,nativeExecutable=str(Path(__file__).resolve()),deadline=110.5)
        def run(argv,**kwargs):
            calls.append((argv,kwargs));return SimpleNamespace(stdout=b'')
        with patch.dict(os.environ,scope),patch.object(chat,'load_visibility',
                return_value=SimpleNamespace(Adapter=lambda deadline:adapter)),patch('runpy.run_path',return_value={}),                patch.object(chat.time,'monotonic',side_effect=lambda:now[0]),patch('os.getppid',return_value=9),                patch('subprocess.run',side_effect=run):
            native=chat.native_adapter(request,110,cleanup_deadline=110.5)
            native.profile_guard=lambda:True
            now[0]=110.1
            with self.assertRaises(TimeoutError):native.clipboard_sentinel()
            self.assertEqual(calls,[])
            native.clear_clipboard()
            self.assertEqual(calls[0][0],['/usr/bin/xclip','-selection','clipboard'])
            self.assertEqual(calls[0][1]['input'],b'')
            self.assertAlmostEqual(calls[0][1]['timeout'],.4)
            native.profile_guard=lambda:False
            with self.assertRaises(chat.Rejected):native.clear_clipboard()
            self.assertEqual(len(calls),1)
            native.profile_guard=lambda:True;now[0]=110.5
            with self.assertRaises(TimeoutError):native.clear_clipboard()
            self.assertEqual(len(calls),1)

    def test_real_adapter_wires_owned_hit_check_without_native_execution(self):
        import os
        from types import SimpleNamespace
        from unittest.mock import patch
        button=(':1.7','/button');child=(':1.7','/child');frame=(':1.7','/frame')
        for found,foreign,expected in ((button,False,True),(child,False,True),
                                      (child,True,False),(frame,False,False)):
            calls=[]
            adapter=SimpleNamespace(dbus=SimpleNamespace(UInt32=int),
                bounds=lambda node:(10,20,30,40),owner=lambda node:8 if foreign else 7,
                parent=lambda node:button if node==child else node)
            def call(node,method,interface,*args):
                calls.append((node,method,interface,args));return found
            adapter.call=call
            scope=dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='Linux',
                NANH_CLAUDE_LINUX_SOURCE_POLICY='official-2.9939.4',
                NANH_CLAUDE_LINUX_NATIVE_CHAT='first-turn',NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline')
            with patch.dict(os.environ,scope),patch.object(chat,'load_visibility',
                    return_value=SimpleNamespace(Adapter=lambda deadline:adapter)),\
                    patch('runpy.run_path',return_value={}):
                deadline=chat.time.monotonic()+10
                native=chat.native_adapter(dict(pid=7,nativeExecutable=str(Path(__file__).resolve()),deadline=deadline),
                    deadline)
            self.assertEqual(native.hit(button,frame),expected)
            self.assertEqual(calls,[(frame,'GetAccessibleAtPoint','org.a11y.atspi.Component',(25,40,0))])

class ResponseFrameRestoreTests(unittest.TestCase):
    def restored(self, **options):
        adapter,old,_=ResponseTests.case(self)
        binding=old.binding();binding['editor']=['r','editor'];binding['frame']=['r','frame'];adapter.copy_actions=0
        new_editor='editor2' if options.get('replaced') else 'editor'
        for method in ('owner','identity','children','parent','state','bounds'):
            original=getattr(adapter,method)
            def call(node,*args,method=method,original=original):
                node=node[1] if isinstance(node,tuple) else node
                if options.get('replaced') and node=='editor':
                    raise AssertionError('old detached editor was queried')
                target='editor' if node=='editor2' else node
                value=original(target,*args)
                if method=='children' and node=='frame':
                    value=[new_editor if item=='editor' else item for item in value]
                    if options.get('duplicate'):value.append(new_editor)
                    if options.get('absent'):value=[item for item in value if item!=new_editor]
                if method=='parent' and node=='frame' and options.get('detached'):return 'foreign-parent'
                if method=='owner' and node==new_editor and options.get('foreign'):return 8
                if method=='state' and node==new_editor and options.get('hidden'):return 0
                if method=='bounds' and node==new_editor:return (20,20,300,80) if not options.get('off_frame') else (900,20,30,30)
                if method=='bounds' and node=='frame' and options.get('moved_frame'):return (1,0,800,600)
                if method=='children':return [('r',item) for item in value]
                if method=='parent':return value if isinstance(value,tuple) else ('r',value)
                return value
            setattr(adapter,method,call)
        for method in ('attributes','focused','actions','invoke_once','hit'):
            original=getattr(adapter,method)
            def call(node,*args,original=original):
                return original(node[1] if isinstance(node,tuple) else node,*args)
            setattr(adapter,method,call)
        if options.get('guard_loss'):adapter.guard=lambda:False
        controller=chat.Controller(adapter,dict(pid=7,bus='r',path='root'),1,adapter.clock,adapter.sleep)
        return adapter,controller,binding

    def test_moved_or_remounted_editor_only_refreshes_response_scope(self):
        for options in ({},{'replaced':True},{'absent':True},{'hidden':True},{'off_frame':True}):
            adapter,controller,binding=self.restored(**options)
            controller.restore(binding,response=True)
            facts=controller.copy_response('private-marker')
            self.assertTrue(facts['responseVerified']);self.assertEqual(adapter.copy_actions,1)
            self.assertEqual(adapter.paste_count,0);self.assertEqual(adapter.focus_count,0)
            self.assertEqual(controller.binding(),dict(binding,editor=tuple(binding["editor"]),frame=tuple(binding["frame"])))
            with self.assertRaises(chat.Rejected):adapter.key_guard()

    def test_original_frame_and_source_negatives_never_copy(self):
        for options in ({'duplicate':True},{'foreign':True},{'moved_frame':True},{'detached':True},{'guard_loss':True}):
            adapter,controller,binding=self.restored(**options)
            with self.assertRaises(Exception):controller.restore(binding,response=True)
            self.assertEqual(adapter.copy_actions,0);self.assertEqual(adapter.paste_count,0)
        adapter,controller,binding=self.restored(replaced=True)
        adapter.now=2
        with self.assertRaises(TimeoutError):controller.restore(binding,response=True)
        self.assertEqual(adapter.copy_actions,0)

    def test_input_restore_still_rejects_old_editor_movement(self):
        adapter,controller,binding=self.restored()
        with self.assertRaises(chat.Rejected):controller.restore(binding)
        self.assertEqual(adapter.paste_count,0);self.assertEqual(adapter.copy_actions,0)

    def test_response_capability_cannot_focus_or_submit(self):
        adapter,controller,binding=self.restored(absent=True)
        controller.restore(binding,response=True)
        facts=controller.submit('private prompt',replace_owned=True)
        self.assertEqual(facts['failureBoundary'],'policy')
        self.assertFalse(facts['pasteAttempted']);self.assertFalse(facts['sendAttempted'])
        self.assertEqual(adapter.focus_count,0);self.assertEqual(adapter.paste_count,0)

    def test_response_rejects_nested_owned_frame_before_copy(self):
        adapter,controller,binding=self.restored()
        children=adapter.children;identity=adapter.identity;owner=adapter.owner
        adapter.children=lambda node:children(node)+[('r','nested')] if node==('r','frame') else [] if node==('r','nested') else children(node)
        adapter.identity=lambda node:(69,'PRIVATE','') if node==('r','nested') else identity(node)
        adapter.owner=lambda node:7 if node==('r','nested') else owner(node)
        with self.assertRaises(chat.Rejected):controller.restore(binding,response=True)
        self.assertEqual(adapter.copy_actions,0);self.assertEqual(adapter.focus_count,0)

class CorrelatedNextInputTests(unittest.TestCase):
    def fixture(self, **options):
        adapter,controller,binding=ResponseFrameRestoreTests.restored(self,replaced=True,
            **{k:v for k,v in options.items() if k in ('moved_frame','wrong_mode','guard_loss')})
        adapter.profile_guard=lambda:not options.get('profile_loss')
        extra={
            'userrow':(39,'',''), 'userheading':(83,'You said: owned prior prompt',''),
            'unknown':(83,'You said: unrelated private turn',''),
            'editor3':(61,'Write your prompt to Claude',''),
            'dialog':(16,'private dialog','')}
        if options.get('wrong_user'):extra['userheading']=(83,'You said: wrong private prompt','')
        for method in ('owner','identity','children','parent','state','bounds'):
            original=getattr(adapter,method)
            def call(node,*args,method=method,original=original):
                name=node[1] if isinstance(node,tuple) else node
                if name in extra:
                    if method=='owner':return 8 if options.get('foreign_user') and name=='userheading' else 7
                    if method=='identity':return extra[name]
                    if method=='children':return [('r','userheading')] if name=='userrow' else []
                    if method=='parent':return ('r','userrow') if name=='userheading' else ('r','frame')
                    if method=='state':return (1<<30)|(1<<25)|(1<<7)|(1<<8)|(1<<24)
                    if method=='bounds':return (15,20,100,30)
                value=original(node,*args)
                if method=='children' and name=='frame':
                    value=value+([] if options.get('missing_user') else [('r','userrow')])
                    if options.get('extra_heading') or getattr(adapter,'extra_after_focus',False):value+=[('r','unknown')]
                    if options.get('duplicate_editor'):value+=[('r','editor3')]
                    if options.get('dialog'):value+=[('r','dialog')]
                if method=='identity' and name=='heading' and options.get('wrong_nonce'):return (83,'Claude responded: wrong nonce','')
                return value
            setattr(adapter,method,call)
        if options.get('wrong_mode'):
            attributes=adapter.attributes
            adapter.attributes=lambda node:{'current':'false'} if node==('r','chat') else attributes(node)
        if options.get('nonempty'):adapter.value='unknown private draft'
        if options.get('literal_filler'):adapter.value='\n'
        history=[dict(prompt='owned prior prompt',marker='private-marker')]
        return adapter,controller,binding,history

    def test_unique_new_empty_editor_can_submit_under_exact_history(self):
        a,c,b,h=self.fixture();c.restore_next_input(b,h)
        self.assertEqual(c.editor,('r','editor2'))
        self.assertEqual(c.frame,('r','frame'))
        self.assertEqual((a.focus_count,a.paste_count,a.copy_actions),(0,0,0))
        facts=c.submit('owned next prompt')
        self.assertTrue(facts['sendForwarded'])
        self.assertEqual((a.focus_count,a.paste_count,a.send_count),(1,1,1))
        self.assertNotIn('owned prior prompt',str(facts));self.assertNotIn('private-marker',str(facts))
        with self.assertRaises(chat.Rejected):a.key_guard()
        c.submit('replayed prompt')
        self.assertEqual(a.send_count,1)
        with self.assertRaises(chat.Rejected):c.restore_next_input(b,h)

    def test_two_prior_pairs_are_complete_without_tree_order_chronology(self):
        a,c,b,h=self.fixture()
        extra={'userrow2':(39,'',''),'userheading2':(83,'You said: second owned prompt',''),
            'row2':(85,'',''),'heading2':(83,'Claude responded: second-marker',''),
            'copy2':(43,'Copy','')}
        edges={'userrow2':['userheading2'],'row2':['heading2','copy2']}
        parents={'userheading2':'userrow2','heading2':'row2','copy2':'row2'}
        for method in ('owner','identity','children','parent','state','bounds'):
            original=getattr(a,method)
            def call(node,*args,method=method,original=original):
                name=node[1] if isinstance(node,tuple) else node
                if name in extra:
                    if method=='owner':return 7
                    if method=='identity':return extra[name]
                    if method=='children':return [('r',n) for n in edges.get(name,[])]
                    if method=='parent':return ('r',parents.get(name,'frame'))
                    if method=='state':return (1<<30)|(1<<25)|(1<<7)|(1<<8)|(1<<24)
                    if method=='bounds':return (15,20,100,30)
                value=original(node,*args)
                # Deliberately place assistant before user in exported traversal.
                if method=='children' and name=='frame':value=[('r','row2')]+value+[('r','userrow2')]
                return value
            setattr(a,method,call)
        h.append(dict(prompt='second owned prompt',marker='second-marker'))
        c.restore_next_input(b,h)
        self.assertTrue(c.submit('third owned prompt')['sendForwarded'])
        self.assertEqual(a.send_count,1)

    def test_history_and_rebind_negatives_are_action_free(self):
        for options in [dict(missing_user=True),dict(wrong_user=True),dict(wrong_nonce=True),
                dict(extra_heading=True),dict(duplicate_editor=True),dict(dialog=True),
                dict(foreign_user=True),dict(moved_frame=True),dict(wrong_mode=True),
                dict(profile_loss=True),dict(guard_loss=True),dict(nonempty=True),dict(literal_filler=True)]:
            with self.subTest(options=options):
                a,c,b,h=self.fixture(**options)
                with self.assertRaises(Exception):c.restore_next_input(b,h)
                self.assertEqual((a.focus_count,a.paste_count,a.send_count,a.copy_actions),(0,0,0,0))

    def test_readonly_history_scan_does_not_grant_input_or_copy(self):
        a,c,b,h=self.fixture();c.restore(b,response=True)
        witness=c.next_history_scope([(h[0]['prompt'],h[0]['marker'])])
        self.assertEqual(len(witness[0]),2)
        with self.assertRaises(chat.Rejected):a.key_guard()
        self.assertFalse(c.submit('ungranted next prompt')['sendAttempted'])
        self.assertEqual((a.focus_count,a.paste_count,a.send_count,a.copy_actions),(0,0,0,0))

    def test_lost_history_between_focus_and_paste_cannot_input(self):
        a,c,b,h=self.fixture();c.restore_next_input(b,h)
        def focus(node):a.focus_count+=1;a.focus=True;a.extra_after_focus=True;return True
        a.grab_focus=focus
        facts=c.submit('owned next prompt')
        self.assertEqual(facts['failureBoundary'],'response-heading')
        self.assertEqual((a.paste_count,a.send_count),(0,0))

    def test_lost_pair_after_readback_prevents_send(self):
        a,c,b,h=self.fixture();c.restore_next_input(b,h)
        readback=a.copy_input_once
        def changed(node):
            value=readback(node);a.extra_after_focus=True;return value
        a.copy_input_once=changed
        facts=c.submit('owned next prompt')
        self.assertEqual(facts['failureBoundary'],'response-heading')
        self.assertEqual((a.paste_count,a.send_count),(1,0))
        self.assertFalse(facts['sendAttempted'])

    def test_missing_duplicate_overflow_or_malformed_capability_rejects(self):
        for h in [[],[dict(prompt='x',marker='m')]*2,[dict(prompt='x',marker='m')]*3,
                [dict(prompt='',marker='m')],[dict(prompt='x',marker='m',extra='private')],
                [dict(prompt='x'*4097,marker='m')]]:
            a,c,b,_=self.fixture()
            with self.assertRaises(chat.Rejected):c.restore_next_input(b,h)
            self.assertEqual((a.focus_count,a.paste_count,a.send_count),(0,0,0))

class OwnedInputInventoryTests(unittest.TestCase):
    fixture=HypertextTests.fixture
    def test_complete_private_coverage_and_placeholder_comparison(self):
        root,leaf,link,records=self.fixture(child='source cue\n')
        records.update({('role',root):61,('role',leaf):73,
            ('attributes',root):{'tag':'div'},
            ('attributes',leaf):{'tag':'p','data-placeholder':'source cue'}})
        inventory={}
        value=chat.flatten_hypertext(root,lambda m,n,*a:records[(m,n,*a)],7,lambda:None,inventory=inventory)
        self.assertEqual(value,'source cue\n')
        self.assertTrue(inventory['completeTextCoverage']);self.assertTrue(inventory['rootSingleParagraph'])
        self.assertTrue(inventory['placeholderAttributeLfMatch']);self.assertFalse(inventory['placeholderAttributeMatch'])
        self.assertEqual((inventory['nodeCount'],inventory['resolvedNodeCount'],inventory['objectLinkCount']),(2,2,1))
        self.assertNotIn('source cue',str(inventory))

    def test_owned_unresolved_descendants_are_reported_without_emptiness_claim(self):
        root,p,link,r=self.fixture(child='source cue')
        extra=('owned','/unmapped')
        r.update({('children',p):[extra],('owner',extra):7,('state',extra):0,
            ('children',extra):[],('parent',extra):p,('role',root):61,('role',p):73,
            ('role',extra):116,('count',extra):1,('text',extra,1):'\n',('attributes',root):{},('attributes',p):{},('attributes',extra):{}})
        inventory={}
        chat.flatten_hypertext(root,lambda m,n,*a:r[(m,n,*a)],7,lambda:None,inventory=inventory)
        self.assertFalse(inventory['completeTextCoverage'])
        self.assertEqual(inventory['sourceShape']['unresolvedLfTextCount'],1)
        self.assertEqual(inventory['sourceShape']['unresolvedTextLeafCount'],1)
        self.assertEqual((inventory['nodeCount'],inventory['resolvedNodeCount']),(3,2))

    def test_moving_placeholder_is_rejected_before_inventory_emission(self):
        root,leaf,link,r=self.fixture(child='cue')
        r.update({('role',root):61,('role',leaf):73,('attributes',root):{},('attributes',leaf):{'data-placeholder':'cue'}})
        reads=0
        def query(method,node,*args):
            nonlocal reads
            if method=='attributes' and node==leaf:
                reads+=1
                if reads>1:return {'data-placeholder':'changed'}
            return r[(method,node,*args)]
        inventory={}
        with self.assertRaises(chat.Rejected):chat.flatten_hypertext(root,query,7,lambda:None,inventory=inventory)
        self.assertEqual(inventory,{})

    def test_exact_known_prompt_relation_is_passive_not_replacement_authority(self):
        for exact in (True,False):
            a,c,b,h=CorrelatedNextInputTests.fixture(self)
            a.value=h[-1]['prompt'] if exact else 'unknown draft'
            a.input_text_inventory={'nodeCount':1,'resolvedNodeCount':1,'paragraphCount':0,
                'rootChildCount':0,'textLeafCount':1,'otherRoleCount':0,'objectLinkCount':0,
                'completeTextCoverage':True,'rootSingleParagraph':False,'rootOnlyObjects':False,
                'placeholderAttributeMatch':False,'placeholderAttributeLfMatch':False}
            with self.assertRaises(chat.Rejected):c.restore_next_input(b,h)
            result=c.facts['ownedInputObservation']
            self.assertEqual(result['latestPromptMatches'],exact)
            self.assertEqual(result['knownPromptMatchCount'],int(exact))
            self.assertEqual((a.focus_count,a.paste_count,a.send_count),(0,0,0))
            self.assertNotIn(a.value,str(result))

class SourceShapeDiagnosticsTests(unittest.TestCase):
    fixture=HypertextTests.fixture
    def test_unresolved_leaf_partition_and_source_attributes(self):
        for value,key in [('', 'unresolvedEmptyTextCount'),('\n','unresolvedLfTextCount'),
                          ('cue','unresolvedExactResultCount'),('other','unresolvedOtherTextCount')]:
            root,p,link,r=self.fixture(child='cue');extra=('owned','/unmapped')
            r.update({('children',p):[extra],('owner',extra):7,('state',extra):0,
                ('children',extra):[],('parent',extra):p,('role',root):61,('role',p):73,
                ('role',extra):116,('count',extra):len(value),('text',extra,len(value)):value,
                ('attributes',root):{},('attributes',extra):{},('attributes',p):
                {'tag':'p','class':'is-empty is-editor-empty','data-placeholder':'cue'}})
            inventory={};chat.flatten_hypertext(root,lambda m,n,*a:r[(m,n,*a)],7,lambda:None,inventory=inventory)
            source=inventory['sourceShape']
            self.assertEqual(source[key],1);self.assertEqual(source['paragraphEmptyClassPairCount'],1)
            self.assertFalse(inventory['completeTextCoverage']);self.assertNotIn('cue',str(source))
    def test_unresolved_leaf_change_rejects(self):
        root,p,link,r=self.fixture(child='cue');extra=('owned','/unmapped')
        r.update({('children',p):[extra],('owner',extra):7,('state',extra):0,
            ('children',extra):[],('parent',extra):p,('role',root):61,('role',p):73,
            ('role',extra):116,('count',extra):1,('text',extra,1):'a',
            ('attributes',root):{},('attributes',p):{},('attributes',extra):{}})
        reads=0
        def query(method,node,*args):
            nonlocal reads
            if method=='text' and node==extra:
                reads+=1
                if reads>1:return 'b'
            return r[(method,node,*args)]
        with self.assertRaises(chat.Rejected):chat.flatten_hypertext(root,query,7,lambda:None,inventory={})

class EmptyClassCapabilityTests(unittest.TestCase):
    def fixture(self):
        a,c,b,h=CorrelatedNextInputTests.fixture(self,nonempty=True)
        a.empty_class_witness=('sealed-editor','sealed-private-text')
        c.empty_class_opt_in=True
        paste=a.paste_once
        def guarded_paste(prompt):
            a.before_paste();paste(prompt)
        a.paste_once=guarded_paste
        return a,c,b,h
    def test_exact_retained_history_capability_consumes_before_paste(self):
        a,c,b,h=self.fixture();c.restore_next_input(b,h)
        f=c.submit('owned next prompt')
        self.assertTrue(f['sendForwarded']);self.assertTrue(c.empty_class_dispatched)
        with self.assertRaises(chat.Rejected):c.before_empty_class_paste()
        self.assertEqual(a.paste_count,1)
    def test_source_attributes_or_native_text_change_after_focus_rejects(self):
        a,c,b,h=self.fixture();c.restore_next_input(b,h)
        focus=a.grab_focus
        def changed(node):
            result=focus(node);a.empty_class_witness=('changed-private-text',);return result
        a.grab_focus=changed
        f=c.submit('owned next prompt')
        self.assertFalse(f['sendForwarded']);self.assertEqual(a.paste_count,0)
        self.assertEqual(f['failureBoundary'],'input-empty-witness')
    def test_state_only_drift_is_identified_without_admitting_paste(self):
        a,c,b,h=self.fixture()
        a.empty_class_witness=('editor','private-text',(('state','editor',(),0),
            ('text','editor',(),'private-text')))
        c.restore_next_input(b,h)
        focus=a.grab_focus
        def changed(node):
            result=focus(node)
            a.empty_class_witness=('editor','private-text',(('state','editor',(),1<<12),
                ('text','editor',(),'private-text')))
            return result
        a.grab_focus=changed
        f=c.submit('owned next prompt')
        self.assertEqual(f['failureBoundary'],'input-empty-state')
        self.assertEqual(a.paste_count,0);self.assertEqual(a.send_count,0)
    def test_capability_is_not_ordinary_restore_permission(self):
        a,c,b,h=self.fixture();c.empty_class_opt_in=False
        with self.assertRaises(chat.Rejected):c.restore_next_input(b,h)
        self.assertEqual(a.paste_count,0)
    def test_lost_history_still_blocks(self):
        a,c,b,h=self.fixture();h[0]['marker']='different-private-marker'
        with self.assertRaises(chat.Rejected):c.restore_next_input(b,h)
        self.assertEqual(a.paste_count,0)
    def test_exact_doc_empty_class_accepts_sealed_unmapped_text_without_dropping_it(self):
        root,p,link,r=HypertextTests.fixture(self,child='cue')
        extra=('owned','/unmapped')
        r.update({('children',p):[extra],('owner',extra):7,('state',extra):0,
            ('children',extra):[],('parent',extra):p,('role',root):61,('role',p):73,
            ('role',extra):116,('count',extra):1,('text',extra,1):'\n',
            ('attributes',root):{},('attributes',extra):{},
            ('attributes',p):{'tag':'p','class':'is-empty is-editor-empty'}})
        cap={};inventory={};chat.flatten_hypertext(root,lambda m,n,*a:r[(m,n,*a)],7,lambda:None,inventory=inventory,capability=cap)
        self.assertFalse(inventory['completeTextCoverage']);self.assertIsNotNone(cap['witness'])
        self.assertTrue(any(record[:3]==('text',extra,(1,)) for record in cap['witness'][2]))
    def test_original_deadline_blocks_before_focus(self):
        a,c,b,h=self.fixture();c.restore_next_input(b,h);c.deadline=c.clock()-1
        f=c.submit('owned next prompt');self.assertFalse(f['sendForwarded']);self.assertEqual(a.paste_count,0)
    def test_missing_exact_class_pair_no_native_capability(self):
        root,p,link,r=HypertextTests.fixture(self,child='cue')
        r.update({('role',root):61,('role',p):73,('attributes',root):{},
            ('attributes',p):{'tag':'p','class':'is-editor-empty'}})
        cap={};chat.flatten_hypertext(root,lambda m,n,*a:r[(m,n,*a)],7,lambda:None,inventory={},capability=cap)
        self.assertIsNone(cap['witness'])

class FocusedEmptySealTests(unittest.TestCase):
    def fixture(self, change=None):
        a,c,b,h=EmptyClassCapabilityTests.fixture(self)
        root=('r','editor2')
        def witness(focused=False, extra=False):
            records=(('state',root,(),1<<12 if focused else 0),
                ('attributes',root,(),{'class':'ProseMirror ProseMirror-focused' if focused else 'ProseMirror'}),
                ('text',root,(),a.value))
            return (root,a.value,records+(('children',root,(),[]),) if extra else records)
        a.empty_class_witness=witness();c.restore_next_input(b,h)
        focus=a.grab_focus
        def focused(node):
            result=focus(node)
            if change=='class-lost':a.empty_class_witness=None
            elif change=='draft':a.value='new private draft';a.empty_class_witness=witness(True)
            elif change=='attachment':a.empty_class_witness=witness(True,True)
            elif change=='state':
                current=witness(True)
                a.empty_class_witness=(*current[:2],(('state',root,(),1<<8),*current[2][1:]))
            else:a.empty_class_witness=witness(True)
            return result
        a.grab_focus=focused
        if change=='late-attributes':
            paste=a.paste_once
            def late(prompt):
                current=witness(True)
                a.empty_class_witness=(*current[:2],(current[2][0],('attributes',root,(),{'class':'changed'}),current[2][2]))
                paste(prompt)
            a.paste_once=late
        return a,c
    def test_fresh_strict_focus_state_and_attributes_are_sealed_once(self):
        a,c=self.fixture();facts=c.submit('owned next prompt')
        self.assertTrue(facts['sendForwarded']);self.assertTrue(c.empty_focus_resealed)
        self.assertFalse(c.empty_focus_transition)
        self.assertEqual((a.focus_count,a.paste_count,a.send_count),(1,1,1))
    def test_draft_attachment_source_state_and_late_attribute_changes_reject(self):
        for change in ('class-lost','draft','attachment','state','late-attributes'):
            with self.subTest(change=change):
                a,c=self.fixture(change);facts=c.submit('owned next prompt')
                self.assertFalse(facts['sendForwarded'])
                self.assertEqual((a.paste_count,a.send_count),(0,0))
    def test_a_second_different_focused_sample_cannot_be_adopted(self):
        a,c=self.fixture();text=a.text;reads=[0]
        def moving(node):
            if a.focus:
                reads[0]+=1
                if reads[0]>1:
                    current=a.empty_class_witness
                    a.empty_class_witness=(*current[:2],(current[2][0],('attributes',current[0],(),{'class':'changed'}),current[2][2]))
            return text(node)
        a.text=moving;facts=c.submit('owned next prompt')
        self.assertFalse(facts['sendForwarded']);self.assertEqual(a.paste_count,0)

class EmptyInputDriftTests(unittest.TestCase):
    def test_focus_attributes_text_and_attachment_changes_are_distinguished(self):
        root=('owned','/editor');record=('state',root,(),0)
        original=(root,'synthetic', (record, ('attributes',root,(),{'class':'empty'}),('text',root,(1,),'x')))
        changed=(root,'synthetic', (('state',root,(),1<<12),*original[2][1:]))
        drift=chat.empty_input_drift(original,changed,True)
        self.assertTrue(drift['focusOnlyStateChange']);self.assertTrue(drift['recordKeysSame'])
        self.assertFalse(drift['stateSame']);self.assertTrue(drift['attributesSame'])
        changed=(root,'synthetic',(record,('attributes',root,(),{'class':'different'}),original[2][2]))
        self.assertFalse(chat.empty_input_drift(original,changed,False)['attributesSame'])
        changed=(root,'synthetic',(record,original[2][1],('text',root,(1,),'y')))
        self.assertFalse(chat.empty_input_drift(original,changed,False)['textRecordsSame'])
        self.assertFalse(chat.empty_input_drift(original,(root,'synthetic',()),False)['recordKeysSame'])
        missing=chat.empty_input_drift(original,None,False)
        self.assertFalse(any(missing.values()))
        self.assertNotIn('synthetic',str(drift));self.assertNotIn('/editor',str(drift))


class MainPacketTests(unittest.TestCase):
    def packet(self, mode='input-next-empty-class', change=False, restore_timeout=False):
        import io,json,sys
        from unittest.mock import patch
        a,c,b,h=EmptyClassCapabilityTests.fixture(self)
        if change:a.empty_class_witness=None
        if restore_timeout:
            def expired(*args):
                c.boundary='tree-identity'
                raise TimeoutError()
            c.restore_next_input=expired
        class Custody:
            def __init__(self,*args):pass
            def verify(self):return True
            def close(self):pass
        request=dict(pid=7,bus='r',path='root',checkerPid=1,window=1,
            bounds=[0,0,800,600],name='synthetic',nativeExecutable='/synthetic',
            deadline=chat.time.monotonic()+10,mode=mode,value='owned next prompt',binding=b,
            profileAuthority={},history=h)
        out=io.StringIO()
        with patch.object(sys,'stdin',type('Input',(),{'buffer':io.BytesIO(json.dumps(request).encode())})()),patch.object(sys,'stdout',out),patch.object(chat,'ProfileCustody',Custody),patch.object(chat,'native_adapter',return_value=a),patch.object(chat,'Controller',return_value=c):
            chat.main()
        return json.loads(out.getvalue())
    def test_new_mode_main_emits_existing_sent_packet(self):
        p=self.packet();self.assertEqual(p['facts']['stage'],'sent')
        self.assertTrue(p['facts']['sendForwarded']);self.assertEqual(set(p),{'facts','binding'})
        self.assertEqual(len(p['binding']),6)
    def test_missing_decoration_main_emits_closed_diagnostic_packet(self):
        p=self.packet(change=True);self.assertEqual(p['facts']['stage'],'input-not-empty')
        self.assertEqual(p['facts']['failureBoundary'],'input');self.assertFalse(p['facts']['pasteAttempted'])
    def test_restore_timeout_is_preserved_before_submit_and_never_dispatches(self):
        p=self.packet(restore_timeout=True)
        self.assertEqual(p['facts']['stage'],'deadline')
        self.assertEqual(p['facts']['failureBoundary'],'tree-identity')
        self.assertFalse(p['facts']['pasteAttempted'])
        self.assertFalse(p['facts']['sendAttempted'])
        self.assertIsNone(p['binding'])
    def test_invalid_mode_main_emits_request_rejection_packet(self):
        p=self.packet(mode='unrecognized');self.assertEqual(p['facts']['stage'],'blocked')
        self.assertEqual(p['facts']['failureBoundary'],'request');self.assertIsNone(p['binding'])

class RetryReadyDiagnosticsTests(unittest.TestCase):
    def case(self, **options):
        adapter,c,binding=ResponseFrameRestoreTests.restored(self)
        adapter.profile_guard=lambda:not options.get('profile_loss')
        identity,children,parent=adapter.identity,adapter.children,adapter.parent
        def new_identity(node):
            if node==('r','user'):return (83,'You said: private-failed-prompt','')
            if node in (('r','retry'),('r','retry2')):
                return (43,'Retry' if options.get('label') else 'Try again','')
            return identity(node)
        def new_children(node):
            if node==('r','row'):
                return [('r','user')]+([] if options.get('absent') else [('r','retry')])+([('r','retry2')] if options.get('duplicate') else [])
            return children(node)
        def new_parent(node):
            if node in (('r','user'),('r','retry'),('r','retry2')):
                return ('r','frame') if options.get('unattached') and node!=('r','user') else ('r','row')
            return parent(node)
        adapter.identity=new_identity;adapter.children=new_children;adapter.parent=new_parent
        c.restore(binding,response=True)
        if options.get('changed'):
            original=adapter.bounds
            count=[0]
            def moved(node):
                if node==('r','retry'):
                    count[0]+=1
                    return (10+count[0],10,100,40)
                return original(node)
            adapter.bounds=moved
        if options.get('disabled'):
            state=adapter.state
            adapter.state=lambda node:state(node)&~(1<<8) if node==('r','retry') else state(node)
        return adapter,c
    def test_main_mode_emits_passive_packet(self):
        import io,json,sys
        from unittest.mock import patch
        adapter,c=self.case();binding=c.binding()
        controller=chat.Controller(adapter,dict(pid=7,bus='r',path='root'),1,adapter.clock,adapter.sleep)
        class Custody:
            def __init__(self,*args):pass
            def verify(self):return True
            def close(self):pass
        request=dict(pid=7,bus='r',path='root',checkerPid=1,window=1,bounds=[0,0,800,600],
            name='synthetic',nativeExecutable='/synthetic',deadline=chat.time.monotonic()+10,
            mode='retry-ready',value='private-failed-prompt',binding=binding,profileAuthority={},history=[])
        out=io.StringIO()
        with patch.object(sys,'stdin',type('Input',(),{'buffer':io.BytesIO(json.dumps(request).encode())})()),patch.object(sys,'stdout',out),patch.object(chat,'ProfileCustody',Custody),patch.object(chat,'native_adapter',return_value=adapter),patch.object(chat,'Controller',return_value=controller):
            chat.main()
        packet=json.loads(out.getvalue())
        self.assertEqual(packet['facts']['stage'],'retry-diagnostic')
        self.assertFalse(packet['facts']['sendAttempted'])
        self.assertFalse(packet['facts']['recoveryVerified'])
        self.assertEqual(set(packet),{'facts','binding'})
        self.assertEqual(len(packet['binding']),6)
        self.assertEqual(adapter.send_count,0)
    def test_readonly_complete_candidate(self):
        for options,label in [({},'try-again'),({'label':True},'retry'),({'absent':True},'none'),({'duplicate':True},'ambiguous')]:
            adapter,c=self.case(**options)
            facts=c.retry_ready('private-failed-prompt',[])
            self.assertEqual(facts['retryCandidateObservation']['candidateLabel'],label)
            self.assertEqual((adapter.send_count,adapter.paste_count,adapter.focus_count,adapter.copy_actions),(0,0,0,0))
            self.assertNotIn('private',str(facts))
            with self.assertRaises(chat.Rejected):adapter.key_guard()
    def test_changed_detached_and_profile_reject_without_action(self):
        for options in ({'changed':True},{'unattached':True}):
            adapter,c=self.case(**options)
            with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',[])
            self.assertEqual(adapter.send_count,0)
        adapter,c=self.case();adapter.profile_guard=lambda:False
        with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',[])
    def test_missing_history_and_disabled_are_diagnostics_not_permission(self):
        adapter,c=self.case(disabled=True)
        facts=c.retry_ready('private-failed-prompt',[{'prompt':'prior','marker':'prior-nonce'}])
        self.assertFalse(facts['retryCandidateObservation']['historyMatched'])
        self.assertFalse(facts['retryCandidateObservation']['candidateEnabled'])
        self.assertFalse(facts['recoveryVerified']);self.assertEqual(adapter.send_count,0)

class RetryActionTests(unittest.TestCase):
    case=RetryReadyDiagnosticsTests.case
    def test_one_exact_failed_turn_retry_dispatch_and_no_send_increment(self):
        for options in ({},{'label':True}):
            adapter,c=self.case(**options)
            facts=c.retry_ready('private-failed-prompt',[],activate=True)
            self.assertEqual(facts['stage'],'retry-forwarded')
            self.assertTrue(facts['retryAttempted']);self.assertTrue(facts['retryForwarded'])
            self.assertFalse(facts['sendAttempted']);self.assertFalse(facts['sendForwarded'])
            self.assertNotIn('retryCandidateObservation',facts)
            self.assertEqual((adapter.send_count,adapter.paste_count,adapter.copy_actions),(1,0,0))
            with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',[],activate=True)
            self.assertEqual(adapter.send_count,1)
    def test_bad_scope_never_invokes(self):
        for options in ({'absent':True},{'duplicate':True},{'disabled':True},
                        {'unattached':True},{'changed':True}):
            adapter,c=self.case(**options)
            with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',[],activate=True)
            self.assertEqual(adapter.send_count,0)
        adapter,c=self.case()
        with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',
            [{'prompt':'missing-owned-prior','marker':'missing-marker'}],activate=True)
        self.assertEqual(adapter.send_count,0)
    def test_two_owned_prior_turns_retry_then_copy_exact_recovered_response(self):
        adapter,c=self.case()
        identity,children,parent=adapter.identity,adapter.children,adapter.parent
        prior={('r','prior-user-0'):'You said: owned-prior-0',
               ('r','prior-user-1'):'You said: owned-prior-1',
               ('r','prior-response-0'):'Claude responded: owned-marker-0',
               ('r','prior-response-1'):'Claude responded: owned-marker-1'}
        recovered=[False]
        adapter.identity=lambda node:(83,prior[node],'') if node in prior else identity(node)
        def items(node):
            if node in prior:return []
            if node==('r','frame'):return children(node)+list(prior)
            if node==('r','row') and recovered[0]:return [('r','heading'),('r','copy')]
            return children(node)
        adapter.children=items
        adapter.parent=lambda node:('r','frame') if node in prior else parent(node)
        invoke=adapter.invoke_once
        def action(node,index):
            if node==('r','retry'):
                adapter.send_count+=1;recovered[0]=True;return True
            return invoke(node,index)
        adapter.invoke_once=action
        records=[{'prompt':f'owned-prior-{index}','marker':f'owned-marker-{index}'} for index in range(2)]
        facts=c.retry_ready('private-failed-prompt',records,activate=True)
        self.assertEqual(facts['stage'],'retry-forwarded')
        facts=c.copy_response('private-marker')
        self.assertTrue(facts['responseVerified'])
        self.assertEqual(facts['stage'],'copied')
        self.assertEqual((adapter.send_count,adapter.copy_actions),(1,1))
        self.assertEqual(adapter.clipboard,'private-marker')
        self.assertNotIn('private-marker',str(facts))
    def test_uncertain_action_is_consumed_and_closed(self):
        adapter,c=self.case()
        def uncertain(node,index):
            adapter.send_count+=1
            raise TimeoutError()
        adapter.invoke_once=uncertain
        facts=c.retry_ready('private-failed-prompt',[],activate=True)
        self.assertEqual(facts['stage'],'action-uncertain')
        self.assertTrue(facts['retryAttempted']);self.assertFalse(facts['retryForwarded'])
        with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',[],activate=True)
        self.assertEqual(adapter.send_count,1)
    def test_failed_prompt_and_action_are_reproved_before_dispatch(self):
        adapter,c=self.case()
        original=adapter.identity;reads=[0]
        def changed(node):
            if node==('r','user'):
                reads[0]+=1
                if reads[0]>4:return (83,'You said: foreign-prompt','')
            return original(node)
        adapter.identity=changed
        with self.assertRaises(chat.Rejected):c.retry_ready('private-failed-prompt',[],activate=True)
        self.assertEqual(adapter.send_count,0)

class NativeTreeDiagnosticTests(unittest.TestCase):
    def test_exact_failure_cause_never_retries_or_exports_data(self):
        for kind in ('unavailable','deadline','non-list','limit','duplicate','wrong-owner','null'):
            a,c,b=ResponseFrameRestoreTests.restored(self)
            c.restore(b,response=True)
            original=a.children
            counts=[0]
            if kind=='wrong-owner':a.owner=lambda node:8
            elif kind=='null':
                a.owner=lambda node:(_ for _ in ()).throw(ValueError('PRIVATE dbus error'))
            else:
                def children(node):
                    if node!=c.frame:return original(node)
                    counts[0]+=1
                    if kind=='unavailable':raise ValueError('PRIVATE dbus error')
                    if kind=='deadline':raise TimeoutError('PRIVATE dbus timeout')
                    if kind=='non-list':return ()
                    if kind=='limit':return [c.frame]*1025
                    return [c.frame]
                a.children=children
            with self.assertRaises(Exception):
                if kind=='null':c.owned(('r','/org/a11y/atspi/null'))
                else:c.tree(c.frame)
            diag=c.facts['nativeTreeObservation']
            expected={'unavailable':'query-unavailable','null':'null-reference'}.get(kind,kind)
            self.assertEqual(diag['reason'],expected)
            self.assertLessEqual(counts[0],1)
            self.assertNotIn('PRIVATE',str(diag))
            self.assertEqual((a.paste_count,a.send_count),(0,0))
    def test_next_input_proof_one_fresh_full_frame_walk(self):
        a,c,b,h=CorrelatedNextInputTests.fixture(self)
        c.restore_next_input(b,h)
        original=c.tree;walks=[]
        def tree(start=None):
            walks.append(start);return original(start)
        c.tree=tree;c.next_input_proof()
        self.assertEqual(walks.count(c.frame),1)
        a.extra_after_focus=True
        with self.assertRaises(chat.Rejected):c.next_input_proof()
        self.assertEqual((a.paste_count,a.send_count),(0,0))

if __name__=='__main__':unittest.main()
