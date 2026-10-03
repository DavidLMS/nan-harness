#!/usr/bin/env python3
"""Exact public Xcursor comparison; never captures an application image.

Selection follows pinned x11rb 0.13.2 / xcursor 0.3.10. Unsupported
themes, core-font cursors and malformed files fail closed.

GPUI PointingHand aliases: zed revision 76659a55a8c10ed355a070f8764a0b1733e3c115,
crates/gpui_linux/src/linux/platform.rs. The dependency implementations are
published at https://docs.rs/crate/x11rb/0.13.2/source/src/cursor/ and
https://docs.rs/crate/xcursor/0.3.10/source/src/lib.rs.
"""
import ctypes
import os
from pathlib import Path
import re
import struct
import time

PUBLIC_KEYS = {'HOME', 'XDG_DATA_HOME', 'XDG_DATA_DIRS', 'XCURSOR_PATH',
               'XCURSOR_THEME', 'XCURSOR_SIZE'}
LIMIT = 1024 * 1024


def parse_images(data, desired):
    if len(data) > LIMIT or len(data) < 16 or data[:4] != b'Xcur':
        raise ValueError('cursor format')
    header, _, count = struct.unpack_from('<III', data, 4)
    if not 16 <= header <= len(data) or not 1 <= count <= 128 or header + count * 12 > len(data):
        raise ValueError('cursor table')
    images = []
    for index in range(count):
        kind, _, offset = struct.unpack_from('<III', data, header + index * 12)
        if kind != 0xfffd0002:
            continue
        if offset + 36 > len(data):
            raise ValueError('cursor bounds')
        h, kind, size, version, width, height, xhot, yhot, _ = struct.unpack_from('<9I', data, offset)
        if (h != 36 or kind != 0xfffd0002 or version != 1 or not 1 <= size <= 512
                or not 1 <= width <= 256 or not 1 <= height <= 256
                or xhot > width or yhot > height or offset + 36 + width * height * 4 > len(data)):
            raise ValueError('cursor image')
        pixels = struct.unpack_from('<' + str(width * height) + 'I', data, offset + 36)
        images.append((size, width, height, xhot, yhot, pixels))
    if not images:
        raise ValueError('cursor empty')
    best = min(images, key=lambda image: abs(image[0] - desired))[0]
    return [image[1:] for image in images if image[0] == best]


def search_paths(env):
    home = env.get('HOME')
    def expand(value):
        if value.startswith('~/'):
            if not home:
                raise ValueError('cursor home')
            value = home + value[1:]
        path = Path(value)
        if not path.is_absolute():
            raise ValueError('cursor path')
        return path
    if env.get('XCURSOR_PATH'):
        return [expand(p) for p in env['XCURSOR_PATH'].split(':') if p]
    paths = []
    if env.get('XDG_DATA_HOME'):
        paths.append(expand(env['XDG_DATA_HOME']))
    elif home:
        paths.append(expand(home + '/.local/share/icons'))
    if home:
        paths.append(expand(home + '/.icons'))
    paths.extend(expand(p) / 'icons' for p in
                 env.get('XDG_DATA_DIRS', '/usr/local/share:/usr/share').split(':') if p)
    paths.append(Path('/usr/share/pixmaps'))
    if home:
        paths.append(expand(home + '/.cursors'))
    paths.append(Path('/usr/share/cursors/xorg-x11'))
    return paths


def public_file(path, home):
    resolved = path.resolve(strict=True)
    allowed = [Path('/usr/share'), Path('/usr/local/share')]
    if home:
        owned = Path(home)
        if (owned.is_symlink() or owned.resolve(strict=True) != owned
                or owned.stat().st_uid != os.getuid() or owned.stat().st_mode & 0o077):
            raise ValueError('cursor home identity')
        allowed.append(owned)
    if not any(resolved.is_relative_to(root) for root in allowed) or not resolved.is_file():
        raise ValueError('cursor public path')
    if resolved.stat().st_size > LIMIT:
        raise ValueError('cursor file limit')
    with resolved.open('rb') as stream:
        data = stream.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError('cursor file limit')
    return data


def find_icon(paths, theme, name, home, visited=None):
    if not re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', theme) or theme == 'core':
        raise ValueError('cursor theme')
    visited = set() if visited is None else visited
    if theme in visited or len(visited) >= 16:
        return None
    visited.add(theme)
    directories = [path / theme for path in paths if (path / theme).is_dir()]
    for directory in directories:
        candidate = directory / 'cursors' / name
        if candidate.is_file():
            return public_file(candidate, home)
    for directory in directories:
        inherits = None
        index = directory / 'index.theme'
        if index.is_file():
            text = public_file(index, home).decode('utf-8')
            for line in text.splitlines():
                match = re.match(r'^Inherits\s*=\s*[;,\s]*([^;,\s]+)', line)
                if match:
                    inherits = match[1]
                    break
        inherits = inherits or ('default' if theme != 'default' else None)
        if inherits:
            result = find_icon(paths, inherits, name, home, visited)
            if result is not None:
                return result
    return None


def expected_images(env, resources, dimensions):
    theme = env.get('XCURSOR_THEME', resources.get('Xcursor.theme'))
    size = env.get('XCURSOR_SIZE')
    if size is not None and not re.fullmatch(r'[0-9]{1,9}', size):
        size = None
    desired = int(size) if size is not None else int(resources.get('Xcursor.size', '0'))
    if not desired:
        dpi = int(resources.get('Xft.dpi', '0'))
        desired = dpi * 16 // 72 if dpi else min(dimensions) // 48
    if not 1 <= desired <= 512:
        raise ValueError('cursor size')
    paths = search_paths(env)
    for name in ('pointer', 'hand', 'hand2'):
        data = find_icon(paths, theme, name, env.get('HOME')) if theme else None
        if data is None:
            data = find_icon(paths, 'default', name, env.get('HOME'))
        if data is not None:
            return parse_images(data, desired)
    raise ValueError('cursor unavailable')


class CursorImage(ctypes.Structure):
    _fields_ = [('x', ctypes.c_short), ('y', ctypes.c_short),
                ('width', ctypes.c_ushort), ('height', ctypes.c_ushort),
                ('xhot', ctypes.c_ushort), ('yhot', ctypes.c_ushort),
                ('serial', ctypes.c_ulong), ('pixels', ctypes.POINTER(ctypes.c_ulong)),
                ('atom', ctypes.c_ulong), ('name', ctypes.c_void_p)]


class PointerShape:
    def __init__(self, pid, guard, deadline):
        self.guard, self.deadline = guard, deadline
        if not guard() or time.monotonic() >= deadline:
            raise ValueError('cursor owner')
        stat = Path('/proc') / str(pid) / 'stat'
        identity = stat.read_bytes()
        with (stat.parent / 'environ').open('rb') as stream:
            data = stream.read(65537)
        if len(data) > 65536 or not guard() or stat.read_bytes().rsplit(b')', 1)[1].split()[19] != identity.rsplit(b')', 1)[1].split()[19]:
            raise ValueError('cursor process')
        env = {}
        for entry in data.split(b'\0'):
            key, _, value = entry.partition(b'=')
            if key.decode('ascii', errors='ignore') in PUBLIC_KEYS:
                decoded = key.decode('ascii')
                if decoded in env:
                    raise ValueError('cursor duplicate')
                env[decoded] = value.decode('utf-8')
        del data
        self.process_stat = stat
        self.process_start = identity.rsplit(b')', 1)[1].split()[19]
        self.xlib = ctypes.CDLL('libX11.so.6')
        self.xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.xlib.XOpenDisplay.restype = ctypes.c_void_p
        self.xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.xlib.XFree.argtypes = [ctypes.c_void_p]
        self.xlib.XResourceManagerString.argtypes = [ctypes.c_void_p]
        self.xlib.XResourceManagerString.restype = ctypes.c_void_p
        for name in ('XDefaultScreen', 'XDisplayWidth', 'XDisplayHeight', 'XImageByteOrder'):
            function = getattr(self.xlib, name)
            function.argtypes = [ctypes.c_void_p] if name in ('XDefaultScreen', 'XImageByteOrder') else [ctypes.c_void_p, ctypes.c_int]
            function.restype = ctypes.c_int
        self.display = self.xlib.XOpenDisplay(None)
        if not self.display:
            raise OSError('cursor display')
        try:
            if self.xlib.XImageByteOrder(self.display) != 0:
                raise ValueError('cursor byte order')
            resource_pointer = self.xlib.XResourceManagerString(self.display)
            libc = ctypes.CDLL(None)
            libc.strnlen.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
            libc.strnlen.restype = ctypes.c_size_t
            length = libc.strnlen(resource_pointer, 65537) if resource_pointer else 0
            if length > 65536:
                raise ValueError('cursor resources')
            raw = ctypes.string_at(resource_pointer, length) if resource_pointer else b''
            resources = {}
            for line in raw.decode('utf-8').splitlines():
                key, separator, value = line.partition(':')
                key = key.strip()
                if key in ('Xcursor.theme', 'Xcursor.size', 'Xft.dpi'):
                    if not separator or key in resources:
                        raise ValueError('cursor resource ambiguity')
                    resources[key] = value.strip()
                elif '*' in key and key.endswith(('theme', 'size', 'dpi')):
                    raise ValueError('cursor resource ambiguity')
            screen = self.xlib.XDefaultScreen(self.display)
            dimensions = (self.xlib.XDisplayWidth(self.display, screen), self.xlib.XDisplayHeight(self.display, screen))
            self.images = expected_images(env, resources, dimensions)
            self.fix = ctypes.CDLL('libXfixes.so.3')
            self.fix.XFixesGetCursorImage.argtypes = [ctypes.c_void_p]
            self.fix.XFixesGetCursorImage.restype = ctypes.POINTER(CursorImage)
            if not guard() or time.monotonic() >= deadline:
                raise ValueError('cursor owner')
        except BaseException:
            self.close()
            raise

    def matches(self):
        if (not self.guard() or time.monotonic() >= self.deadline
                or self.process_stat.read_bytes().rsplit(b')', 1)[1].split()[19] != self.process_start):
            return False
        pointer = self.fix.XFixesGetCursorImage(self.display)
        if not pointer:
            return False
        try:
            image = pointer.contents
            if not 1 <= image.width <= 256 or not 1 <= image.height <= 256:
                return False
            actual = (image.width, image.height, image.xhot, image.yhot,
                      tuple(image.pixels[index] & 0xffffffff for index in range(image.width * image.height)))
            return actual in self.images and self.guard() and time.monotonic() < self.deadline
        finally:
            self.xlib.XFree(pointer)

    def close(self):
        if getattr(self, 'display', None):
            self.xlib.XCloseDisplay(self.display)
            self.display = None
        self.images = []
