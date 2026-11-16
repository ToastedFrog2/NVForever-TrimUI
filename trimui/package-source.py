#!/usr/bin/env python3
"""Archive the current maintained source (tracked + unignored new files), no builds/data."""
import argparse
import hashlib
from pathlib import Path
import subprocess
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

def write_file(archive, path, name):
    info = ZipInfo(str(name), date_time=(2026, 11, 16, 0, 0, 0))
    info.create_system = 3
    info.external_attr = (0o100000 | (0o755 if path.stat().st_mode & 0o111 else 0o644)) << 16
    archive.writestr(info, path.read_bytes(), compress_type=ZIP_DEFLATED, compresslevel=6)


root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--output", type=Path, default=root / "trimui/release/NVForever-0.3.1-source.zip")
a = p.parse_args()
output = a.output.resolve()
output.parent.mkdir(parents=True, exist_ok=True)
temporary = output.with_suffix(".zip.part")
files = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
state = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root).decode().strip()
status = subprocess.check_output(["git", "status", "--short"], cwd=root).decode()
try:
    with ZipFile(temporary, "w", ZIP_DEFLATED, compresslevel=6) as z:
        for name in sorted(set(files) - {""}):
            path = root / name
            if not path.is_file():
                continue
            if path.is_symlink():
                raise ValueError(f"Source symlink needs explicit handling: {name}")
            if path.resolve() in (output, temporary):
                continue
            write_file(z, path, "NVForever/" + name)
        info = ZipInfo("NVForever/SOURCE-STATE.txt", date_time=(2026, 11, 16, 0, 0, 0))
        info.create_system = 3
        info.external_attr = 0o100644 << 16
        z.writestr(info, f"Source revision: {state}\n")
    with ZipFile(temporary) as z:
        assert z.testzip() is None
        for needed in ("Cargo.toml", "Cargo.lock", "engine/Cargo.lock", "LICENSE", "trimui/FPS-WORK.md", "trimui/build.sh", "engine/nvforever/src/trimui_idle.rs"):
            assert "NVForever/" + needed in z.namelist(), needed
        assert not any("/target/" in n or "/target-trimui" in n or "/.git/" in n or "/trimui/release/" in n for n in z.namelist())
    temporary.replace(output)
finally:
    temporary.unlink(missing_ok=True)
digest = hashlib.sha256(output.read_bytes()).hexdigest()
output.with_suffix(".zip.sha256").write_text(f"{digest}  {output.name}\n")
print(f"{output}: {output.stat().st_size:,} bytes")
