"""Reject release archives with absent or stale dependency notices."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate(root: Path, app: Path | None = None):
    report = root / "THIRD-PARTY-LICENSES.txt"
    inventory = root / "licensing/inventory.json"
    data = json.loads(inventory.read_text())
    if (data.get("schema") != 1 or data.get("target") != "aarch64-unknown-linux-gnu"
            or data.get("features") != ["trimui", "intro-video"] or not data.get("packages")):
        raise ValueError("Invalid TrimUI dependency notice inventory")
    expected = {"THIRD-PARTY-LICENSES.txt": data["report_sha256"],
                "Cargo.lock": data["root_lock_sha256"],
                "engine/Cargo.lock": data["engine_lock_sha256"], **data["local_manifests"],
                **data["notice_inputs"]}
    for name, sha in expected.items():
        if not (root / name).is_file() or digest(root / name) != sha:
            raise ValueError(f"Missing/stale notices: {name}; regenerate dependency notices")
    if app is not None:
        for name, source in {"THIRD-PARTY-LICENSES.txt": report,
                             "licenses/THIRD-PARTY-LICENSES.inventory.json": inventory,
                             "LICENSE": root / "LICENSE", "NOTICE.md": root / "NOTICE.md",
                             "LEGAL.md": root / "LEGAL.md"}.items():
            if not (app / name).is_file() or digest(app / name) != digest(source):
                raise ValueError(f"Missing/stale packaged notice: {name}")
        pins = (app / "spirv-opt.version").read_text()
        for component in data["native_components"]:
            if component["name"] in ("SPIRV-Tools", "SPIRV-Headers") and component["version"] not in pins:
                raise ValueError("Optimizer changed; refresh native notices")
    return data
