import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('retry', Path(__file__).with_name('zed-atspi-retry.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class NativeActionTests(unittest.TestCase):
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
