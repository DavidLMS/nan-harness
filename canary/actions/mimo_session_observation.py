#!/usr/bin/env python3
"""Read only the source-pinned MiMo store in one fresh owned probe workspace."""
import json
import math
import os
from pathlib import Path
import sqlite3
import stat
import time

ERRORS = frozenset(('ProviderAuthError', 'UnknownError', 'MessageOutputLengthError',
    'MessageAbortedError', 'StructuredOutputError', 'ContextOverflowError', 'InvalidOutputError',
    'TextToolCallError', 'ContentFilterError', 'ModelError', 'APIError', 'other'))
REASONS = frozenset(('env-unbound', 'path-unbound', 'database-missing', 'database-unavailable',
    'schema-invalid', 'limit-exceeded', 'session-unbound', 'session-ambiguous', 'data-invalid'))
COUNTS = frozenset(('mainUsers', 'assistants', 'completedAssistants', 'readTools', 'completedReadTools'))
FLAGS = frozenset(('readOutputMarker', 'finalAssistantComplete', 'finalReadMarker', 'finalCompletionMarker'))
METRICS = COUNTS | FLAGS | {'finalFinish', 'errorFamilies', 'httpStatuses'}
MAX_DATABASE = 16 * 1024 * 1024
MAX_JSON = 65536
MAX_JSON_TOTAL = 1024 * 1024


class Unavailable(ValueError):
    def __init__(self, reason):
        self.reason = reason


def require(condition, reason='data-invalid'):
    if not condition:
        raise Unavailable(reason)


def integer(value, maximum):
    return type(value) is int and 0 <= value <= maximum


def validate(value):
    require(type(value) is dict and set(value) == METRICS | {'schemaVersion', 'status', 'reason'})
    require(type(value['schemaVersion']) is int and value['schemaVersion'] == 1)
    require(type(value['status']) is str and value['status'] in ('observed', 'unavailable'))
    if value['status'] == 'unavailable':
        require(type(value['reason']) is str and value['reason'] in REASONS)
        require(all(value[key] is None for key in METRICS))
        return value
    require(value['reason'] is None)
    require(all(integer(value[key], 128 if 'ReadTools' in key or key == 'readTools' else 64) for key in COUNTS))
    require(value['mainUsers'] == 1 and value['completedAssistants'] <= value['assistants'])
    require(value['completedReadTools'] <= value['readTools'])
    require(all(type(value[key]) is bool for key in FLAGS))
    require(not value['readOutputMarker'] or value['completedReadTools'] > 0)
    require(not (value['finalReadMarker'] or value['finalCompletionMarker']) or value['finalAssistantComplete'])
    require(not value['finalAssistantComplete'] or value['completedAssistants'] > 0)
    require(type(value['finalFinish']) is str and value['finalFinish'] in ('none', 'stop', 'tool-calls', 'length', 'other'))
    families = value['errorFamilies']
    require(type(families) is dict and set(families) == ERRORS)
    require(all(integer(count, 64) for count in families.values()) and sum(families.values()) <= value['assistants'])
    statuses = value['httpStatuses']
    require(type(statuses) is list and len(statuses) <= families['APIError'])
    require(all(type(code) is int and 100 <= code <= 599 for code in statuses))
    require(statuses == sorted(set(statuses)))
    if value['assistants'] == 0:
        require(value['finalFinish'] == 'none' and value['readTools'] == 0)
    return value


def owned_path(workspace):
    require(isinstance(workspace, Path) and workspace.is_absolute() and '..' not in workspace.parts, 'path-unbound')
    require(not os.environ.get('MIMOCODE_DB'), 'env-unbound')
    require(os.environ.get('MIMOCODE_HOME') == str(workspace / 'mimo'), 'env-unbound')
    require(not os.environ.get('MIMOCODE_DISABLE_CHANNEL_DB'), 'env-unbound')
    root = workspace.lstat()
    require(stat.S_ISDIR(root.st_mode) and root.st_uid == os.geteuid() and not root.st_mode & 0o077, 'path-unbound')
    # System ancestors may be aliases (/var -> /private/var on macOS). The owned
    # workspace and every native child below it must themselves be non-links.
    physical = workspace.resolve(strict=True)
    for path in (workspace / 'mimo', workspace / 'mimo/data'):
        info = path.lstat()
        require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.geteuid(), 'path-unbound')
        require(path.resolve().is_relative_to(physical), 'path-unbound')
    database = workspace / 'mimo/data/mimocode.db'
    sizes = {}
    for suffix in ('', '-wal', '-shm'):
        path = Path(str(database) + suffix)
        try:
            info = path.lstat()
        except FileNotFoundError:
            require(bool(suffix), 'database-missing')
            continue
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid() and info.st_nlink == 1, 'path-unbound')
        require(info.st_size <= MAX_DATABASE, 'limit-exceeded')
        sizes[suffix] = info.st_size
    require(sizes.get('', 0) + sizes.get('-wal', 0) <= MAX_DATABASE, 'limit-exceeded')
    # A read-only WAL connection must not create a missing shared-memory file.
    require(not sizes.get('-wal') or '-shm' in sizes, 'database-unavailable')
    return database, physical


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def decode(raw):
    require(type(raw) is str)
    return json.loads(raw, object_pairs_hook=unique_object,
        parse_constant=lambda _value: require(False))


def snapshot(database):
    connection = sqlite3.connect(database.as_uri() + '?mode=ro', uri=True, timeout=0.2)
    deadline = time.monotonic() + 0.2
    instructions = 0

    def expired():
        nonlocal instructions
        instructions += 1000
        return instructions > 1000000 or time.monotonic() >= deadline

    connection.set_progress_handler(expired, 1000)
    try:
        connection.execute('PRAGMA query_only=ON')
        connection.execute('BEGIN')
        for table, required in (
            ('session', {'id', 'parent_id', 'directory'}),
            ('message', {'id', 'session_id', 'agent_id', 'time_created', 'data'}),
            ('part', {'id', 'session_id', 'message_id', 'data'}),
        ):
            columns = {row[1] for row in connection.execute(f'PRAGMA table_info("{table}")')}
            require(required <= columns, 'schema-invalid')
        sessions = connection.execute('SELECT id,parent_id,directory FROM session LIMIT 17').fetchall()
        require(len(sessions) <= 16, 'limit-exceeded')
        sizes = []
        for table, limit in (('message', 65), ('part', 129)):
            rows = connection.execute(f'SELECT length(CAST(data AS BLOB)) FROM {table} LIMIT {limit}').fetchall()
            require(len(rows) < limit, 'limit-exceeded')
            require(all(integer(row[0], MAX_JSON) for row in rows), 'limit-exceeded')
            sizes.extend(row[0] for row in rows)
        require(sum(sizes) <= MAX_JSON_TOTAL, 'limit-exceeded')
        messages = connection.execute('SELECT id,session_id,agent_id,time_created,data FROM message ORDER BY time_created,id LIMIT 65').fetchall()
        parts = connection.execute('SELECT id,session_id,message_id,data FROM part ORDER BY message_id,id LIMIT 129').fetchall()
        require(time.monotonic() < deadline, 'limit-exceeded')
        return sessions, messages, parts
    finally:
        connection.close()


def records(rows):
    sessions, message_rows, part_rows = rows
    require(all(type(row[0]) is str and type(row[2]) is str for row in sessions))
    session_ids = {row[0] for row in sessions}
    require(len(session_ids) == len(sessions))
    messages = {}
    for identity, session, agent, created, raw in message_rows:
        require(type(identity) is str and identity not in messages and session in session_ids)
        require(type(agent) is str and type(created) is int)
        data = decode(raw)
        require(type(data) is dict and data.get('role') in ('user', 'assistant'))
        messages[identity] = dict(session=session, agent=agent, created=created, data=data, parts=[])
    seen = set()
    for identity, session, message, raw in part_rows:
        require(type(identity) is str and identity not in seen and message in messages)
        seen.add(identity)
        require(session == messages[message]['session'])
        data = decode(raw)
        require(type(data) is dict and type(data.get('type')) is str)
        messages[message]['parts'].append(data)
    return sessions, messages


def ordinary_text(parts):
    result = []
    for part in parts:
        if part['type'] != 'text':
            continue
        require(type(part.get('text')) is str)
        require(all(key not in part or type(part[key]) is bool for key in ('ignored', 'synthetic')))
        if not part.get('ignored') and not part.get('synthetic'):
            result.append(part['text'])
    return result


def principal(sessions, messages, directories, prompt):
    candidates = []
    for session, parent, directory in sessions:
        if parent is not None or directory not in directories:
            continue
        users = [(identity, message) for identity, message in messages.items()
                 if message['session'] == session and message['agent'] == 'main' and message['data']['role'] == 'user']
        matched = [(identity, message) for identity, message in users if ordinary_text(message['parts']) == [prompt]]
        require(len(matched) <= 1, 'session-ambiguous')
        if matched:
            require(len(users) == 1, 'session-ambiguous')
            candidates.append((session, matched[0][0], len(users)))
    require(candidates, 'session-unbound')
    require(len(candidates) == 1, 'session-ambiguous')
    session, user, count = candidates[0]
    assistants = [(identity, message) for identity, message in messages.items()
                  if message['session'] == session and message['agent'] == 'main'
                  and message['data']['role'] == 'assistant' and message['data'].get('parentID') == user]
    assistants.sort(key=lambda pair: (pair[1]['created'], pair[0]))
    return count, [message for _identity, message in assistants]


def completed(message):
    timing = message.get('time')
    require(type(timing) is dict and 'created' in timing)
    for field in ('created', 'completed'):
        if field in timing:
            value = timing[field]
            require(type(value) in (int, float) and math.isfinite(value))
    return 'completed' in timing


def project(count, assistants, marker):
    result = dict(mainUsers=count, assistants=len(assistants), completedAssistants=0,
        readTools=0, completedReadTools=0, readOutputMarker=False,
        finalAssistantComplete=False, finalReadMarker=False, finalCompletionMarker=False,
        finalFinish='none', errorFamilies={key: 0 for key in sorted(ERRORS)}, httpStatuses=[])
    statuses = set()
    for message in assistants:
        data = message['data']
        result['completedAssistants'] += int(completed(data))
        if 'error' in data:
            error = data['error']
            require(type(error) is dict and type(error.get('name')) is str)
            name = error['name'] if error['name'] in ERRORS else 'other'
            result['errorFamilies'][name] += 1
            if name == 'APIError':
                require(type(error.get('data')) is dict)
                if 'statusCode' in error['data']:
                    status = error['data']['statusCode']
                    require(type(status) is int and 100 <= status <= 599)
                    statuses.add(status)
        for part in message['parts']:
            if part['type'] != 'tool' or part.get('tool') != 'read':
                continue
            state = part.get('state')
            require(type(state) is dict and state.get('status') in ('pending', 'running', 'completed', 'error'))
            result['readTools'] += 1
            if state['status'] == 'completed':
                require(type(state.get('output')) is str)
                result['completedReadTools'] += 1
                result['readOutputMarker'] |= marker in state['output']
    result['httpStatuses'] = sorted(statuses)
    if assistants:
        latest = assistants[-1]['data']
        finish = latest.get('finish')
        require('finish' not in latest or type(finish) is str)
        result['finalFinish'] = finish if finish in ('stop', 'tool-calls', 'length') else 'none' if finish is None else 'other'
        terminal = completed(latest) and 'error' not in latest and bool(finish) and finish not in ('length', 'tool-calls')
        result['finalAssistantComplete'] = bool(terminal)
        if terminal:
            text = ''.join(ordinary_text(assistants[-1]['parts']))
            result['finalReadMarker'] = marker in text
            result['finalCompletionMarker'] = 'NAN_CANARY_OK' in text
    return result


def require_task(assistants, workspace, marker):
    """Admit only a fully settled principal turn reading this probe's target."""
    require(bool(assistants))
    read = False
    for message in assistants:
        data = message['data']
        require(completed(data) and 'error' not in data)
        require(data['time']['completed'] >= data['time']['created'])
        for part in message['parts']:
            if part['type'] == 'text':
                require(type(part.get('text')) is str and 'NH-BRIDGE-' not in part['text'])
            if part['type'] != 'tool':
                continue
            state = part.get('state')
            require(type(state) is dict and state.get('status') == 'completed')
            output = state.get('output')
            require(type(output) is str and 'NH-BRIDGE-' not in output)
            if part.get('tool') != 'read':
                continue
            arguments = state.get('input')
            require(type(arguments) is dict and type(arguments.get('file_path')) is str)
            target = Path(arguments['file_path'])
            if not target.is_absolute():
                target = workspace / target
            targets = {workspace / 'read-target.txt', workspace.resolve() / 'read-target.txt'}
            read |= target in targets and marker in output
    require(read)
    result = project(1, assistants, marker)
    require(result['finalFinish'] == 'stop' and all(result[key] for key in FLAGS))


def collect(workspace: Path, marker: str, prompt: str, *, require_live_task=False):
    result = dict(schemaVersion=1, status='unavailable', reason='data-invalid', **dict.fromkeys(METRICS))
    try:
        require(type(marker) is str and 0 < len(marker) <= 256 and marker.isascii())
        require(type(prompt) is str and 0 < len(prompt) <= MAX_JSON)
        database, physical = owned_path(workspace)
        root_identity = workspace.lstat()
        identity = database.lstat()
        sessions, messages = records(snapshot(database))
        count, assistants = principal(sessions, messages, {str(workspace), str(physical)}, prompt)
        if require_live_task:
            require_task(assistants, workspace, marker)
        current = database.lstat()
        require((current.st_dev, current.st_ino, current.st_size, current.st_mtime_ns) ==
                (identity.st_dev, identity.st_ino, identity.st_size, identity.st_mtime_ns), 'path-unbound')
        root_current = workspace.lstat()
        require((root_current.st_dev, root_current.st_ino) ==
                (root_identity.st_dev, root_identity.st_ino), 'path-unbound')
        result.update(project(count, assistants, marker), status='observed', reason=None)
    except Unavailable as error:
        result['reason'] = error.reason
    except sqlite3.Error as error:
        result['reason'] = 'limit-exceeded' if getattr(error, 'sqlite_errorcode', None) == sqlite3.SQLITE_INTERRUPT else 'database-unavailable'
    except OSError:
        result['reason'] = 'database-unavailable'
    except (ValueError, TypeError, RecursionError, OverflowError):
        pass
    return validate(result)
