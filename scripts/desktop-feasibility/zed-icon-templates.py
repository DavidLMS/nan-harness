#!/usr/bin/env python3
"""Generate offline alpha masks from immutable official Zed SVG sources.

Linux: pinned build-only resvg with GPUI sampling and One Dark shader correction.
Other platforms retain CairoSVG 2.8.2 and Pillow 11.3.0.
Mask byte digests fix the Cairo 1.18.4 reference output across host libraries.
No native screenshots or application payloads are read by this generator.
"""
import hashlib
import io
import json
import os
import urllib.request
from pathlib import Path
import subprocess
import sys

linux_reference = sys.platform == 'linux'
if not linux_reference:
    import cairosvg
    from PIL import Image
    import PIL
    from cairosvg import surface
    assert cairosvg.__version__ == "2.8.2"
    assert PIL.__version__ == "11.3.0"
    cairo_version = surface.cairo.cairo_version_string()
else:
    helper_root = Path(__file__).resolve().with_name('zed-icon-raster')
    private_root = Path(os.environ['NANH_ZED_ICON_TEMPLATES']).resolve()
    private_root.mkdir(mode=0o700, parents=True, exist_ok=True)
    private_root.chmod(0o700)
    target_root = private_root / '.build'
    subprocess.run(['cargo', 'build', '--release', '--locked', '--manifest-path',
                    str(helper_root / 'Cargo.toml'), '--target-dir', str(target_root)],
                   check=True, timeout=180, stdout=subprocess.DEVNULL)
    raster_helper = target_root / 'release' / 'zed-icon-raster'

root = Path(os.environ["NANH_ZED_ICON_TEMPLATES"]).resolve()
root.mkdir(mode=0o700, parents=True, exist_ok=True)
root.chmod(0o700)
source_hashes = {'close.svg': '698d46fcdfd3be6bec156f5223f70436a4044b871c411ac9f65abb0dd7c4b619', 'copy.svg': '91a44e5c8af0e9b0fed70c22483b55d5de5a8cbeb5682b9a0c976cefec374c8b', 'rotate_cw.svg': '981ac36ab677c57bb59909dfd0d86cf8e326f2fa282cb99da013fb0811d2f6fe'}
source_hashes.update({'maximize.svg': 'f72db80db3a7d86c023ea4cc1b57b950d6179091ace7b4758b98449d40761a93',
                      'minimize.svg': '401a0f593c4d616f91421ebdcdc9e3179942e75cebcc20e29be331d8a16f82a8'})
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
            "rasterizer": {**({"resvg": "0.46.0", "usvg": "0.46.0", "tiny-skia": "0.11.4",
                "sampling": "2x-linear", "gamma": 1.8, "enhancedContrast": 1.0,
                "theme": "One Dark", "defaultRgb": "dce0e5", "selectedRgb": "74ade8"}
               if linux_reference else {"CairoSVG": "2.8.2", "Pillow": "11.3.0", "Cairo": cairo_version, "referenceCairo": "1.18.4"})}, "files": {}}
mask_hashes = {'close-14.alpha': 'ca9478e6d5ad466f0b930723ddeec769e2a0a818e5d48b01d990318c809be870', 'close-28.alpha': '79d36e9f7728628480497c0f559f273b4ed96d6454c9f18316729c91cfd96a56', 'copy-14.alpha': 'fd141505ee35387d2022907d97676d0e8442a7c0df7c555594e1e11f6d549495', 'copy-28.alpha': '5b8e649ba9854408f5938edd843875c5c5ef031900fff389e888ffb15619b146', 'rotate_cw-14.alpha': 'c653a9b025e2112e1b68c084da89d0f7e7e5488447f7e7072dddeae8946fdf44', 'rotate_cw-28.alpha': '81d4300229176b9070aea15f2d6758a4c6d578a51e5554d02a3d6af1001249ce'}
mask_hashes.update({'maximize-14.alpha': '909e9097e14d4eb8979c1bf3a6364fdfe78e2d3af304d1271dfac1e4ec7eee92',
                    'maximize-28.alpha': 'a6ccfa3e880c52701b83aa3904e1c6da97b43f04ef58e429a6212f4edb6af567',
                    'minimize-14.alpha': '8ea16a5bff909b7c9038d29d14584ec9a4e71cef3ea22d6fd63ef76cbdfb99bb',
                    'minimize-28.alpha': '630adb7db0c8c33ca94c981cba0468750ea8ab3372f42f03a615f432f24c925c'})
linux_mask_hashes = {'close-14.alpha': 'd080fca3754c1bcc7b49e69dbcb2c2f5022717dda19e710664ff8e5ea2347a9f', 'close-28.alpha': 'a67afdde0dbd8e8a4bad55eecec560ffe1a0d92ea66b46ae08289839fd54b723', 'copy-14.alpha': '2c33502f7d1f18652b8c0caee4aec0b551148ec9200b34d3bfaaa0f3b43f4dc2', 'copy-28.alpha': '659939d1a9a986f311379405e423e6108cd9e25330340ae58238278c901b5e30', 'maximize-14.alpha': 'c1739abf9e2a77dff175fc6b2b816e75c963206765e4ef31544b5b5470205827', 'maximize-28.alpha': '06342909a3974f39695a6eea9a3a86f641469786830316c0b178374a86df7c17', 'minimize-14.alpha': 'd015f04f4d96e71b8ab88742e82627abd72d4342258b4e46ac82c4b9c7cbc468', 'minimize-28.alpha': '07cae8b48db005b13da69f638723dc6fdc77d96ec8a46df79d92fdc06e6d1337', 'rotate_cw-14.alpha': '3ca55a13c548909625049d65d50a72b6ee1448135533bb11abbfd9953ec6dbad', 'rotate_cw-28.alpha': 'dd8e88bfdbba116e47b0fcb8f22768439df86a5cad52d496b6916d8203133586'}
for name in ("rotate_cw", "copy", "close", "maximize", "minimize"):
    source = root / f"{name}.svg"
    manifest["files"][source.name] = hashlib.sha256(source.read_bytes()).hexdigest()
    for side in (14, 28):
        if linux_reference:
            # The fullscreen toggle uses Color::Selected only when zoomed.
            palette = 'selected' if name == 'minimize' else 'default'
            alpha = subprocess.run([str(raster_helper), str(side), palette],
                input=source.read_bytes(), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                timeout=3, check=True).stdout
            if len(alpha) != side * side:
                raise ValueError("reference mask dimensions mismatch")
        else:
            png = cairosvg.svg2png(bytestring=source.read_bytes(), output_width=side, output_height=side)
            alpha = Image.open(io.BytesIO(png)).convert("RGBA").getchannel("A").tobytes()
        target = root / f"{name}-{side}.alpha"
        expected_digest = (linux_mask_hashes if linux_reference else mask_hashes)[target.name]
        if hashlib.sha256(alpha).hexdigest() != expected_digest:
            raise ValueError("reference mask digest mismatch")
        target.write_bytes(alpha)
        target.chmod(0o600)
        manifest["files"][target.name] = hashlib.sha256(alpha).hexdigest()
(root / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
(root / "manifest.json").chmod(0o600)
