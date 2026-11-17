import sys
import types

providers = types.ModuleType("providers")
base = types.ModuleType("providers.base")
class ProviderProfile:
    def __init__(self, **kwargs):
        self.__dict__.update(kwargs)
providers.register_provider = lambda provider: None
base.ProviderProfile = ProviderProfile
sys.modules["providers"] = providers
sys.modules["providers.base"] = base
scope = {}
exec(sys.stdin.read(), scope)
profile = scope["nan"]
for model in ("qwen3.6", "gemma4", "glm5.3-flash"):
    for effort in ("low", "medium", "high", "max"):
        assert profile.build_api_kwargs_extras(model=model, reasoning_config={"effort": effort}) == ({}, {"reasoning_effort": effort})
    assert profile.build_api_kwargs_extras(model=model) == ({}, {})
for model in ("qwen3.6", "gemma4"):
    assert profile.build_api_kwargs_extras(model=model, reasoning_config={"enabled": False}) == ({}, {"reasoning_effort": "none"})
for model in ("glm5.3-flash", "deepseek-v4-flash", "qwen3.8-flash", "unknown"):
    assert profile.build_api_kwargs_extras(model=model, reasoning_config={"enabled": False}) == ({}, {})
for effort in ("low", "max"):
    assert profile.build_api_kwargs_extras(model="deepseek-v4-flash", reasoning_config={"effort": effort}) == ({}, {})
assert profile.build_api_kwargs_extras(model="mimo-v2.6-flash", reasoning_config={"enabled": False}) == ({"chat_template_kwargs": {"enable_thinking": False}}, {})
assert "quoted-'\"-model" in profile.fetch_models()
