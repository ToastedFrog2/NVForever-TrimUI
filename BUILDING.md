# Building the TrimUI release

Build on macOS or Linux with Rust, the `aarch64-unknown-linux-gnu` target, LLVM tools, Zig, cargo-zigbuild, Python 3, CMake, Ninja and Git. The target is Cortex-A53 ARM64 Linux with glibc 2.33. Dependency versions are pinned in the Cargo lockfiles.

```sh
rustup target add aarch64-unknown-linux-gnu
rustup component add llvm-tools
cargo install cargo-zigbuild --locked
```

The firmware's ALSA library is required: the newer Debian runtime requires a newer glibc. On the first build, set `TRIMUI_SSH` to your device's SSH destination so the build can fetch that library. Configure SSH outside this repository; never store passwords or private keys here.

```sh
TRIMUI_SSH=root@DEVICE-IP python3 trimui/build-release.py
python3 trimui/package-portmaster.py
python3 trimui/package-source.py
```

`build-release.py` requires a clean committed source tree. It prepares the SDK/optimizer once, builds an exact tracked snapshot in an automatically cleaned temporary directory and copies the finished package tree to `trimui/dist/`. It remaps compiler paths and avoids embedding a developer's checkout path through manifest-directory constants. Release builds use the `release` Cargo profile by default; `NVF_CARGO_PROFILE=trimui` enables the alternative fat-LTO profile for separate testing.

Outputs:

- `trimui/release/nvforever.zip`: PortMaster archive, developer menu off, no game/home folders.
- `trimui/release/NVForever-0.3.1-source.zip`: corresponding maintained source.
- SHA-256 files alongside the archives.

The first SDK preparation downloads the ALSA development headers and builds a pinned SPIRV-Tools optimizer from source. `TRIMUI_SYSROOT`, `TRIMUI_DEPS` and `CARGO_TARGET_DIR` can select existing SDK, optimizer and build-cache locations. These paths are environment settings and are not committed.

For ordinary local development, `bash trimui/build.sh` also builds directly in the checkout. Use the staged release helper for distributable binaries. Generated SDKs, caches, build outputs and archives are ignored by Git.

## Checks

```sh
cargo test --release -p fnv-sim -p fnv-game
cargo run --manifest-path engine/Cargo.toml -p nvforever --example idle_ui_check --no-default-features
```

Before distributing, verify ZIP checksums and layout, developer-menu default, shared-library requirements, license/source completeness, screenshot metadata and identifying information in source and compiled files. Test the port on the target hardware. Keep diagnostics separate from matched FPS comparisons.

Launcher/swap regression checks (temporary files and simulated Linux utilities):

```sh
python3 -m unittest discover -s trimui/tests -v
```
