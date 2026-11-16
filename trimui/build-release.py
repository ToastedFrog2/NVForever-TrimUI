#!/usr/bin/env python3
"""Build a clean tracked source snapshot without embedding a developer checkout path."""
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

root = Path(__file__).resolve().parents[1]
if subprocess.check_output(["git", "status", "--porcelain"], cwd=root).strip():
    raise SystemExit("Commit the source changes before building a distributable release")
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
env = dict(os.environ)
env.setdefault("TRIMUI_SYSROOT", str(root / "trimui/sysroot"))
env.setdefault("TRIMUI_DEPS", str(root / "trimui/deps"))
env.setdefault("CARGO_TARGET_DIR", str(root / "engine/target-trimui"))
env.setdefault("NVF_CARGO_PROFILE", "release")
env["NVF_SOURCE_REVISION"] = revision
subprocess.run(["df", "-h", str(root)], check=True)
subprocess.run(["du", "-sh", str(root)], check=True)
subprocess.run(["bash", "trimui/build.sh", "--prepare-only"], cwd=root, env=env, check=True)
with tempfile.TemporaryDirectory(prefix="nvforever-source-", dir="/tmp") as temporary:
    stage = Path(temporary)
    archive = stage / "source.tar"
    with archive.open("wb") as f:
        subprocess.run(["git", "archive", "--format=tar", revision], cwd=root, stdout=f, check=True)
    with tarfile.open(archive) as t:
        members = t.getmembers()
        for member in members:
            resolved = (stage / member.name).resolve()
            if stage.resolve() not in resolved.parents or member.issym() or member.islnk():
                raise ValueError("Unexpected path in tracked source archive")
        t.extractall(stage, members=members)
    archive.unlink()
    subprocess.run(["bash", "trimui/build.sh"], cwd=stage, env=env, check=True)
    built = stage / "trimui/dist"
    assert (built / "nvforever/nvforever").read_bytes()[:6] == b"\x7fELF\x02\x01"
    destination = root / "trimui/dist"
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(built, destination)
print("Release tree: " + str(root / "trimui/dist"))
