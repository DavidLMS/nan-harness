"""Count pinned overlay renderer entries, never arguments or text.

Totals describe the whole capture; activation windows describe only the marked
Retry dispatch interval. Neither count proves that a layer owns the occluder.
"""
import json

SYMBOLS = {'notification': '_RNvXs5_NtNtCsiUBGiTlBjqq_9workspace13notifications27simple_message_notificationNtB5_19MessageNotificationNtNtCs1R78ycJoit4_4gpui7element6Render6render', 'commandPalette': '_RNvXs2_Cs1JTbeO7la4H_15command_paletteNtB5_14CommandPaletteNtNtCs1R78ycJoit4_4gpui7element6Render6render', 'fallbackPrompt': '_RNvXs0_NtNtCs1R78ycJoit4_4gpui6window7promptsNtB5_22FallbackPromptRendererNtNtB9_7element6Render6render', 'contextMenu': '_RNvXs8_NtNtCsl3JUc7Y4cqq_2ui10components12context_menuNtB5_11ContextMenuNtNtCs1R78ycJoit4_4gpui7element6Render6render', 'zedPrompt': '_RNvXs_Cs4f7hRN5AQlc_9ui_promptNtB4_17ZedPromptRendererNtNtCs1R78ycJoit4_4gpui7element6Render6render', 'whichKey': '_RNvXs_NtCs5NjnPrxLavC_9which_key15which_key_modalNtB4_13WhichKeyModalNtNtCs1R78ycJoit4_4gpui7element6Render6render', 'securityModal': '_RNvXs1_NtCsiUBGiTlBjqq_9workspace14security_modalNtB5_13SecurityModalNtNtCs1R78ycJoit4_4gpui7element6Render6render'}


def seeds():
    # One indexed map keeps the complete BEGIN below Linux MAX_USED_MAPS=64.
    return ' '.join(f'@renders[{index}, {slot}] = count();' for index in range(7) for slot in range(4))


def probes(executable):
    lines = []
    for index, symbol in enumerate(SYMBOLS.values()):
        body = f'@renders[{index}, 0] = count(); '
        body += ' '.join(f'if (@active == 1 && @slot == {slot}) {{ @renders[{index}, {slot}] = count(); }}'
                         for slot in range(1, 4))
        lines.append(f'uprobe:{executable}:{symbol} {{ {body} }}')
    return '\n'.join(lines)


def split_counts(data):
    if len(data) > 16384:
        raise ValueError('render count budget')
    expected = {f'{index},{slot}': (key, slot) for index, key in enumerate(SYMBOLS) for slot in range(4)}
    counts, remaining = {}, []
    found = False
    for line in data.splitlines():
        if not line.strip():
            continue
        record = json.loads(line)
        if (type(record) is not dict or record.get('type') != 'map'
                or type(record.get('data')) is not dict or len(record['data']) != 1):
            raise ValueError('invalid render count record')
        key, value = next(iter(record['data'].items()))
        if key != '@renders':
            remaining.append(line)
            continue
        if found or type(value) is not dict or set(value) != set(expected):
            raise ValueError('invalid render count')
        found = True
        for item, count in value.items():
            if type(count) is not int or not 1 <= count <= 65537:
                raise ValueError('invalid render count')
            counts[item] = count - 1
    if set(counts) != set(expected):
        raise ValueError('incomplete render counts')
    totals = {key: counts[f'{index},0'] for index, key in enumerate(SYMBOLS)}
    windows = [{key: counts[f'{index},{slot}'] for index, key in enumerate(SYMBOLS)} for slot in range(1, 4)]
    if any(sum(window[key] for window in windows) > totals[key] for key in SYMBOLS):
        raise ValueError('inconsistent render counts')
    return b'\n'.join(remaining), dict(totals=totals, windows=windows)
