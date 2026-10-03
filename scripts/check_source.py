#!/usr/bin/env python3
"""Standard-library-only source packaging checks. This does NOT compile Rust."""
from __future__ import annotations

import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = workspace["workspace"]["package"]["version"]
    members: dict[str, tuple[Path, dict]] = {}
    checks: list[str] = []
    for rel in workspace["workspace"]["members"]:
        folder = ROOT / rel
        manifest = tomllib.loads((folder / "Cargo.toml").read_text())
        name = manifest["package"]["name"]
        assert name not in members, name
        assert manifest["package"]["version"] == {"workspace": True}
        assert (folder / "README.md").is_file()
        assert (folder / "LICENSE-MIT").is_file()
        assert (folder / "LICENSE-APACHE").is_file()
        assert (folder / "src/lib.rs").is_file() or (folder / "src/main.rs").is_file()
        members[name] = (folder, manifest)
    checks.append("five package manifests parse and required files exist")
    assert len(members) == 5
    graph: dict[str, list[str]] = {}
    for name, (folder, manifest) in members.items():
        graph[name] = []
        for dep, spec in manifest.get("dependencies", {}).items():
            assert "path" in spec, f"unexpected nonlocal dependency: {dep}"
            assert spec["version"] == version
            target = (folder / spec["path"]).resolve()
            assert target == members[dep][0].resolve()
            graph[name].append(dep)
        for example in manifest.get("example", []):
            assert (folder / example["path"]).is_file()
    done: set[str] = set()
    def visit(name: str, active: set[str]) -> None:
        assert name not in active, f"dependency cycle at {name}"
        if name in done:
            return
        for child in graph[name]:
            visit(child, active | {name})
        done.add(name)
    for name in graph:
        visit(name, set())
    checks.append("all Rust dependencies are workspace-local and acyclic")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    assert {p["name"] for p in lock["package"]} == set(members)
    assert all(p["version"] == version and "source" not in p for p in lock["package"])
    for package in lock["package"]:
        assert set(package.get("dependencies", [])) == set(graph[package["name"]])
    checks.append("lockfile matches workspace package versions and dependency graph")
    facade = members["caexfer-formats"][1]
    assert facade["features"]["default"] == []
    assert set(facade["features"]) == {"default", "bdf", "vtu", "all-formats"}
    assert workspace["workspace"]["lints"]["rust"]["unsafe_code"] == "forbid"
    checks.append("feature gates match implemented libraries; unsafe code forbidden")
    tests = 0
    for source in (ROOT / "crates").rglob("*.rs"):
        text = source.read_text()
        assert not re.search(r"\b(?:todo|unimplemented)!\s*\(", text), source
        tests += len(re.findall(r"#\[test\]", text))
    checks.append("no todo!/unimplemented! placeholders in Rust source")
    small = (ROOT / "tests/fixtures/small.bdf").read_bytes()
    large = (ROOT / "tests/fixtures/large.bdf").read_bytes()
    assert all(len(line) == 80 for line in small.splitlines())
    assert all(len(line) == 80 for line in large.splitlines())
    assert large.splitlines()[0][72:80].strip() == large.splitlines()[1][:8].strip() == b"*A"
    assert b"\xff" in (ROOT / "tests/fixtures/mixed-newlines.bdf").read_bytes()
    checks.append("fixed-width and byte-sensitive fixtures have intended byte layouts")
    expected = json.loads((ROOT / "tests/fixtures/mixed-linear.expected.json").read_text())
    ids = {point["id"] for point in expected["nodes"]}
    assert len(ids) == 9
    assert [c["vtk_type"] for c in expected["cells"]] == [3, 5, 9, 10, 12, 13, 14]
    for cell in expected["cells"]:
        assert set(cell["node_ids"]) <= ids
    checks.append("independent topology expectations are internally consistent")
    print(json.dumps({
        "kind": "source-package-checks", "version": version,
        "checks_passed": checks, "rust_test_functions_present": tests,
        "rust_compilation_run": False, "rust_tests_run": False,
        "note": "Packaging/fixture checks are not Rust compilation or implementation tests.",
    }, indent=2))


if __name__ == "__main__":
    main()
