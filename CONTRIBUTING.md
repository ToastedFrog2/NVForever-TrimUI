# Contributing

Report reproducible problems with the device/firmware version, port version, relevant settings and steps to reproduce. Remove names, local paths, addresses and other identifying information from reports and logs. Never attach game data, saves or credentials.

## Clean-room rules

- No leaked, disassembled or decompiled Bethesda/Gamebryo code or debug symbols, and no DRM circumvention.
- Describe format and behavior facts from public documentation or observations of legally owned game data. Cite sources and distinguish documented facts, observations and hypotheses.
- Never copy game files, proprietary program text or game-derived dumps into the repository. The two documentation screenshots are illustrative images, not redistributable game assets.
- Keep the game data read-only. Settings, logs and saves belong in the port's own home directory.
- Preserve upstream and third-party copyright notices and licenses. Contributions must be available under GPL-3.0-or-later, with compatible licensing for dependencies.

## Changes and checks

Keep changes focused. Document behavior and build requirements; add meaningful regression coverage when needed. Do not weaken existing comparisons or tests to hide a regression.

```sh
cargo test --release -p fnv-sim -p fnv-game
cargo run --manifest-path engine/Cargo.toml -p nvforever --example idle_ui_check --no-default-features
```

The wider root workspace can be checked with `cargo test --workspace --release`. Hardware changes need testing on a TrimUI Smart Pro, including loading, menus, controls, save compatibility, driver stability and temperature. Use matched scenes and separate diagnostic runs before claiming performance gains.
