import json
import os
import sys
import tempfile
import types
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

agent = types.ModuleType('agent')
provider_module = types.ModuleType('agent.image_gen_provider')
provider_module.ImageGenProvider = type('ImageGenProvider', (), {})
provider_module.success_response = lambda **kwargs: kwargs
sys.modules['agent'] = agent
sys.modules['agent.image_gen_provider'] = provider_module
native = types.ModuleType('tools.image_generation_tool')
native._read_configured_image_provider = lambda: 'nan-harness'
native.IMAGE_GENERATE_SCHEMA = {'name': 'image_generate', 'parameters': {'properties': {'prompt': {'type': 'string'}}, 'required': ['prompt']}}
registry = types.ModuleType('tools.registry')
registry.tool_error = lambda message: {'success': False, 'error': message}
tools = types.ModuleType('tools')
tools.image_generation_tool = native
sys.modules['tools'] = tools
sys.modules['tools.registry'] = registry
sys.modules['tools.image_generation_tool'] = native
namespace = {}
exec(sys.stdin.read(), namespace)

class Context:
    def register_image_gen_provider(self, provider):
        self.provider = provider

    def register_tool(self, **tool):
        assert tool['name'] == 'image_generate' and tool['override']
        self.tool = tool

ctx = Context()
namespace['register'](ctx)
assert [m['id'] for m in ctx.provider.list_models()] == ['flux-2-klein', 'qwen-image-2.1']
assert ctx.tool['schema']['parameters']['properties']['model']['enum'] == ['flux-2-klein', 'qwen-image-2.1']
assert 'model' not in native.IMAGE_GENERATE_SCHEMA['parameters']['properties']

def native_handler(args, **kwargs):
    assert kwargs['task_id'] == 'native-task'
    if args['prompt'] == 'raise':
        raise RuntimeError('synthetic native failure')
    # The native handler supplies its configured model, not the per-call override.
    return ctx.provider.generate(args['prompt'], model='flux-2-klein',
                                 reference_image_urls=args.get('reference_image_urls'))

native._handle_image_generate = native_handler
calls = []
def run_media(command, **kwargs):
    assert kwargs['env']['NAN_MEDIA_API_KEY'] == 'synthetic-key'
    calls.append(command)
    Path(command[command.index('--output') + 1]).write_bytes(b'synthetic-image')
    return types.SimpleNamespace(returncode=0)

with tempfile.TemporaryDirectory() as home, patch.dict(os.environ, {'HERMES_HOME': home, 'NAN_MEDIA_API_KEY': 'synthetic-key'}, clear=True), patch.object(namespace['subprocess'], 'run', side_effect=run_media):
    assert ctx.provider.generate('default')['model'] == sys.argv[1]
    handler = ctx.tool['handler']
    for model in ('flux-2-klein', 'qwen-image-2.1'):
        result = handler({'prompt': 'explicit', 'model': model}, task_id='native-task')
        assert result['model'] == model
        assert Path(result['image']).read_bytes() == b'synthetic-image'
        assert calls[-1][calls[-1].index('--model') + 1] == model
    assert handler({'prompt': 'native default'}, task_id='native-task')['model'] == 'flux-2-klein'
    count = len(calls)
    assert not handler({'prompt': 'invalid', 'model': 'text-model'}, task_id='native-task')['success']
    assert not handler({'prompt': 'edit', 'model': 'qwen-image-2.1', 'reference_image_urls': ['reference.png']}, task_id='native-task')['success']
    assert len(calls) == count
    try:
        handler({'prompt': 'raise', 'model': 'qwen-image-2.1'}, task_id='native-task')
    except RuntimeError:
        pass
    else:
        raise AssertionError('native failure must propagate')
    assert namespace['_request_model'].get() is None
    def generate(model):
        return handler({'prompt': 'parallel', 'model': model}, task_id='native-task')['model']
    with ThreadPoolExecutor(max_workers=2) as pool:
        models = ['flux-2-klein', 'qwen-image-2.1'] * 4
        assert list(pool.map(generate, models)) == models
    reference = Path(home) / 'reference.png'
    reference.write_bytes(b'reference')
    handler({'prompt': 'edit', 'model': 'flux-2-klein', 'reference_image_urls': [str(reference)]}, task_id='native-task')
    assert '--input-image' in calls[-1]

# Keeping the plugin installed must not replace another selected provider's tool.
native._read_configured_image_provider = lambda: 'external'
external_ctx = Context()
namespace['register'](external_ctx)
assert not hasattr(external_ctx, 'tool')
