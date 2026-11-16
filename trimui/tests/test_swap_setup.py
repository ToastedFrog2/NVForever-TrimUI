"""Exercise setup decisions with disposable files and simulated Linux utilities."""
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "package/setup-swap.sh"
GIB = 1024 ** 3
STUB = r'''#!/usr/bin/env python3
import json, os, sys, time
from pathlib import Path
cmd = Path(sys.argv[0]).name
args = sys.argv[1:]
with Path(os.environ["TEST_EVENTS"]).open("a") as f:
    f.write(json.dumps([cmd, args]) + "\n")
if cmd == "df":
    print("Filesystem 1024-blocks Used Available Capacity Mounted on")
    print("/dev/internal 5000000 1000000", os.environ.get("TEST_FREE_KIB", "3000000"), "20% /internal")
elif cmd == "stat":
    print(Path(args[-1]).stat().st_size)
elif cmd == "dd":
    values = dict(a.split("=", 1) for a in args)
    if os.environ.get("TEST_DD_PAUSE"):
        time.sleep(30)
    with Path(values["of"]).open("wb") as f:
        f.truncate(16 if os.environ.get("TEST_DD_FAIL") else int(values["count"]) * 1024 ** 2)
    if os.environ.get("TEST_DD_FAIL"):
        sys.exit(1)
elif cmd == "mkswap":
    with Path(args[-1]).open("r+b") as f:
        f.write(b"FAKE_SWAP\n")
elif cmd == "swapon":
    p = Path(args[-1])
    with p.open("rb") as f:
        if f.read(10) != b"FAKE_SWAP\n":
            sys.exit(1)
    with Path(os.environ["NVF_PROC_SWAPS"]).open("a") as f:
        f.write(f"{p} file {p.stat().st_size // 1024 - 4} 0 10\n")
'''


class SwapSetupTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="nvf-swap-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.app = self.root / "app"; self.app.mkdir()
        self.internal = self.root / "internal"; self.internal.mkdir()
        self.swap = self.internal / "nvforever.swap"
        self.proc = self.root / "swaps"
        self.proc.write_text("Filename Type Size Used Priority\n")
        self.mounts = self.root / "mounts"
        self.mounts.write_text(f"/dev/internal {self.internal} ext4 rw 0 0\n")
        self.events = self.root / "events"
        tools = self.root / "bin"; tools.mkdir()
        for name in ["dd", "mkswap", "swapon", "df", "stat", "sync"]:
            p = tools / name; p.write_text(STUB); p.chmod(0o755)
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                        NVF_APP_DIR=str(self.app), NVF_SWAP_INTERNAL_DIR=str(self.internal),
                        NVF_PROC_SWAPS=str(self.proc), NVF_PROC_MOUNTS=str(self.mounts), TEST_EVENTS=str(self.events))

    def run_setup(self, *args, expected=0):
        p = subprocess.run(["/bin/sh", str(SCRIPT), *(args or ("--ensure",))], env=self.env,
                           text=True, capture_output=True, timeout=15)
        self.assertEqual(p.returncode, expected, p.stdout + p.stderr)
        return p

    def commands(self):
        return [json.loads(l)[0] for l in self.events.read_text().splitlines()] if self.events.exists() else []

    def existing(self, path=None, active=False, valid=True):
        path = path or self.swap
        with path.open("wb") as f:
            f.write(b"FAKE_SWAP\n" if valid else b"USER_DATA\n")
            f.truncate(2 * GIB)
        if active:
            self.proc.write_text(self.proc.read_text() + f"{path} file 2097148 0 10\n")
        return path

    def test_first_launch_creates_internal_swap_and_records_it(self):
        self.run_setup()
        self.assertEqual(self.swap.stat().st_size, 2 * GIB)
        self.assertEqual(self.swap.stat().st_mode & 0o777, 0o600)
        self.assertEqual((self.app / "swap-file").read_text().strip(), str(self.swap))
        self.assertEqual(self.commands().count("dd"), 1)
        self.assertFalse(list(self.internal.glob("*.setup.*")))
        self.assertFalse((self.app / ".swap-setup.lock").exists())

    def test_later_launch_does_not_allocate_or_reformat(self):
        self.existing(active=True)
        self.env["TEST_FREE_KIB"] = "1"
        self.run_setup()
        self.assertEqual(self.commands(), [])

    def test_inactive_existing_swap_is_enabled_without_reformatting(self):
        self.existing()
        self.run_setup()
        self.assertEqual(self.commands(), ["stat", "swapon"])

    def test_other_ports_active_internal_swap_is_reused(self):
        other = self.existing(self.internal / "other-port.swap", active=True)
        self.run_setup()
        self.assertFalse(self.swap.exists())
        self.assertEqual((self.app / "swap-file").read_text().strip(), str(other))
        self.assertEqual(self.commands(), [])

    def test_explicit_manual_target_does_not_reuse_another_file(self):
        self.existing(self.internal / "other-port.swap", active=True)
        target = self.internal / "manual.swap"
        self.run_setup("64", str(target))
        self.assertEqual(target.stat().st_size, 64 * 1024 ** 2)
        self.assertEqual((self.app / "swap-file").read_text().strip(), str(target))

    def test_insufficient_space_leaves_files_and_config_untouched(self):
        self.env["TEST_FREE_KIB"] = "2300000"
        self.run_setup(expected=20)
        self.assertFalse(self.swap.exists())
        self.assertFalse((self.app / "swap-file").exists())
        self.assertNotIn("dd", self.commands())

    def test_existing_non_swap_file_is_not_overwritten(self):
        self.existing(valid=False)
        before = (self.swap.stat().st_size, self.swap.stat().st_mode)
        self.run_setup(expected=21)
        with self.swap.open("rb") as f:
            self.assertEqual(f.read(10), b"USER_DATA\n")
        self.assertEqual(before, (self.swap.stat().st_size, self.swap.stat().st_mode))
        self.assertNotIn("mkswap", self.commands())
        self.assertNotIn("dd", self.commands())

    def test_unsupported_filesystem_does_not_allocate(self):
        self.mounts.write_text(f"/dev/card {self.internal} vfat rw 0 0\n")
        self.run_setup(expected=22)
        self.assertFalse(self.swap.exists())
        self.assertEqual(self.commands(), [])

    def test_invalid_sizes_cannot_touch_existing_user_files(self):
        self.swap.write_bytes(b"keep this file")
        for value in ["0", "-1", "invalid", "999999"]:
            self.run_setup(value, str(self.swap), expected=2)
        self.assertEqual(self.swap.read_bytes(), b"keep this file")
        self.assertEqual(self.commands(), [])

    def test_failed_allocation_cleans_partial_file(self):
        self.env["TEST_DD_FAIL"] = "1"
        self.run_setup(expected=23)
        self.assertFalse(self.swap.exists())
        self.assertFalse(list(self.internal.glob("*.setup.*")))
        self.assertNotIn("mkswap", self.commands())

    def test_stale_setup_file_is_recovered_without_removing_other_files(self):
        lock = self.app / ".swap-setup.lock"; lock.mkdir()
        stale = self.internal / "nvforever.swap.setup.abcdef"; stale.write_bytes(b"partial")
        for name, value in {"pid": "99999999", "boot": "previous-boot", "start": "0", "target": str(self.swap), "temp": str(stale)}.items():
            (lock / name).write_text(value)
        keep = self.internal / "unrelated-file"; keep.write_bytes(b"keep")
        self.run_setup()
        self.assertFalse(stale.exists())
        self.assertEqual(keep.read_bytes(), b"keep")

    def test_interrupt_stops_allocation_and_cleans_up(self):
        self.env["TEST_DD_PAUSE"] = "1"
        p = subprocess.Popen(["/bin/sh", str(SCRIPT), "--ensure"], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for _ in range(200):
                if "dd" in self.commands():
                    break
                time.sleep(0.01)
            self.assertIn("dd", self.commands())
            p.send_signal(signal.SIGTERM)
            p.communicate(timeout=5)
            self.assertEqual(p.returncode, 143)
            self.assertFalse(list(self.internal.glob("*.setup.*")))
            self.assertFalse((self.app / ".swap-setup.lock").exists())
        finally:
            if p.poll() is None:
                p.kill(); p.communicate()


if __name__ == "__main__":
    unittest.main()
