#!/usr/bin/env python3
"""Generate offline alpha masks from immutable official Zed SVG sources.

Build-only environment: CairoSVG 2.8.2 and Pillow 11.3.0.
Mask byte digests fix the Cairo 1.18.4 reference output across host libraries.
No native screenshots or application payloads are read by this generator.
"""
import hashlib
import io
import json
import os
import urllib.request
from pathlib import Path
import cairosvg
from PIL import Image
import PIL
from cairosvg import surface

assert cairosvg.__version__ == "2.8.2"
assert PIL.__version__ == "11.3.0"
cairo_version = surface.cairo.cairo_version_string()
root = Path(os.environ["NANH_ZED_ICON_TEMPLATES"]).resolve()
root.mkdir(mode=0o700, parents=True, exist_ok=True)
root.chmod(0o700)
source_hashes = {'close.svg': '698d46fcdfd3be6bec156f5223f70436a4044b871c411ac9f65abb0dd7c4b619', 'copy.svg': '91a44e5c8af0e9b0fed70c22483b55d5de5a8cbeb5682b9a0c976cefec374c8b', 'rotate_cw.svg': '981ac36ab677c57bb59909dfd0d86cf8e326f2fa282cb99da013fb0811d2f6fe'}
for name, digest in source_hashes.items():
    url = "https://raw.githubusercontent.com/zed-industries/zed/76659a55a8c10ed355a070f8764a0b1733e3c115/assets/icons/" + name
    with urllib.request.urlopen(url, timeout=10) as response:
        data = response.read(16385)
    if len(data) > 16384 or hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("source digest mismatch")
    target = root / name
    target.write_bytes(data)
    target.chmod(0o600)
manifest = {"sourceCommit": "76659a55a8c10ed355a070f8764a0b1733e3c115", "diagnosticsOnly": True,
            "rasterizer": {"CairoSVG": "2.8.2", "Pillow": "11.3.0", "Cairo": cairo_version, "referenceCairo": "1.18.4"}, "files": {}}
mask_hashes = {'close-14.alpha': 'ca9478e6d5ad466f0b930723ddeec769e2a0a818e5d48b01d990318c809be870', 'close-28.alpha': '79d36e9f7728628480497c0f559f273b4ed96d6454c9f18316729c91cfd96a56', 'copy-14.alpha': 'fd141505ee35387d2022907d97676d0e8442a7c0df7c555594e1e11f6d549495', 'copy-28.alpha': '5b8e649ba9854408f5938edd843875c5c5ef031900fff389e888ffb15619b146', 'rotate_cw-14.alpha': 'c653a9b025e2112e1b68c084da89d0f7e7e5488447f7e7072dddeae8946fdf44', 'rotate_cw-28.alpha': '81d4300229176b9070aea15f2d6758a4c6d578a51e5554d02a3d6af1001249ce'}
for name in ("rotate_cw", "copy", "close"):
    source = root / f"{name}.svg"
    manifest["files"][source.name] = hashlib.sha256(source.read_bytes()).hexdigest()
    for side in (14, 28):
        png = cairosvg.svg2png(bytestring=source.read_bytes(), output_width=side, output_height=side)
        alpha = Image.open(io.BytesIO(png)).convert("RGBA").getchannel("A").tobytes()
        target = root / f"{name}-{side}.alpha"
        if hashlib.sha256(alpha).hexdigest() != mask_hashes[target.name]:
            raise ValueError("reference mask digest mismatch")
        target.write_bytes(alpha)
        target.chmod(0o600)
        manifest["files"][target.name] = hashlib.sha256(alpha).hexdigest()
(root / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
(root / "manifest.json").chmod(0o600)
