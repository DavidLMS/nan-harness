#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
spec = importlib.util.spec_from_file_location('visibility', Path(__file__).with_name('claude-atspi-visibility.py'))
v = importlib.util.module_from_spec(spec); spec.loader.exec_module(v)
HELD = dict(pid=12, bus='private-bus', path='/private-target')
TARGET = ('private-bus','/private-target'); PARENT=('private-bus','/private-parent'); APP=('private-bus','/private-app')
class Fixture:
    def __init__(self): self.calls=[]; self.states={TARGET:1<<7,PARENT:0}; self.now=0; self.changed=False; self.reads=0
    def log(self, method,node): self.calls.append((method,node))
    def owner(self,node): self.log('owner',node); return 12
    def identity(self,node):
        self.log('identity',node); self.reads+=1
        return (61, v.LABEL if not self.changed or self.reads==1 else 'other', '')
    def state(self,node): self.log('state',node); return self.states[node]
    def bounds(self,node): self.log('bounds',node); return (1,2,30,40)
    def parent(self,node): self.log('parent',node); return PARENT if node==TARGET else APP
    def role(self,node): self.log('role',node); return 75 if node==APP else 20
class Tests(unittest.TestCase):
    def test_explicit_wire_calls_have_no_implicit_proxy_or_introspection(self):
        calls=[]
        class Bus:
            def get_object(self, *_args, **_kwargs):
                raise AssertionError('implicit proxy query')
            def call_blocking(self, *args, **kwargs):
                calls.append((args,kwargs))
                return 12 if args[3]=='GetConnectionUnixProcessID' else 'owned'
        adapter=v.Adapter.__new__(v.Adapter);adapter.bus=Bus();adapter.deadline=5
        with patch.object(v.time, 'monotonic', return_value=2):
            self.assertEqual(adapter.owner(TARGET),12)
            self.assertEqual(adapter.call(TARGET,'Get','org.freedesktop.DBus.Properties',
                'org.a11y.atspi.Accessible','Name'),'owned')
            adapter.call(TARGET,'GetAccessibleAtPoint','org.a11y.atspi.Component',3,4,0)
        self.assertEqual([call[0][4] for call in calls],['s','ss','iiu'])
        self.assertTrue(all(call[1]=={'timeout':3} for call in calls))
        with patch.object(v.time, 'monotonic', return_value=5):
            with self.assertRaises(TimeoutError):adapter.owner(TARGET)
        self.assertEqual(len(calls),3)
        with self.assertRaises(KeyError):adapter.call(TARGET,'Introspect')
        self.assertEqual(len(calls),3)
    def run_fixture(self,f,deadline=5): return v.observe(HELD,f,deadline,lambda:f.now)
    def test_hidden_editor_and_ancestor_are_distinct_native_states(self):
        f=Fixture(); result=self.run_fixture(f)
        self.assertEqual(result['status'],'complete'); self.assertFalse(result['visible']); self.assertFalse(result['showing'])
        self.assertEqual(result['checkedAncestorCount'],1); self.assertEqual(result['hiddenAncestorCount'],1)
        f.states[TARGET]|=1<<25; f.states[PARENT]=1<<30
        result=self.run_fixture(f); self.assertFalse(result['visible']); self.assertTrue(result['showing']); self.assertEqual(result['hiddenAncestorCount'],0)
    def test_expired_budget_performs_no_query(self):
        f=Fixture(); self.assertEqual(self.run_fixture(f,0)['stage'],'deadline'); self.assertEqual(f.calls,[])
    def test_expiry_during_query_stops_next_query(self):
        f=Fixture()
        def owner(node): f.log('owner',node); f.now=5; return 12
        f.owner=owner; self.assertEqual(self.run_fixture(f)['stage'],'deadline'); self.assertEqual(len(f.calls),1)
    def test_foreign_node_or_parent_cannot_publish_states(self):
        for foreign in (TARGET,PARENT):
            f=Fixture(); f.owner=lambda node: 13 if node==foreign else 12
            result=self.run_fixture(f); self.assertEqual(result['status'],'unavailable'); self.assertTrue(all(result[k] is None for k in v.FIELDS))
    def test_changed_source_identity_is_not_complete(self):
        f=Fixture(); f.changed=True; self.assertEqual(self.run_fixture(f)['stage'],'identity')
    def test_changed_bounds_cannot_publish_complete(self):
        f=Fixture(); n=[0]
        def bounds(node): n[0]+=1; return (1,2,30+n[0],40)
        f.bounds=bounds; self.assertEqual(self.run_fixture(f)['status'],'changed')
    def test_malformed_state_or_defunct_or_noneditable_rejected(self):
        for state in (True,-1,1<<64,1<<6,0):
            f=Fixture(); f.states[TARGET]=state
            self.assertEqual(self.run_fixture(f)['stage'],'state')
    def test_cycle_and_parent_limit_are_bounded(self):
        f=Fixture(); f.parent=lambda node:TARGET; self.assertEqual(self.run_fixture(f)['stage'],'parent')
        f=Fixture(); f.parent=lambda node:(node[0],node[1]+'x'); f.role=lambda node:20; f.state=lambda node:1<<7
        self.assertEqual(self.run_fixture(f)['status'],'limit')
    def test_timeout_and_invalid_geometry_never_leak(self):
        for cause in ('timeout','geometry'):
            f=Fixture()
            if cause=='timeout':
                def fail(node): raise TimeoutError('PRIVATE')
                f.state=fail
            else: f.bounds=lambda node:(1,2,'PRIVATE',4)
            result=self.run_fixture(f); self.assertNotIn('PRIVATE',str(result)); self.assertTrue(all(result[k] is None for k in v.FIELDS))
    def test_supervised_cached_root_resolves_unique_source_then_revalidates(self):
        f=Fixture(); f.children=lambda node: [TARGET] if node==APP else []
        f.role=lambda node: 75 if node==APP else 61
        root=dict(HELD,path=APP[1])
        result=v.inspect(root,f,5,lambda:f.now)
        self.assertEqual(result['status'],'complete')
    def test_supervised_root_rejects_duplicate_source_foreign_and_cycles(self):
        for cause in ('duplicate','foreign','cycle'):
            f=Fixture(); f.role=lambda node:75 if node==APP else 61
            second=('private-bus','/second')
            f.state=lambda node:1<<7
            f.children=lambda node: [TARGET,second] if node==APP else []
            if cause=='foreign': f.owner=lambda node:13 if node==TARGET else 12
            if cause=='cycle': f.children=lambda node:[APP]
            result=v.inspect(dict(HELD,path=APP[1]),f,5,lambda:f.now)
            self.assertEqual(result['status'],'unavailable'); self.assertTrue(all(result[k] is None for k in v.FIELDS))
    def test_supervised_root_expiry_has_no_late_queries(self):
        f=Fixture(); f.children=lambda node:[]
        result=v.inspect(dict(HELD,path=APP[1]),f,0,lambda:f.now)
        self.assertEqual(result['stage'],'deadline'); self.assertEqual(f.calls,[])
if __name__=='__main__': unittest.main()
