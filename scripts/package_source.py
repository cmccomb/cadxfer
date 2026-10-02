#!/usr/bin/env python3
"""Create source ZIP/tar.gz archives and SHA256SUMS; never publish anything."""
from __future__ import annotations
import hashlib
from pathlib import Path
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    name = f"caxifer-{version}"
    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    files = [p for p in ROOT.rglob("*") if p.is_file() and not any(part in {".git", "target", "dist", "__pycache__"} for part in p.relative_to(ROOT).parts)]
    archives = [output / f"{name}.zip", output / f"{name}.tar.gz"]
    with zipfile.ZipFile(archives[0], "w", zipfile.ZIP_DEFLATED) as z:
        for path in sorted(files):
            z.write(path, str(Path(name) / path.relative_to(ROOT)))
    with tarfile.open(archives[1], "w:gz") as tar:
        for path in sorted(files):
            tar.add(path, arcname=str(Path(name) / path.relative_to(ROOT)), recursive=False)
    checksums = "".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in archives)
    (output / "SHA256SUMS.txt").write_text(checksums)
    print(checksums, end="")


if __name__ == "__main__":
    main()
