#!/usr/bin/env python3
"""Package the current cross-build without game data, saves or build intermediates."""
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo
import hashlib
import json
import struct
import argparse
from tools.license_bundle import validate

def write_file(archive, path, name):
    info = ZipInfo(str(name), date_time=(2026, 11, 17, 0, 0, 0))
    info.create_system = 3
    info.external_attr = (0o100000 | (0o755 if path.stat().st_mode & 0o111 else 0o644)) << 16
    archive.writestr(info, path.read_bytes(), compress_type=ZIP_DEFLATED, compresslevel=6)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, help="candidate ZIP path; default: release/nvforever.zip")
args = parser.parse_args()

dist = Path(__file__).resolve().parent / "dist"
app = dist / "nvforever"
validate(dist.parents[1], app)
assert (app / "nvforever").is_file(), "run trimui/build.sh first"
assert not (app / "game").exists() and not (app / "home").exists()
release = dist.parent / "release"
release.mkdir(exist_ok=True)
output = args.output.resolve() if args.output else release / "nvforever.zip"
output.parent.mkdir(parents=True, exist_ok=True)
assert output.suffix == ".zip", "output must end in .zip"
temporary = output.with_suffix(".zip.part")
with (app / "nvforever").open("rb") as executable:
    header = executable.read(64)
assert header[:6] == b"\x7fELF\x02\x01" and struct.unpack_from("<H",header,18)[0] == 183, "expected ARM64 Linux executable"
assert "Enabled=0" in (app / "dev-menu.ini").read_text(), "release developer menu must default off"
metadata = json.loads((app / "port.json").read_text())
assert metadata["name"] == "nvforever.zip" and set(metadata["items"]) == {"NVForever.sh", "nvforever/"}
try:
    with ZipFile(temporary, "w", compression=ZIP_DEFLATED, compresslevel=6) as archive:
        write_file(archive, dist / "NVForever.sh", "NVForever.sh")
        for path in sorted(app.rglob("*")):
            if path.is_file() and not any(part.startswith('.') or part == '__MACOSX' for part in path.relative_to(app).parts):
                assert not path.is_symlink(), f"unexpected symlink: {path}"
                write_file(archive, path, path.relative_to(dist))
    with ZipFile(temporary) as archive:
        assert archive.testzip() is None, "ZIP checksum verification failed"
        assert archive.read("nvforever/dev-menu.ini") == (app / "dev-menu.ini").read_bytes()
        assert not any(name.startswith(("nvforever/home/", "nvforever/game/")) for name in archive.namelist())
    temporary.replace(output)
finally:
    temporary.unlink(missing_ok=True)
print(f"{output}: {output.stat().st_size:,} bytes")
digest = hashlib.sha256(output.read_bytes()).hexdigest()
output.with_suffix('.zip.sha256').write_text(f"{digest}  {output.name}\n")
