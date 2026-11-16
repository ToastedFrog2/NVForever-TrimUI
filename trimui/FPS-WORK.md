# 0.3 performance test build

No new FPS result is claimed until matched tests on the TrimUI. Rendering quality,
streaming, GPU barriers and swapchain behavior are unchanged by this work.

## Changes

- Skip Bevy UI layout and stack rebuilds while their inputs are unchanged. Node,
  content measurement, transform, hierarchy, scroll, font size, outline, fixed-node,
  viewport and rem-size changes invalidate the cache. Interaction, text measurement,
  clipping and animation systems remain active. `NVF_IDLE_UI=0` disables this cache
  for comparisons. The admin panel avoids writing unchanged text/font values.
- Backport the animation-table portion of upstream [e12c79d31](https://github.com/LootJunkieAzza/NVForever/commit/e12c79d3126de00d60919e37154fe6574b66c51b)
  (LootJunkieAzza, November 12): build object-animation reference/actor lookups only
  when there are script requests or resets. Existing clips, newly streamed rigs and
  reset handling continue. The upstream commit's codec, spline and load-order changes
  are not included in this candidate.
- Clock global form IDs are resolved once per World, while their current values are
  still read each time. Enable-parent queries reuse placement lookups and avoid
  heap allocation for ordinary references and short chains; cycle/missing-parent
  fallback and runtime flags are preserved.
- Package-expiry and remembered concussion checks visit deterministic active lists
  instead of every `Game::extra` entry. Empty package lists skip the calendar read.
  Lists are derived state rebuilt from the existing save fields on restore; no save
  format change. Engine writers of package timers/head-crippled state maintain these
  lists. Embedders directly mutating the public `extra` fields must also maintain
  this invariant; use the engine APIs or restore a snapshot after bulk state edits.
- Opt-in Vulkan API measurements cover all engine fence waits, image acquire,
  queue submit, present and idle waits. Each reports call count, wall/CPU/off-CPU
  milliseconds per presented frame and the slowest call in the 10-second interval.
  The normal FPS log also includes p99 frame time.

## Local checks and builds

```sh
cargo test --release -p fnv-sim -p fnv-game
cargo run --manifest-path engine/Cargo.toml -p nvforever --example idle_ui_check --no-default-features
CARGO_TARGET_DIR="$PWD/engine/target-trimui-profile" NVF_CARGO_PROFILE=release bash trimui/build.sh
python3 trimui/package-portmaster.py --output trimui/release/0.3-candidate/nvforever.zip
python3 trimui/package-source.py --output trimui/release/0.3-candidate/NVForever-0.3-source.zip
```

The headless check compares actual Bevy layout geometry/transforms with caching off
and on through mutations, then checks that idle frames skip both work sets. It uses
no game data or graphics device. The package has `Enabled=0` in `dev-menu.ini` and
contains no game data or user saves. The existing device override
`home/dev-menu.ini` with `Enabled=1` keeps the developer menu enabled across updates.

## Device measurements (pending)

Keep the known working 0.2 binary/package for rollback until this build passes.
Test from the same save and camera, with fixed warm-up and comparable temperature.
Alternate 0.2/0.3/0.2/0.3 runs with untouched controls. Compare steady-state FPS,
mean/p95/p99 frame time, scene counts/screenshots, clock/temperature, driver recovery
counters and save checksums. Then repeat a fixed travel route to examine stutter.
Use separate diagnostic runs so profiling overhead is not counted as an improvement.

For diagnostics set `NVF_GPU_REPORT=1` in the launch environment, then run this on
the device while the game is running (the firmware's Python environment is needed):

```sh
python3 tools/frame-costs.py "$(pidof nvforever)" 90 1 > /tmp/nvf-frame-costs.jsonl
python3 tools/summarize-frame-costs.py /tmp/nvf-frame-costs.jsonl --after 20
```

Match telemetry elapsed time to the game log. High Vulkan wall time with low thread
CPU time identifies off-CPU time in that API, which can include driver waits,
scheduling and locks. It is not a GPU timestamp or sufficient proof of GPU overload.
CPU time close to wall time suggests active driver/API work. Check per-thread CPU
ticks/wchan and GPU utilization before assigning a cause. Fence times include the
fence-pool lock and already-completed checks. Calls on multiple threads can overlap;
do not add all categories and treat the sum as a serial frame breakdown. Rare device
teardown idle calls can also appear in the final interval.

The telemetry distinguishes process major/minor faults, process storage I/O and
RSS/swap from system-wide swap-in/out counters. A major fault can read either a
file-backed page or swap; a minor fault is not evidence of SD I/O. A growing swap
allocation alone does not show ongoing swap-in stalls. Investigate coincident
major-fault/read/swap spikes before changing caches or streaming budgets.

At launch and after every game exit, keep the device awake:

```sh
touch /tmp/stay_awake /tmp/stay_alive
echo nvf_admin_session > /sys/power/wake_lock
```
