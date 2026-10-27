#!/usr/bin/env python3
"""Synthetic owned SQLite trajectories and strict live-task evidence stay private."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'actions'))
import mimo_session_observation as observation

MARKER = 'PRIVATE_READ_MARKER'
PROMPT = 'PRIVATE_ORIGINAL_PROMPT'


class NativeSessionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.root.chmod(0o700)
        self.data = self.root / 'mimo/data'
        self.data.mkdir(parents=True)
        self.database = self.data / 'mimocode.db'
        self.env = patch.dict(os.environ, {'MIMOCODE_HOME': str(self.root / 'mimo'),
                                         'NAN_API_KEY': 'PRIVATE_CREDENTIAL'}, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)
        with self.connection() as db:
            db.executescript('''
                CREATE TABLE session(id TEXT PRIMARY KEY,parent_id TEXT,directory TEXT);
                CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,agent_id TEXT,time_created INTEGER,data TEXT);
                CREATE TABLE part(id TEXT PRIMARY KEY,session_id TEXT,message_id TEXT,data TEXT);
            ''')
        self.session('PRIVATE_SESSION')
        self.message('PRIVATE_USER', 'user', 1)
        self.part('PRIVATE_USER_TEXT', 'PRIVATE_USER', dict(type='text', text=PROMPT))
        self.assistant('PRIVATE_ASSISTANT', 2)
        self.part('PRIVATE_READ', 'PRIVATE_ASSISTANT', dict(type='tool', tool='read',
            state=dict(status='completed', input={'file_path': 'PRIVATE_PATH'}, output=MARKER)))
        self.part('PRIVATE_TEXT', 'PRIVATE_ASSISTANT', dict(type='text', text=MARKER + ' NAN_CANARY_OK PRIVATE_RESPONSE'))

    def connection(self):
        return sqlite3.connect(self.database)

    def session(self, identity, parent=None, directory=None):
        with self.connection() as db:
            db.execute('INSERT INTO session VALUES(?,?,?)', (identity, parent, str(directory or self.root)))

    def message(self, identity, role, created, session='PRIVATE_SESSION', agent='main', **data):
        with self.connection() as db:
            db.execute('INSERT INTO message VALUES(?,?,?,?,?)',
                (identity, session, agent, created, json.dumps(dict(role=role, **data))))

    def assistant(self, identity, created, session='PRIVATE_SESSION', agent='main', **changes):
        data = dict(parentID='PRIVATE_USER', time={'created': created, 'completed': created + 1}, finish='stop')
        self.message(identity, 'assistant', created, session, agent, **(data | changes))

    def part(self, identity, message, data, session='PRIVATE_SESSION'):
        with self.connection() as db:
            db.execute('INSERT INTO part VALUES(?,?,?,?)', (identity, session, message, json.dumps(data)))

    def collect(self):
        value = observation.collect(self.root, MARKER, PROMPT)
        self.assertNotIn('PRIVATE_', json.dumps(value))
        self.assertEqual(observation.validate(value), value)
        return value

    def live_evidence(self):
        return observation.collect(self.root, MARKER, PROMPT, require_live_task=True)

    def bind_read(self):
        with self.connection() as db:
            db.execute("UPDATE part SET data=json_set(data,'$.state.input.file_path',?) WHERE id='PRIVATE_READ'",
                       (str(self.root / 'read-target.txt'),))

    def test_live_evidence_requires_the_exact_target_and_a_fully_completed_turn(self):
        self.assertEqual(self.live_evidence()['status'], 'unavailable')
        self.bind_read()
        self.assertEqual(self.live_evidence()['status'], 'observed')
        with self.connection() as db:
            db.execute("UPDATE part SET data=json_set(data,'$.state.input.file_path','read-target.txt') WHERE id='PRIVATE_READ'")
        self.assertEqual(self.live_evidence()['status'], 'observed')

    def test_live_evidence_rejects_incomplete_failed_or_wrong_turn_facts(self):
        self.bind_read()
        cases = (
            ("UPDATE part SET data=json_set(data,'$.state.status','running') WHERE id='PRIVATE_READ'", ()),
            ("UPDATE part SET data=json_set(data,'$.state.output','wrong marker') WHERE id='PRIVATE_READ'", ()),
            ("UPDATE part SET data=json_set(data,'$.text','wrong answer') WHERE id='PRIVATE_TEXT'", ()),
            ("UPDATE part SET data=json_set(data,'$.text','different prompt') WHERE id='PRIVATE_USER_TEXT'", ()),
            ("UPDATE message SET data=json_remove(data,'$.time.completed') WHERE id='PRIVATE_ASSISTANT'", ()),
            ("UPDATE message SET data=json_set(data,'$.finish','length') WHERE id='PRIVATE_ASSISTANT'", ()),
            ("UPDATE message SET data=json_set(data,'$.finish','other') WHERE id='PRIVATE_ASSISTANT'", ()),
            ("UPDATE message SET data=json_set(data,'$.error',json(?)) WHERE id='PRIVATE_ASSISTANT'",
             (json.dumps({'name': 'APIError', 'data': {'statusCode': 401}}),)),
            ("UPDATE part SET data=json_set(data,'$.state.output',?) WHERE id='PRIVATE_READ'", (MARKER + ' NH-BRIDGE-error',)),
            ("UPDATE session SET directory='other directory'", ()),
        )
        baseline = self.database.read_bytes()
        for query, arguments in cases:
            with self.subTest(query=query):
                self.database.write_bytes(baseline)
                with self.connection() as db:
                    db.execute(query, arguments)
                self.assertEqual(self.live_evidence()['status'], 'unavailable')

    def test_live_evidence_rejects_a_failed_tool_even_after_a_successful_read(self):
        self.bind_read()
        self.part('PRIVATE_FAILED_TOOL', 'PRIVATE_ASSISTANT', dict(type='tool', tool='shell', state={'status': 'error'}))
        self.assertEqual(self.live_evidence()['status'], 'unavailable')

    def test_live_evidence_cannot_adopt_a_newer_incomplete_assistant(self):
        self.bind_read()
        self.assistant('PRIVATE_INCOMPLETE', 3, time={'created': 3}, finish='stop')
        self.assertEqual(self.live_evidence()['status'], 'unavailable')

    def unavailable(self, reason):
        value = self.collect()
        self.assertEqual((value['status'], value['reason']), ('unavailable', reason))
        self.assertTrue(all(value[key] is None for key in observation.METRICS))

    def test_completed_owned_turn_projects_only_counts_and_marker_booleans(self):
        before = self.database.read_bytes()
        value = self.collect()
        self.assertEqual(value['status'], 'observed')
        self.assertEqual((value['mainUsers'], value['assistants'], value['completedAssistants']), (1, 1, 1))
        self.assertEqual((value['readTools'], value['completedReadTools']), (1, 1))
        self.assertTrue(all(value[key] for key in observation.FLAGS))
        self.assertEqual(value['finalFinish'], 'stop')
        self.assertFalse(any(value['errorFamilies'].values()))
        self.assertEqual(value['httpStatuses'], [])
        self.assertEqual(self.database.read_bytes(), before)
        self.assertEqual({p.name for p in self.data.iterdir()}, {'mimocode.db'})

    def test_read_only_snapshot_includes_committed_wal_without_checkpoint(self):
        db = self.connection()
        self.addCleanup(db.close)
        db.execute('PRAGMA journal_mode=WAL')
        db.execute("UPDATE message SET data=json_set(data,'$.finish','tool-calls') WHERE id='PRIVATE_ASSISTANT'")
        db.commit()
        before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.data.iterdir() if not p.name.endswith('-shm')}
        value = self.collect()
        self.assertEqual(value['status'], 'observed')
        self.assertEqual(value['finalFinish'], 'tool-calls')
        self.assertFalse(value['finalAssistantComplete'])
        self.assertFalse(value['finalReadMarker'])
        after = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.data.iterdir() if not p.name.endswith('-shm')}
        self.assertEqual(after, before)

    def test_auxiliary_agents_sessions_and_foreign_parent_do_not_supply_main_evidence(self):
        self.session('PRIVATE_AUX_SESSION', parent='PRIVATE_SESSION')
        self.message('PRIVATE_AUX_USER', 'user', 8, agent='actor')
        self.part('PRIVATE_AUX_PROMPT', 'PRIVATE_AUX_USER', dict(type='text', text=PROMPT))
        self.assistant('PRIVATE_AUX_ASSISTANT', 99, agent='actor')
        self.part('PRIVATE_AUX_TEXT', 'PRIVATE_AUX_ASSISTANT', dict(type='text', text=MARKER + ' NAN_CANARY_OK'))
        self.assistant('PRIVATE_OTHER_PARENT', 100, parentID='PRIVATE_FOREIGN_USER')
        value = self.collect()
        self.assertEqual((value['mainUsers'], value['assistants'], value['readTools']), (1, 1, 1))

    def test_latest_matching_main_assistant_controls_final_text(self):
        self.assistant('PRIVATE_LATEST', 3, time={'created': 3}, finish='tool-calls')
        value = self.collect()
        self.assertEqual(value['assistants'], 2)
        self.assertEqual(value['completedAssistants'], 1)
        self.assertTrue(value['readOutputMarker'])
        self.assertFalse(value['finalAssistantComplete'])
        self.assertFalse(value['finalReadMarker'])
        self.assertFalse(value['finalCompletionMarker'])

    def test_stop_without_native_completion_timestamp_is_not_finalized(self):
        with self.connection() as db:
            payload = dict(role='assistant', parentID='PRIVATE_USER', time={'created': 2}, finish='stop')
            db.execute('UPDATE message SET data=? WHERE id=?', (json.dumps(payload), 'PRIVATE_ASSISTANT'))
        value = self.collect()
        self.assertEqual(value['status'], 'observed')
        self.assertEqual(value['finalFinish'], 'stop')
        self.assertEqual(value['completedAssistants'], 0)
        self.assertFalse(value['finalAssistantComplete'])
        self.assertFalse(value['finalReadMarker'])
        self.assertFalse(value['finalCompletionMarker'])
        self.assertTrue(value['readOutputMarker'])
        with self.assertRaises(ValueError):
            observation.validate(value | {'finalAssistantComplete': True})

    def test_pending_running_and_error_reads_are_not_completed(self):
        for status in ('pending', 'running', 'error'):
            with self.subTest(status=status), self.connection() as db:
                db.execute('UPDATE part SET data=? WHERE id=?',
                    (json.dumps(dict(type='tool', tool='read', state={'status': status, 'output': MARKER})), 'PRIVATE_READ'))
            value = self.collect()
            self.assertEqual((value['readTools'], value['completedReadTools']), (1, 0))
            self.assertFalse(value['readOutputMarker'])

    def test_error_names_and_http_codes_project_without_private_payloads(self):
        for name in (*sorted(observation.ERRORS - {'other'}), 'PRIVATE_UNKNOWN_ERROR'):
            with self.subTest(name=name), self.connection() as db:
                payload = dict(role='assistant', parentID='PRIVATE_USER', time={'created': 2, 'completed': 3},
                    finish='stop', error={'name': name, 'data': {'message': 'PRIVATE_ERROR', 'statusCode': 401}})
                db.execute('UPDATE message SET data=? WHERE id=?', (json.dumps(payload), 'PRIVATE_ASSISTANT'))
            value = self.collect()
            family = name if name in observation.ERRORS else 'other'
            self.assertEqual(value['errorFamilies'][family], 1)
            self.assertEqual(sum(value['errorFamilies'].values()), 1)
            self.assertEqual(value['httpStatuses'], [401] if name == 'APIError' else [])
            self.assertFalse(value['finalAssistantComplete'])

    def test_ambiguous_or_unmatched_principal_sessions_remain_unavailable(self):
        self.session('PRIVATE_SECOND')
        self.message('PRIVATE_SECOND_USER', 'user', 4, session='PRIVATE_SECOND')
        self.part('PRIVATE_SECOND_TEXT', 'PRIVATE_SECOND_USER', dict(type='text', text=PROMPT), session='PRIVATE_SECOND')
        self.unavailable('session-ambiguous')
        with self.connection() as db:
            db.execute("UPDATE session SET directory='PRIVATE_OTHER_DIRECTORY' WHERE id='PRIVATE_SECOND'")
            db.execute("UPDATE part SET data=? WHERE id='PRIVATE_USER_TEXT'", (json.dumps(dict(type='text', text='PRIVATE_OTHER_PROMPT')),))
        self.unavailable('session-unbound')

    def test_additional_main_user_is_not_a_sole_principal(self):
        self.message('PRIVATE_OTHER_USER', 'user', 4)
        self.part('PRIVATE_OTHER_TEXT', 'PRIVATE_OTHER_USER', dict(type='text', text='PRIVATE_OTHER_PROMPT'))
        self.unavailable('session-ambiguous')

    def test_synthetic_or_ignored_user_text_does_not_bind_prompt(self):
        for key in ('synthetic', 'ignored'):
            with self.subTest(key=key), self.connection() as db:
                db.execute('UPDATE part SET data=? WHERE id=?', (json.dumps(dict(type='text', text=PROMPT, **{key: True})), 'PRIVATE_USER_TEXT'))
            self.unavailable('session-unbound')

    def test_environment_binding_refuses_overrides_before_opening_sqlite(self):
        for change in ({'MIMOCODE_HOME': 'PRIVATE_OTHER_HOME'}, {'MIMOCODE_DB': ':memory:'},
                       {'MIMOCODE_DB': '/PRIVATE_DATABASE'}, {'MIMOCODE_DISABLE_CHANNEL_DB': 'false'}):
            with self.subTest(change=change), patch.dict(os.environ, change), patch.object(observation.sqlite3, 'connect') as connect:
                self.unavailable('env-unbound')
                connect.assert_not_called()

    def test_raw_and_physical_owned_directory_aliases_bind_only_exact_workspace(self):
        alias = self.root / 'ancestor'
        alias.symlink_to(self.root, target_is_directory=True)
        raw = alias / 'inner'
        raw.mkdir(mode=0o700)
        (raw / 'mimo/data').mkdir(parents=True)
        target = raw / 'mimo/data/mimocode.db'
        target.write_bytes(self.database.read_bytes())
        with sqlite3.connect(target) as db:
            db.execute('UPDATE session SET directory=?', (str(raw.resolve()),))
        with patch.dict(os.environ, {'MIMOCODE_HOME': str(raw / 'mimo')}):
            self.assertEqual(observation.collect(raw, MARKER, PROMPT)['status'], 'observed')
        # The owned workspace itself may not be a symlink.
        with patch.dict(os.environ, {'MIMOCODE_HOME': str(alias / 'mimo')}):
            self.assertEqual(observation.collect(alias, MARKER, PROMPT)['reason'], 'path-unbound')

    def test_database_and_sidecar_links_are_never_followed(self):
        for suffix in ('-wal', '-shm'):
            link = Path(str(self.database) + suffix)
            link.symlink_to(self.database)
            with self.subTest(suffix=suffix), patch.object(observation.sqlite3, 'connect') as connect:
                self.unavailable('path-unbound')
                connect.assert_not_called()
            link.unlink()
        moved = self.database.with_suffix('.private-copy')
        self.database.rename(moved)
        self.database.symlink_to(moved)
        self.unavailable('path-unbound')

    def test_private_workspace_and_native_child_directory_ownership_is_required(self):
        self.root.chmod(0o755)
        self.unavailable('path-unbound')
        self.root.chmod(0o700)
        original = self.root / 'mimo'
        moved = self.root / 'native-private'
        original.rename(moved)
        original.symlink_to(moved, target_is_directory=True)
        self.unavailable('path-unbound')

    def test_missing_schema_and_foreign_part_membership_are_closed_failures(self):
        with self.connection() as db:
            db.execute("UPDATE part SET session_id='PRIVATE_FOREIGN_SESSION' WHERE id='PRIVATE_READ'")
        self.unavailable('data-invalid')
        with self.connection() as db:
            db.execute('DROP TABLE part')
        self.unavailable('schema-invalid')
        self.database.unlink()
        self.unavailable('database-missing')

    def test_malformed_json_duplicate_fields_and_bad_typed_errors_fail_closed(self):
        bad = ['{"role":"assistant","role":"user"}', 'PRIVATE_INVALID_JSON',
               json.dumps(dict(role='assistant', parentID='PRIVATE_USER', time={'created': 2}, error=None)),
               json.dumps(dict(role='assistant', parentID='PRIVATE_USER', time={'created': 2},
                               error={'name': 'APIError', 'data': {'statusCode': True}})),
               json.dumps(dict(role='assistant', parentID='PRIVATE_USER', time={'created': 2}, finish=None)),
               json.dumps(dict(role='assistant', parentID='PRIVATE_USER', time={'created': float('nan')}))]
        for raw in bad:
            with self.subTest(raw=raw), self.connection() as db:
                db.execute('UPDATE message SET data=? WHERE id=?', (raw, 'PRIVATE_ASSISTANT'))
            self.unavailable('data-invalid')

    def test_row_json_and_database_caps_do_not_publish_partial_zeroes(self):
        with self.connection() as db:
            db.execute('UPDATE part SET data=? WHERE id=?', ('X' * (observation.MAX_JSON + 1), 'PRIVATE_READ'))
        self.unavailable('limit-exceeded')
        with self.database.open('ab') as output:
            output.truncate(observation.MAX_DATABASE + 1)
        self.unavailable('limit-exceeded')

    def test_total_json_bound_and_observed_wire_invariants_are_enforced(self):
        with patch.object(observation, 'MAX_JSON_TOTAL', 1):
            self.unavailable('limit-exceeded')
        value = self.collect()
        mutations = [dict(mainUsers=2), dict(assistants=True), dict(completedAssistants=2),
            dict(readTools=False), dict(completedReadTools=2),
            dict(completedReadTools=0), dict(finalAssistantComplete=False),
            dict(finalFinish={'stop': None}), dict(errorFamilies=value['errorFamilies'] | {'PRIVATE': 1}),
            dict(errorFamilies=value['errorFamilies'] | {'APIError': True}), dict(httpStatuses=[401]),
            dict(httpStatuses=[True]), dict(status='unavailable'), dict(reason='database-missing')]
        for mutation in mutations:
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                observation.validate(value | mutation)

    def test_query_budget_expiration_never_publishes_partial_metrics(self):
        calls = 0

        def clock():
            nonlocal calls
            calls += 1
            return 0.0 if calls == 1 else 1.0

        with patch.object(observation.time, 'monotonic', side_effect=clock):
            self.unavailable('limit-exceeded')

    def test_session_message_and_part_limits_reject_instead_of_truncating(self):
        for table, limit, insert in (
            ('session', 17, lambda index: self.session('PRIVATE_EXTRA_SESSION_' + str(index))),
            ('message', 65, lambda index: self.message('PRIVATE_EXTRA_MESSAGE_' + str(index), 'user', index + 10, agent='actor')),
            ('part', 129, lambda index: self.part('PRIVATE_EXTRA_PART_' + str(index), 'PRIVATE_USER', dict(type='snapshot'))),
        ):
            with self.subTest(table=table), self.connection() as db:
                count = db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                # Restore the previous limited table before checking the next one.
                db.execute("DELETE FROM session WHERE id LIKE 'PRIVATE_EXTRA_%'")
                db.execute("DELETE FROM message WHERE id LIKE 'PRIVATE_EXTRA_%'")
                db.execute("DELETE FROM part WHERE id LIKE 'PRIVATE_EXTRA_%'")
                count = db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
            for index in range(limit - count):
                insert(index)
            self.unavailable('limit-exceeded')


class ConsumerTests(unittest.TestCase):
    def test_unavailable_never_contains_invented_metrics_or_private_values(self):
        value = dict(schemaVersion=1, status='unavailable', reason='database-missing', **dict.fromkeys(observation.METRICS))
        self.assertEqual(observation.validate(value), value)
        for change in ({'mainUsers': 0}, {'reason': 'PRIVATE'}, {'reason': None}, {'schemaVersion': True}, {'raw': 'PRIVATE'}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                observation.validate(value | change)


if __name__ == '__main__':
    unittest.main()
