#!/usr/bin/env python3
"""Build an offline native helper from digest-pinned upstream source, in OUT_DIR only."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request

SOURCES = {
    "leptonica": (
        "https://codeload.github.com/DanBloomberg/leptonica/tar.gz/13275a278eb55b5746e33f95fbf5a2c8f604b3ab",
        "e72242133adf678e8584081a506ca3a665c8f6f6de8c3fe7b47d1d5fcf9a4ce4",
    ),
    "tesseract": (
        "https://codeload.github.com/tesseract-ocr/tesseract/tar.gz/6e1d56a847e697de07b38619356550e5cf4e8633",
        "51342815a262a5c1d000bab44503ddbf71ef210053375d504f619ca7a3b381bd",
    ),
    "eng.traineddata": (
        "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/87416418657359cb625c412a48b6e1d6d41c29bd/eng.traineddata",
        "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2",
    ),
}


def download(path, identity):
    url, digest = identity
    if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == digest:
        return
    # No arbitrary URLs, proxy credentials or provider keys are passed to tools.
    with urllib.request.urlopen(url, timeout=120) as response:
        data = response.read(64 * 1024 * 1024 + 1)
    if len(data) > 64 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
        raise RuntimeError("native source failed its pinned digest check")
    path.write_bytes(data)


def unpack(path, output):
    stamp = output / ".source-sha256"
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if stamp.is_file() and stamp.read_text() == digest:
        return
    if output.exists():
        raise RuntimeError("partial native source exists; clean this Cargo build output and retry")
    output.mkdir()
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            parts = Path(member.name).parts
            if len(parts) < 2:
                continue
            relative = Path(*parts[1:])
            if relative.is_absolute() or ".." in relative.parts:
                raise RuntimeError("unsafe native source archive path")
            target = output / relative
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, target.open("xb") as destination:
                    shutil.copyfileobj(source, destination)
            else:
                raise RuntimeError("native source archive contains an unexpected link or special file")
    stamp.write_text(digest)


def build(cmake, source, build_root, prefix, definitions):
    configure = [cmake, "-S", str(source), "-B", str(build_root),
                 "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
                 "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW",
                 "-DCMAKE_INSTALL_LIBDIR=lib", "-DCMAKE_INSTALL_PREFIX=" + str(prefix),
                 "-DCMAKE_PREFIX_PATH=" + str(prefix), "-DBUILD_SHARED_LIBS=OFF",
                 "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded"]
    if sys.platform == "darwin":
        configure.append("-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0")
    subprocess.run(configure + definitions, check=True, timeout=120)
    subprocess.run([cmake, "--build", str(build_root), "--config", "Release",
                    "--parallel", "4"], check=True, timeout=1200)
    subprocess.run([cmake, "--install", str(build_root), "--config", "Release"],
                   check=True, timeout=120)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    cmake = os.environ.get("NAN_DESKTOP_CMAKE", "cmake")
    prefix = output / "native-prefix"
    for name in ("leptonica", "tesseract"):
        archive = output / (name + ".tar.gz")
        download(archive, SOURCES[name])
        unpack(archive, output / name)
    download(output / "eng.traineddata", SOURCES["eng.traineddata"])
    # Both upstream projects default to fetching SW dependencies on Windows.
    # Use only the digest-pinned sources and explicit local prefix on every OS.
    build(cmake, output / "leptonica", output / "leptonica-build", prefix,
          ["-DSW_BUILD=OFF", "-DBUILD_PROG=OFF", "-DENABLE_ZLIB=OFF", "-DENABLE_PNG=OFF",
           "-DENABLE_JPEG=OFF", "-DENABLE_TIFF=OFF", "-DENABLE_WEBP=OFF",
           "-DENABLE_OPENJPEG=OFF", "-DENABLE_GIF=OFF"])
    build(cmake, output / "tesseract", output / "tesseract-build", prefix,
          ["-DSW_BUILD=OFF", "-DBUILD_TRAINING_TOOLS=OFF", "-DBUILD_TESTS=OFF", "-DBUILD_PROG=OFF",
           "-DGRAPHICS_DISABLED=ON", "-DDISABLE_ARCHIVE=ON", "-DDISABLE_CURL=ON",
           "-DOPENMP_BUILD=OFF", "-DDISABLED_LEGACY_ENGINE=OFF"])
    build(cmake, Path(__file__).resolve().parent, output / "helper-build", output,
          ["-DNAN_OCR_PREFIX=" + str(prefix)])


if __name__ == "__main__":
    main()
