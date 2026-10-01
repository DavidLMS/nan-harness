import os
import subprocess
import tempfile
import uuid
from contextvars import ContextVar
from pathlib import Path

from agent.image_gen_provider import ImageGenProvider, success_response


BASE_URL = __NAN_BASE_URL__
DEFAULT_IMAGE_MODEL = __NAN_IMAGE_MODEL__

_request_model = ContextVar("nan_image_model", default=None)
IMAGE_MODELS = ("flux-2-klein", "qwen-image-2.1")


class NanHarnessImageProvider(ImageGenProvider):
    name = "nan-harness"
    display_name = "NaN Images"
    def capabilities(self):
        return {"modalities": ["text", "image"], "max_reference_images": 4}

    def is_available(self):
        if os.getenv("NAN_MEDIA_API_KEY", "").strip():
            return True
        try:
            return subprocess.run(
                ["nanh", "__media", "credentials"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
            ).returncode == 0
        except OSError:
            return False

    def list_models(self):
        return [{"id": "flux-2-klein", "name": "Flux 2 Klein"},
                {"id": "qwen-image-2.1", "name": "Qwen Image 2.1"}]

    def generate(self, prompt, aspect_ratio="landscape", *, image_url=None,
                 reference_image_urls=None, model=None, **_kwargs):
        model = _request_model.get() or model or DEFAULT_IMAGE_MODEL
        if model not in IMAGE_MODELS:
            return {"success": False, "error": "Choose flux-2-klein or qwen-image-2.1."}
        references = ([image_url] if image_url else []) + list(reference_image_urls or [])
        if model == "qwen-image-2.1" and references:
            return {"success": False, "error": "Qwen Image supports generation only. Choose flux-2-klein to edit images."}
        with tempfile.TemporaryDirectory(prefix="nanh-image-") as directory:
            output = Path(directory) / "image.png"
            command = [
                "nanh", "__media", "image", "--provider-base-url", BASE_URL,
                "--model", str(model), "--prompt", str(prompt), "--output", str(output)
            ]
            for index, image in enumerate(references):
                reference = Path(image)
                if isinstance(image, str) and image.startswith(("http://", "https://")):
                    import urllib.request
                    reference = Path(directory) / f"reference-{index}.bin"
                    with urllib.request.urlopen(image, timeout=60) as response:
                        data = response.read(25 * 1024 * 1024 + 1)
                        if len(data) > 25 * 1024 * 1024:
                            return {"success": False, "error": "NH-MEDIA-REFERENCE"}
                        reference.write_bytes(data)
                if reference.exists():
                    command.extend(["--input-image", str(reference)])
            completed = subprocess.run(
                command, env=os.environ.copy(),
                capture_output=True, text=True, check=False
            )
            if completed.returncode != 0 or not output.is_file():
                return {"success": False, "error": "NH-MEDIA-IMAGE"}
            hermes_home = os.getenv("HERMES_HOME")
            image_directory = Path(hermes_home) if hermes_home else Path.home() / ".hermes"
            image_directory = image_directory / "cache" / "images"
            image_directory.mkdir(parents=True, exist_ok=True)
            image_path = image_directory / f"nan-image-{uuid.uuid4().hex}.png"
            image_path.write_bytes(output.read_bytes())
            return success_response(
                image=str(image_path), model=str(model), prompt=str(prompt),
                aspect_ratio=str(aspect_ratio), provider=self.name,
                modality="image" if references else "text",
            )


def register(ctx):
    from copy import deepcopy
    from tools import image_generation_tool as native

    provider = NanHarnessImageProvider()
    ctx.register_image_gen_provider(provider)
    if native._read_configured_image_provider() != "nan-harness":
        return
    schema = deepcopy(native.IMAGE_GENERATE_SCHEMA)
    schema["description"] = "Generate NaN images. Choose a model when the user requests one; otherwise omit model to use the configured default. Only flux-2-klein supports image editing and references."
    properties = schema["parameters"]["properties"]
    properties["model"] = {"type": "string", "enum": list(IMAGE_MODELS),
        "description": "flux-2-klein for fast generation and editing; qwen-image-2.1 for realistic generation."}
    properties["image_url"] = {"type": "string", "description": "Source image URL or absolute path. Requires flux-2-klein."}
    properties["reference_image_urls"] = {"type": "array", "items": {"type": "string"}, "maxItems": 4,
        "description": "Reference image URLs or absolute paths. Requires flux-2-klein."}

    def generate(args, **kwargs):
        model = args.get("model")
        if model is not None and model not in IMAGE_MODELS:
            from tools.registry import tool_error
            return tool_error("Choose flux-2-klein or qwen-image-2.1.")
        token = _request_model.set(model)
        try:
            # Keep Hermes reference confinement, result delivery, and task handling.
            return native._handle_image_generate(args, **kwargs)
        finally:
            _request_model.reset(token)

    ctx.register_tool(name="image_generate", toolset="image_gen", schema=schema,
                      handler=generate, check_fn=provider.is_available, override=True)
