#!/usr/bin/env python3
"""Check source packaging and fixture invariants without compiling Rust."""
from __future__ import annotations

import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
FORMATS = ("bdf", "vtu", "msh", "inp", "frd", "op2")


def main() -> None:
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    package = manifest["package"]
    version = package["version"]
    assert package["name"] == "caexfer"
    assert not manifest.get("workspace") and not manifest.get("dependencies")
    assert manifest["lints"]["rust"]["unsafe_code"] == "forbid"
    assert all((ROOT / name).is_file() for name in ("README.md", "LICENSE-MIT", "LICENSE-APACHE"))
    checks = ["one dependency-free package with README and licenses; unsafe code forbidden"]

    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    assert lock["package"] == [{"name": "caexfer", "version": version}]
    checks.append("lockfile contains only the caexfer package")

    lib = (ROOT / "src/lib.rs").read_text()
    assert all((ROOT / "src" / f"{name}.rs").is_file() or (ROOT / "src" / name / "mod.rs").is_file() for name in FORMATS)
    assert all(re.search(rf"^pub mod {name};$", lib, re.M) for name in FORMATS)
    assert (ROOT / "src/bdf/mesh.rs").is_file()
    assert (ROOT / "src/main.rs").is_file()
    checks.append("library exposes every supported format and the CLI is present")

    sources = list((ROOT / "src").rglob("*.rs")) + list((ROOT / "tests").rglob("*.rs"))
    tests = sum(len(re.findall(r"#\[test\]", source.read_text())) for source in sources)
    assert not any(re.search(r"\b(?:todo|unimplemented)!\s*\(", source.read_text()) for source in sources)
    checks.append("no todo!/unimplemented! placeholders in Rust source")

    small = (ROOT / "tests/fixtures/small.bdf").read_bytes()
    large = (ROOT / "tests/fixtures/large.bdf").read_bytes()
    assert all(len(line) == 80 for line in small.splitlines() + large.splitlines())
    assert large.splitlines()[0][72:80].strip() == large.splitlines()[1][:8].strip() == b"*A"
    assert b"\xff" in (ROOT / "tests/fixtures/mixed-newlines.bdf").read_bytes()
    checks.append("fixed-width and byte-sensitive fixtures have intended byte layouts")

    expected = json.loads((ROOT / "tests/fixtures/mixed-linear.expected.json").read_text())
    ids = {point["id"] for point in expected["nodes"]}
    assert len(ids) == 9
    assert [cell["vtk_type"] for cell in expected["cells"]] == [3, 5, 9, 10, 12, 13, 14]
    assert all(set(cell["node_ids"]) <= ids for cell in expected["cells"])
    checks.append("independent topology expectations are internally consistent")

    print(json.dumps({
        "kind": "source-package-checks", "version": version,
        "checks_passed": checks, "rust_test_functions_present": tests,
        "rust_compilation_run": False, "rust_tests_run": False,
        "note": "Packaging/fixture checks are not Rust compilation or implementation tests.",
    }, indent=2))


if __name__ == "__main__":
    main()
