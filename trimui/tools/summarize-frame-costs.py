#!/usr/bin/env python3
"""Summarize frame-costs.py JSONL; choose --after to exclude load/warm-up."""
import argparse
import json
from pathlib import Path


def summarize(rows, after=0):
    metadata = next(r for r in rows if r.get("type") == "metadata")
    samples = [r for r in rows if r.get("type") == "sample" and r["elapsed"] >= after]
    if len(samples) < 2:
        raise ValueError("at least two samples required after warm-up")
    a, b = samples[0], samples[-1]
    if a["process"]["start_ticks"] != b["process"]["start_ticks"]:
        raise ValueError("process identity changed")
    secs = b["elapsed"] - a["elapsed"]
    if secs <= 0:
        raise ValueError("sample timestamps must increase")
    def delta(group, key):
        x, y = a[group].get(key), b[group].get(key)
        return y - x if isinstance(x, int) and isinstance(y, int) and y >= x else None
    cpu = delta("process", "cpu_ticks")
    return {"seconds": secs, "samples": len(samples),
            "process_cpu_core_equivalents": cpu / metadata["ticks_per_second"] / secs if cpu is not None else None,
            "process_minor_faults": delta("process", "minflt"),
            "process_major_faults": delta("process", "majflt"),
            "process_storage_read_bytes": delta("io", "read_bytes"),
            "process_rss_kib_end": b["status_kib"]["VmRSS"],
            "process_swap_kib_end": b["status_kib"]["VmSwap"],
            "system_swapin_pages": delta("vmstat_system", "pswpin"),
            "system_swapout_pages": delta("vmstat_system", "pswpout"),
            "note": "Minor faults do not imply disk I/O; major faults may be file-backed or swap. System swap counters include other processes. Correlate time series with Vulkan API wall/CPU timings and FPS."}


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("jsonl", type=Path)
    p.add_argument("--after", type=float, default=0)
    a = p.parse_args()
    print(json.dumps(summarize([json.loads(line) for line in a.jsonl.read_text().splitlines() if line.strip()], a.after), indent=2))
