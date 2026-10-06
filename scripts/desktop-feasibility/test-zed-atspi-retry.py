import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('retry', Path(__file__).with_name('zed-atspi-retry.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class NativeActionTests(unittest.TestCase):
    def test_pointer_readback_distinguishes_identity_from_transport(self):
        bounds = (10,20,80,30)
        for identity,expected in (((43,'Retry',bounds),'same-source'),
                ((43,'PRIVATE other',bounds),'changed-source'),
                ((43,'Retry',(11,20,80,30)),'changed-source'),
                ((43,'Retry',(10,20,0,30)),'unavailable')):
            self.assertEqual(module.classify_pointer_target((0,0),lambda:identity,bounds),expected)
        self.assertEqual(module.classify_pointer_target((64,0),lambda:self.fail('defunct node queried'),bounds),'defunct')
        for states in ((True,0),(-1,0),(0,),[0,0]):
            self.assertEqual(module.classify_pointer_target(states,lambda:self.fail('invalid state queried'),bounds),'unavailable')

    def test_pointer_readback_checks_owner_geometry_guard_and_closes_bus(self):
        request=dict(bus=':1.2',path='/org/a11y/atspi/accessible/3',pid=7,
                     bounds=(110,220,80,30),clientOrigin=(100,200))
        for case,expected in (('same','same-source'),('foreign','unavailable'),
                ('lost-guard','unavailable'),('query-failed','unavailable'),('close-failed','unavailable')):
            with self.subTest(case=case):
                node=Mock()
                node.GetState.return_value=(0,0)
                node.GetRole.return_value=43
                node.Get.return_value='Retry'
                node.GetExtents.return_value=(10,20,80,30)
                bus=Mock()
                owner=Mock()
                owner.GetConnectionUnixProcessID.return_value=8 if case=='foreign' else 7
                bus.get_object.side_effect=lambda name,path:owner if name=='org.freedesktop.DBus' else node
                dbus=Mock()
                dbus.UInt32.side_effect=lambda value:value
                dbus.bus.BusConnection.return_value=bus
                if case=='query-failed':node.GetState.side_effect=RuntimeError('PRIVATE')
                if case=='close-failed':bus.close.side_effect=RuntimeError('PRIVATE')
                guard=Mock(side_effect=[True,case!='lost-guard'])
                with patch.dict('sys.modules',dbus=dbus):
                    result=module.observe_pointer_target(request,guard,4,clock=lambda:0)
                self.assertEqual(result,expected)
                bus.close.assert_called_once()
                node.DoAction.assert_not_called()
                if case=='same':self.assertEqual(node.GetExtents.call_args.args,(1,))

    def run_action(self, snapshots, response=True, clock=lambda: 0, post=lambda: True):
        facts = {'actionAttempted': False, 'forwarded': False}
        values = iter(snapshots)
        actions = []
        def action():
            actions.append(1)
            if isinstance(response, Exception):
                raise response
            return response
        try:
            result = module.perform_once(lambda: next(values), action, 4, facts, clock, post)
        except TimeoutError:
            result = False
        return result, facts, actions

    def test_only_one_action_after_two_identical_proofs(self):
        result, facts, actions = self.run_action([('held',), ('held',)])
        self.assertTrue(result)
        self.assertEqual(actions, [1])
        self.assertTrue(facts['forwarded'])

    def test_changed_bounds_or_owner_never_dispatch(self):
        for changed in [('bounds-changed',), ('owner-changed',), ('action-count-changed',)]:
            result, facts, actions = self.run_action([('held',), changed])
            self.assertFalse(result)
            self.assertEqual(actions, [])
            self.assertFalse(facts['actionAttempted'])

    def test_false_timeout_and_nonboolean_never_fallback(self):
        for response in (False, TimeoutError(), 'true'):
            result, facts, actions = self.run_action([1, 1], response)
            self.assertFalse(result)
            self.assertEqual(actions, [1])
            self.assertTrue(facts['actionAttempted'])

    def test_deadline_before_action_and_after_forwarding(self):
        result, _, actions = self.run_action([1, 1], clock=lambda: 4)
        self.assertFalse(result)
        self.assertEqual(actions, [])
        ticks = iter([0, 0, 4])
        result, facts, actions = self.run_action([1, 1], clock=lambda: next(ticks))
        self.assertFalse(result)
        self.assertEqual(actions, [1])
        self.assertTrue(facts['forwarded'])

    def test_post_ownership_loss_never_certifies_forwarding_as_recovery(self):
        result, facts, actions = self.run_action([1, 1], post=lambda: False)
        self.assertFalse(result)
        self.assertTrue(facts['forwarded'])
        self.assertEqual(actions, [1])

    def test_close_failure_preserves_closed_receipt_and_reports_failure(self):
        events = []
        class BrokenBus:
            def close(self):
                events.append('close')
                raise RuntimeError('private transport detail')
        facts = {'actionAttempted': True, 'forwarded': True}
        self.assertFalse(module.close_transport(BrokenBus(), lambda value: events.append(dict(value)), facts))
        self.assertEqual(events, ['close', facts])

    def test_request_rejects_extra_payload_and_bool_identity(self):
        request = dict(pid=42, window=8, x=5, y=5, bus=':1.3', path='/org/a11y/atspi/accessible/4', bounds=[0, 0, 10, 10])
        self.assertTrue(module.valid_request(request))
        self.assertFalse(module.valid_request(dict(request, secret='private')))
        self.assertFalse(module.valid_request(dict(request, pid=True)))

class RetainedTargetTests(unittest.TestCase):
    def test_one_retained_observation_classifies_without_replaying(self):
        sealed = (43, 'Retry', ((1, 2, 3, 4), (1, 2, 3, 4)), (256, 0))
        for expected, states, identity in (
                ('unchanged', (256, 0), sealed[:3]),
                ('changed', (0, 0), sealed[:3]),
                ('changed', (256, 0), (43, 'Retry', ((2, 2, 3, 4), (2, 2, 3, 4)))),
                ('defunct', (64, 0), None)):
            queries = []
            def state_query():
                queries.append('state')
                return states
            def identity_query():
                queries.append('identity')
                return identity
            self.assertEqual(module.observe_retained_target(state_query, identity_query,
                sealed, lambda: True, 4, lambda: 0), (expected, True))
            self.assertEqual(queries, ['state'] if expected == 'defunct' else ['state', 'identity'])

    def test_transport_failure_is_unavailable_and_elapsed_deadline_never_queries(self):
        queries = []
        def failed():
            queries.append('state')
            raise RuntimeError('org.freedesktop.DBus.Error.UnknownObject PRIVATE')
        self.assertEqual(module.observe_retained_target(failed, lambda: None,
            (), lambda: True, 4, lambda: 0), ('unavailable', True))
        self.assertEqual(module.observe_retained_target(failed, lambda: None,
            (), lambda: True, 4, lambda: 4), ('unavailable', False))
        self.assertEqual(queries, ['state'])
        def lost_guard():
            raise TimeoutError('private guard detail')
        self.assertEqual(module.observe_retained_target(failed, lambda: None,
            (), lost_guard, 4, lambda: 0), ('unavailable', False))
        self.assertEqual(queries, ['state'])
        guards = iter([True, False])
        self.assertEqual(module.observe_retained_target(lambda: (64, 0), lambda: None,
            (), lambda: next(guards), 4, lambda: 0), ('unavailable', False))

if __name__ == '__main__':
    unittest.main()
