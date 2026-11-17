#!/usr/bin/env python3
"""Generate complete TrimUI dependency notices from the locked Cargo release graph.

Normal/build dependencies are included conservatively. Development dependencies
are excluded. Missing crate license texts require a pinned supplemental record;
declaring a license name alone never silently satisfies this generator.
"""
from __future__ import annotations

import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
TARGET = "aarch64-unknown-linux-gnu"
FEATURES = "trimui,intro-video"
LICENSE_NAME = re.compile(r"^(licen[sc]e|copying|copyright|unlicense|notice|ofl)(?:$|[-_.])", re.I)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args: list[str]) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True)


def parse_tree(text: str) -> set[tuple[str, str]]:
    return {m.groups() for line in text.splitlines() if (m := re.match(r"^(\S+) v(\S+)", line))}


def select_packages(meta: dict, wanted: set[tuple[str, str]]) -> list[dict]:
    selected = [p for p in meta["packages"] if (p["name"], p["version"]) in wanted]
    if len(selected) != len(wanted):
        raise ValueError("Cargo graph has missing or ambiguous package identities")
    return sorted(selected, key=lambda p: (p["name"], p["version"]))


def ours(pkg: dict, root: Path = ROOT) -> bool:
    path = Path(pkg["manifest_path"]).resolve()
    return not pkg.get("source") and root in path.parents and root / "engine/vendor" not in path.parents


def license_files(folder: Path) -> list[Path]:
    result = []
    for p in sorted(folder.rglob("*")):
        if not p.is_file() or p.is_symlink():
            continue
        relative = p.relative_to(folder)
        if LICENSE_NAME.match(p.name) or any(s.lower() in ("licenses", "licences") for s in relative.parts[:-1]):
            if p.suffix.lower() not in (".rs", ".ttf", ".otf", ".png"):
                result.append(p)
    return result


def supplemental_files(pkg: dict, records: dict, root: Path = ROOT) -> list[tuple[str, str]]:
    key = pkg["name"] + "@" + pkg["version"]
    record = records.get(key)
    if record is None:
        raise ValueError(f"Missing license text for {key}; add a pinned supplemental record")
    vcs = Path(pkg["manifest_path"]).parent / ".cargo_vcs_info.json"
    if vcs.is_file() and json.loads(vcs.read_text())["git"]["sha1"] != record["revision"]:
        raise ValueError(f"Supplemental license revision mismatch for {key}")
    found = []
    for entry in record["files"]:
        path = root / entry["file"]
        if root.resolve() not in path.resolve().parents or digest(path) != entry["sha256"]:
            raise ValueError(f"Supplemental license integrity failure for {key}")
        found.append((f'{path.name}; {entry["source"]}', path.read_text()))
    if not found:
        raise ValueError(f"Empty supplemental license record for {key}")
    return found


class PlainHtml(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []
        self.links = []

    def handle_starttag(self, tag, attrs):
        if tag in ("p", "li", "div", "tr", "br", "pre", "h1", "h2", "h3", "h4"):
            self.parts.append("\n")
        if tag == "a":
            self.links.append(dict(attrs).get("href", ""))

    def handle_endtag(self, tag):
        if tag == "a" and self.links:
            url = self.links.pop()
            if url.startswith(("https://", "http://")):
                self.parts.append(f" ({url})")
        if tag in ("p", "li", "div", "tr", "pre", "h1", "h2", "h3", "h4"):
            self.parts.append("\n")
        if tag == "td":
            self.parts.append(" | ")

    def handle_data(self, data):
        self.parts.append(data)

    def text(self):
        return re.sub(r"\n[ \t]*\n(?:[ \t]*\n)+", "\n\n", "".join(self.parts)).strip()


def generate(offline: bool = False) -> tuple[str, dict]:
    common = ["--locked"] + (["--offline"] if offline else [])
    meta = json.loads(run(["cargo", "metadata", "--manifest-path", "engine/nvforever/Cargo.toml",
                           "--format-version", "1", "--filter-platform", TARGET,
                           "--no-default-features", "--features", FEATURES] + common))
    tree = run(["cargo", "tree", "--manifest-path", "engine/Cargo.toml", "--package", "nvforever",
                "--target", TARGET, "--no-default-features", "--features", FEATURES,
                "--edges", "normal,build", "--prefix", "none", "--format", "{p}"] + common)
    packages = select_packages(meta, parse_tree(tree))
    records = json.loads((ROOT / "licensing/supplemental.json").read_text())
    native = json.loads((ROOT / "licensing/native.json").read_text())
    texts: dict[str, list[str]] = {}
    inventory = []
    local_manifests = {}
    lines = ["NVForever TrimUI — THIRD-PARTY LICENSES AND NOTICES", "",
             f"Release graph: nvforever; target {TARGET}; features {FEATURES}.",
             "Includes normal and build dependencies; excludes development dependencies.",
             "This conservative inventory can include build-time or unused components.",
             "Each component retains its own license; this file does not relicense it.",
             "Copyright/permission notices below are reproduced from component sources.",
             "NVForever's own GPL-3.0-or-later license is supplied separately as LICENSE.", "",
             "RUST PACKAGE INDEX", ""]
    for pkg in packages:
        manifest = Path(pkg["manifest_path"])
        if ROOT in manifest.resolve().parents:
            local_manifests[str(manifest.resolve().relative_to(ROOT))] = digest(manifest)
        if ours(pkg):
            continue
        label = f'{pkg["name"]} {pkg["version"]}'
        if not pkg.get("license"):
            raise ValueError(f"No license expression declared for {label}")
        folder = manifest.parent
        files = license_files(folder)
        top = [p for p in files if p.parent == folder]
        found = [(str(p.relative_to(folder)), p.read_text(encoding="utf-8")) for p in files]
        if not top:
            found += supplemental_files(pkg, records)
        if not found or not any(len(text.strip()) >= 80 for _, text in found):
            raise ValueError(f"No license texts found for {label}")
        source = pkg.get("repository") or "https://crates.io/crates/" + pkg["name"]
        if pkg.get("source"):
            source_archive = f'https://crates.io/api/v1/crates/{pkg["name"]}/{pkg["version"]}/download'
        else:
            source_archive = "https://github.com/ToastedFrog2/NVForever-TrimUI (engine/vendor/)"
        lines += [f'{label} — {pkg["license"]}', f"  Project: {source}", f"  Versioned source: {source_archive}"]
        if records.get(pkg["name"] + "@" + pkg["version"], {}).get("selected_license"):
            lines.append("  Selected option: " + records[pkg["name"] + "@" + pkg["version"]]["selected_license"])
        inventory.append({"name": pkg["name"], "version": pkg["version"], "license": pkg["license"],
                          "source": source_archive, "notice_documents": len(found)})
        for name, text in found:
            text = text.replace("\r\n", "\n").strip()
            if text:
                texts.setdefault(text, []).append(label + " — " + name)
    for text, users in sorted(texts.items(), key=lambda x: x[1][0]):
        lines += ["", "=" * 78, "From: " + "\n      ".join(users), "=" * 78, "", text]
    lines += ["", "=" * 78, "NATIVE RUNTIMES, FONT AND SEPARATE OPTIMIZER", "=" * 78]
    for c in native:
        lines += ["", f'{c["name"]} — {c["version"]}', "License: " + c["license"], "Source: " + c["source"]]
        if c.get("copyright"):
            lines.append(c["copyright"])
        for name in c["files"]:
            p = ROOT / name
            if digest(p) != c["sha256"][name]:
                raise ValueError(f"Native notice integrity failure: {name}")
            text = p.read_text()
            if p.suffix == ".html":
                parser = PlainHtml(); parser.feed(text); text = parser.text()
            lines += ["", "--- " + p.name + " ---", text.strip()]
    lines += ["", "System libraries supplied by the device firmware are not copied into this ZIP."]
    text = "\n".join(lines) + "\n"
    rust_version = run(["rustc", "--version"]).strip()
    recorded_rust = next(c["version"] for c in native if c["name"].startswith("Rust standard library"))
    if recorded_rust != rust_version:
        raise ValueError("Rust toolchain changed; refresh the pinned standard-library notices")
    data = {"schema": 1, "root_package": "nvforever", "target": TARGET, "features": FEATURES.split(","),
            "rust_version": rust_version,
            "engine_lock_sha256": digest(ROOT / "engine/Cargo.lock"),
            "root_lock_sha256": digest(ROOT / "Cargo.lock"), "local_manifests": local_manifests,
            "packages": inventory, "native_components": [{"name": c["name"], "version": c["version"]} for c in native],
            "notice_inputs": {str(p.relative_to(ROOT)): digest(p) for p in sorted((ROOT / "licensing").rglob("*")) if p.is_file() and p.name != "inventory.json"},
            "report_sha256": hashlib.sha256(text.encode()).hexdigest()}
    return text, data


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--out", type=Path, default=ROOT / "THIRD-PARTY-LICENSES.txt")
    p.add_argument("--inventory", type=Path, default=ROOT / "licensing/inventory.json")
    p.add_argument("--offline", action="store_true")
    a = p.parse_args()
    text, inventory = generate(a.offline)
    a.out.write_text(text, encoding="utf-8")
    a.inventory.write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Generated notices for {len(inventory['packages'])} Rust packages and {len(inventory['native_components'])} additional components")


if __name__ == "__main__":
    main()
