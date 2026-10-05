"""Reduce pinned GPUI numeric hitbox observations without exporting geometry.

The caller pins the exact official ELF before installing probes. Temporary
maps contain only numeric rectangles, masks and behavior flags, never text,
object identifiers, pointers or stacks. Only classifications leave the runner.
"""
import json
import math
import os
from pathlib import Path
import re
import stat
import struct

MAX_HITBOXES = 1024
MAX_OUTPUT = 768 * 1024


CHUNK_SIZE = 64
CHUNKS = range(MAX_HITBOXES // CHUNK_SIZE)


def seeds():
    return ' '.join(f'@geometrySlot{chunk} = 0;' for chunk in CHUNKS)


def cleanup():
    return ' '.join(f'delete(@geometrySlot{chunk});' for chunk in CHUNKS)


def probe(executable, symbol):
    # Verified in the pinned ELF: rendered Vec at +592/+600, last pointer at
    # +7840/+7844, viewport at +6416/+6420. Hitbox stride is 48 bytes, excluding
    # the object ID at +0. Independent straight-line programs avoid asking the
    # kernel verifier to explore a 1024-iteration loop with helper-error branches.
    fields = ', '.join(f'*(uint32*)uptr($box + {offset})' for offset in range(8, 40, 4))
    programs = []
    for chunk in CHUNKS:
        header = ('$x = *(uint32*)uptr($window + 7840); $y = *(uint32*)uptr($window + 7844); '
                  '$vw = *(uint32*)uptr($window + 6416); $vh = *(uint32*)uptr($window + 6420); '
                  '@geometryHeaders[@slot, $n, $x, $y, $vw, $vh] = count(); ') if chunk == 0 else ''
        programs.append(
            f'uprobe:{executable}:{symbol} /@active == 1 && @slot >= 1 && @slot <= 3/ {{ '
            f'if (@geometrySlot{chunk} != @slot) {{ @geometrySlot{chunk} = @slot; '
            '$window = arg0; $n = *(uint64*)uptr($window + 600); ' + header +
            f'if ($n > 0 && $n <= {MAX_HITBOXES}) {{ '
            f'$base = *(uint64*)uptr($window + 592); $i = {chunk * CHUNK_SIZE}; '
            f'unroll({CHUNK_SIZE}) {{ if ($i < $n) {{ $box = $base + $i * 48; '
            f'@geometryRects[@slot, $i, {fields}, *(uint8*)uptr($box + 40)] = count(); '
            '} $i = $i + 1; } } } }')
    return '\n'.join(programs)


def split_maps(data):
    """Keep the original counter parser isolated from private numeric maps."""
    if len(data) > MAX_OUTPUT:
        raise ValueError('geometry output budget')
    maps, remaining = {}, []
    for line in data.splitlines():
        if not line.strip():
            continue
        record = json.loads(line)
        if type(record) is dict and record.get('type') == 'helper_error':
            if record.get('helper') == 'probe_read_user' and record.get('retcode') == -14:
                raise ValueError('geometry user read fault')
            if record.get('helper') == 'probe_read_user_str' and record.get('retcode') == -14:
                raise ValueError('geometry marker read fault')
            raise ValueError('geometry helper failure')
        if type(record) is dict and record.get('type') == 'lost_events':
            raise ValueError('geometry lost events')
        if (type(record) is not dict or record.get('type') != 'map'
                or type(record.get('data')) is not dict or len(record['data']) != 1):
            raise ValueError('unexpected geometry output')
        key, value = next(iter(record['data'].items()))
        if key not in {'@geometryHeaders', '@geometryRects'}:
            remaining.append(line)
            continue
        if key in maps or type(value) is not dict:
            raise ValueError('duplicate geometry map')
        size = 6 if key == '@geometryHeaders' else 11
        if len(value) > (3 if size == 6 else 3 * MAX_HITBOXES):
            raise ValueError('geometry map budget')
        entries = []
        for encoded, count in value.items():
            if (type(count) is not int or count != 1 or type(encoded) is not str
                    or len(encoded) > 160 or not re.fullmatch(r'[0-9]+(?:,[0-9]+)*', encoded)):
                raise ValueError('invalid geometry tuple')
            numbers = tuple(map(int, encoded.split(',')))
            if len(numbers) != size or not 1 <= numbers[0] <= 3 or any(n > 0xffffffff for n in numbers):
                raise ValueError('invalid geometry fields')
            entries.append(numbers)
        maps[key] = entries
    return b'\n'.join(remaining), maps


def number(bits):
    if type(bits) is not int or not 0 <= bits <= 0xffffffff:
        raise ValueError('invalid geometry number')
    value = struct.unpack('<f', struct.pack('<I', bits))[0]
    if not math.isfinite(value) or abs(value) > 1048576:
        raise ValueError('unbounded geometry number')
    return value


def rectangle(values):
    result = tuple(number(value) for value in values)
    if len(result) != 4 or result[2] < 0 or result[3] < 0:
        raise ValueError('invalid geometry rectangle')
    return result


def contains(rect, point):
    x, y, width, height = rect
    return x <= point[0] < x + width and y <= point[1] < y + height


def validate_target(target):
    if (type(target) is not dict or set(target) != {'point', 'bounds', 'viewport'}
            or any(type(target[key]) is not list or len(target[key]) != size
                   for key, size in (('point', 2), ('bounds', 4), ('viewport', 2)))
            or any(type(n) is not int or not -65536 <= n <= 65536
                   for key in target for n in target[key])
            or target['bounds'][2] <= 0 or target['bounds'][3] <= 0
            or any(n <= 0 for n in target['viewport'])
            or not contains(target['bounds'], target['point'])):
        raise ValueError('invalid owned geometry target')


def save_target(directory, target):
    validate_target(target)
    directory = Path(directory)
    info = directory.stat()
    if (not directory.is_absolute() or directory.resolve() != directory
            or not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077):
        raise ValueError('invalid geometry marker directory')
    for slot in range(1, 4):
        try:
            descriptor = os.open(directory / f'target-{slot}.json', os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            continue
        with os.fdopen(descriptor, 'w') as output:
            json.dump(target, output, separators=(',', ':'))
        return
    raise ValueError('geometry target budget')


def classify(target, pointer, boxes, viewport):
    """Replay normal-mouse clipping/occlusion at the independently owned point.

    A geometric match is not an element-identity proof. Multiple matching
    rectangles remain ambiguous; never select one by paint order.
    """
    validate_target(target)
    if len(viewport) != 2 or any(not math.isfinite(n) or n <= 0 for n in viewport):
        raise ValueError('invalid rendered viewport')
    scales = [a / b for a, b in zip(target['viewport'], viewport)]
    if any(not 0.25 <= scale <= 8 for scale in scales) or abs(scales[0] - scales[1]) > 0.002:
        raise ValueError('inconsistent coordinate scale')
    # Accessible extents and X11 client geometry use physical pixels; GPUI
    # hitboxes use logical pixels. The viewport ratio binds these spaces.
    scale = sum(scales) / 2
    if not 0 < len(boxes) <= MAX_HITBOXES:
        raise ValueError('invalid hitbox census')
    point = [n / scale for n in target['point']]
    target_bounds = [n / scale for n in target['bounds']]
    matches = [index for index, (bounds, _, _) in enumerate(boxes)
               if all(abs(a - b) <= 1 / scale for a, b in zip(bounds, target_bounds))]
    result = dict(status='matched' if len(matches) == 1 else 'absent' if not matches else 'ambiguous',
                  renderedHitboxes=len(boxes), boundsMatches=len(matches),
                  priorPointerMatches=all(abs(a - b) <= 1 / scale for a, b in zip(pointer, point)),
                  targetMaskContainsPoint=None, blockingHitboxesAhead=None, targetWouldBeHovered=None)
    if len(matches) == 1:
        index = matches[0]
        bounds, mask, _ = boxes[index]
        visible = contains(bounds, point) and contains(mask, point)
        blockers = sum(behavior != 0 and contains(bounds, point) and contains(mask, point)
                       for bounds, mask, behavior in boxes[index + 1:])
        result.update(targetMaskContainsPoint=visible, blockingHitboxesAhead=blockers,
                      targetWouldBeHovered=visible and blockers == 0)
    return result


def observations(maps, markers, started):
    headers, rows = {}, {}
    for slot, count, x, y, width, height in maps.get('@geometryHeaders', []):
        if slot in headers or not 1 <= count <= MAX_HITBOXES:
            raise ValueError('invalid geometry header')
        headers[slot] = (count, (number(x), number(y)), (number(width), number(height)))
    for slot, index, *values in maps.get('@geometryRects', []):
        if index >= MAX_HITBOXES or (slot, index) in rows or values[-1] not in (0, 1, 2):
            raise ValueError('invalid geometry row')
        rows[slot, index] = (rectangle(values[:4]), rectangle(values[4:8]), values[8])
    if set(headers) != set(range(1, started + 1)):
        raise ValueError('incomplete geometry frames')
    if len(rows) != sum(count for count, _, _ in headers.values()):
        raise ValueError('incomplete geometry rows')
    results = []
    for slot, (count, pointer, viewport) in sorted(headers.items()):
        path = Path(markers) / f'target-{slot}.json'
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 256:
            raise ValueError('invalid geometry target file')
        target = json.loads(path.read_bytes())
        try:
            boxes = [rows[slot, index] for index in range(count)]
        except KeyError:
            raise ValueError('missing geometry row') from None
        results.append(classify(target, pointer, boxes, viewport))
    return results
