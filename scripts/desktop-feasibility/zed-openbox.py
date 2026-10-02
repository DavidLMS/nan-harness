#!/usr/bin/env python3
"""Add a first-map policy to the disposable runner's stock Openbox configuration."""
import os
from pathlib import Path
import sys
import xml.etree.ElementTree as ET

NAMESPACE = 'http://openbox.org/3.4/rc'


def transform(payload):
    root = ET.fromstring(payload)
    if root.tag != f'{{{NAMESPACE}}}openbox_config':
        raise ValueError('invalid Openbox root')
    for section in ('keyboard', 'mouse', 'focus'):
        if len(root.findall(f'{{{NAMESPACE}}}{section}')) != 1:
            raise ValueError('missing stock policy section')
    sections = root.findall(f'{{{NAMESPACE}}}applications')
    if len(sections) != 1:
        raise ValueError('invalid application policy section')
    application = ET.SubElement(sections[0], f'{{{NAMESPACE}}}application',
                                name='dev.zed.Zed', **{'class': 'dev.zed.Zed', 'type': 'normal'})
    ET.SubElement(application, f'{{{NAMESPACE}}}maximized').text = 'yes'
    ET.register_namespace('', NAMESPACE)
    return ET.tostring(root, encoding='utf-8', xml_declaration=True)


def main():
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted' or os.environ.get('RUNNER_OS') != 'Linux':
        raise ValueError('hosted runner required')
    source, destination = map(Path, sys.argv[1:])
    if not source.is_absolute() or source.is_symlink() or not source.is_file():
        raise ValueError('invalid stock configuration')
    if not destination.is_absolute():
        raise ValueError('invalid private configuration')
    with source.open('rb') as stream:
        payload = stream.read(1048577)
    if len(payload) > 1048576:
        raise ValueError('configuration oversized')
    result = transform(payload)
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(result)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, ET.ParseError):
        sys.exit('Invalid disposable Openbox configuration.')
