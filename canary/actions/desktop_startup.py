"""Validate bounded, closed startup facts without publishing application stderr."""
STARTUP_CATEGORIES = set('sandbox-helper namespace-denied root-without-sandbox display-unavailable missing-library gpu-fatal native-module unclassified'.split())

def shape(value, keys, mechanism):
    if type(value) is not dict or set(value) != set(keys.split()) or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1 or value['mechanism'] != mechanism:
        raise ValueError('invalid closed experiment schema')


def flags(value, keys):
    if any(type(value[key]) is not bool for key in keys.split()):
        raise ValueError('invalid closed experiment flag')


def startup(value):
    shape(value, 'schemaVersion mechanism startupCategory namespacePolicy disableSetuidSandbox stderrPresent captureTruncated drainComplete launcherExitCode effectiveUserIsRoot apparmor_restrict_unprivileged_userns unprivileged_userns_clone sandboxHelperPresent sandboxHelperOwnerIsRoot sandboxHelperModeIs4755', 'hermes-startup')
    flags(value, 'stderrPresent captureTruncated drainComplete disableSetuidSandbox')
    if value['namespacePolicy'] not in {'default', 'scoped-apparmor-userns'} or value['disableSetuidSandbox'] and value['namespacePolicy'] != 'scoped-apparmor-userns':
        raise ValueError('invalid namespace experiment policy')
    if value['startupCategory'] not in STARTUP_CATEGORIES:
        raise ValueError('invalid startup category')
    if (value['captureTruncated'] or not value['drainComplete']) and value['startupCategory'] != 'unclassified':
        raise ValueError('classification from incomplete stderr')
    for key in ('effectiveUserIsRoot', 'sandboxHelperPresent', 'sandboxHelperOwnerIsRoot', 'sandboxHelperModeIs4755'):
        if value[key] is not None and type(value[key]) is not bool:
            raise ValueError('invalid startup fact')
    for key in ('apparmor_restrict_unprivileged_userns', 'unprivileged_userns_clone'):
        if value[key] is not None and (type(value[key]) is not int or value[key] not in (0, 1)):
            raise ValueError('invalid namespace policy')
    code = value['launcherExitCode']
    if code is not None and (type(code) is not int or not -127 <= code <= 255):
        raise ValueError('invalid launcher exit')
    return value
