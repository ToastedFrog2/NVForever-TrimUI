#!/usr/bin/env python3
"""Read-only Linux telemetry. Run alongside NVF_GPU_REPORT=1, never ptrace the FPS run.

Usage: python3 frame-costs.py PID [SECONDS=90] [INTERVAL=1] > frame-costs.jsonl
Process faults/I/O are attributed to PID; vmstat and GPU counters are system-wide.
"""
import argparse
import json
import os
from pathlib import Path
import time


def read(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return ""


def stat_fields(raw):
    # comm may contain spaces and parentheses. Fields after the last ')' start at #3.
    f = raw[raw.rfind(")") + 2:].split()
    return {"state": f[0], "minflt": int(f[7]), "majflt": int(f[9]),
            "cpu_ticks": int(f[11]) + int(f[12]), "start_ticks": int(f[19]),
            "rss_pages": int(f[21]), "delay_ticks": int(f[39]) if len(f) > 39 else 0}


def counters(raw, colon=False):
    result = {}
    for line in raw.splitlines():
        key, _, value = line.partition(":" if colon else " ")
        try:
            result[key] = int(value.split()[0])
        except (ValueError, IndexError):
            pass
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("pid", type=int)
    p.add_argument("seconds", type=float, nargs="?", default=90)
    p.add_argument("interval", type=float, nargs="?", default=1)
    a = p.parse_args()
    if a.pid <= 0 or not 0 < a.seconds <= 3600 or not 0.1 <= a.interval <= 60:
        p.error("positive PID, 0 < seconds <= 3600, 0.1 <= interval <= 60 required")
    proc = Path(f"/proc/{a.pid}")
    first = stat_fields((proc / "stat").read_text())
    def emit(value):
        print(json.dumps(value, sort_keys=True), flush=True)
    started = time.monotonic()
    emit({"type": "metadata", "pid": a.pid, "boot_id": read("/proc/sys/kernel/random/boot_id"),
          "unix_time": time.time(), "uptime": read("/proc/uptime"),
          "ticks_per_second": os.sysconf("SC_CLK_TCK"), "page_bytes": os.sysconf("SC_PAGE_SIZE"),
          "maps": read(proc / "maps"), "note": "Vulkan API wall/cpu timings are in the game log; no GPU timestamps"})
    deadline = started
    try:
        while time.monotonic() - started < a.seconds:
            raw = read(proc / "stat")
            if not raw:
                break
            stats = stat_fields(raw)
            if stats["start_ticks"] != first["start_ticks"]:
                break  # PID reused; never attribute a different process to this run.
            vm = counters(read("/proc/vmstat"))
            status = counters(read(proc / "status"), colon=True)
            io = counters(read(proc / "io"), colon=True)
            threads = []
            for task in sorted((proc / "task").glob("[0-9]*")):
                raw = read(task / "stat")
                if not raw:
                    continue
                t = stat_fields(raw)
                threads.append({"tid": int(task.name), "name": read(task / "comm"),
                                "state": t["state"], "wchan": read(task / "wchan"),
                                "cpu_ticks": t["cpu_ticks"], "start_ticks": t["start_ticks"]})
            emit({"type": "sample", "elapsed": time.monotonic() - started, "unix_time": time.time(), "process": stats,
                  "status_kib": {k: status.get(k) for k in ("VmRSS", "VmSwap", "RssAnon", "RssFile")},
                  "io": {k: io.get(k) for k in ("read_bytes", "write_bytes")},
                  "vmstat_system": {k: vm.get(k) for k in ("pgfault", "pgmajfault", "pswpin", "pswpout", "pgscan_direct", "pgscan_kswapd")},
                  "cpu_khz": read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"),
                  "temp": read("/sys/class/thermal/thermal_zone0/temp"),
                  "gpu": read("/sys/kernel/debug/pvr/status"), "threads": threads})
            deadline += a.interval
            time.sleep(max(0, deadline - time.monotonic()))
    finally:
        emit({"type": "end", "elapsed": time.monotonic() - started})


if __name__ == "__main__":
    main()
