# Upstream performance review — November 16, 2026

Reviewed upstream `main` at `be6db0948d0f547c617a149a017fc4d89f4b87c3`.
The TrimUI port is based on `4d6e4bdf6` (November 4), plus its local port commits.
The following changes are newer than that base. Any measurements in upstream's
commits are their PC results, not measurements on the TrimUI.

| Change | Relevance / candidate status |
| --- | --- |
| [Avoid idle object-animation tables, faster spline spans, lazy NIF diagnostic paths, inline load-order definitions](https://github.com/LootJunkieAzza/NVForever/commit/e12c79d3126de00d60919e37154fe6574b66c51b), Nov 12 | **Object-animation table portion included** in 0.3. Spline evaluation could reduce animation CPU cost; codec/index allocation changes mainly concern loading and memory. Remaining portions need separate tests/backports. |
| [Cache aggression/kinship for each detection round](https://github.com/LootJunkieAzza/NVForever/commit/274d37133b280076fa62cc3944f7a00b97960772), Nov 8; [reuse ally checks and index watchers](https://github.com/LootJunkieAzza/NVForever/commit/1c304245ad69d5011aebbbffbffc84ff1ecf83d4), Nov 7 | Candidate for crowded combat; not included. Measure detection cost on this device first; previous quiet-view AI-off test showed no gain. |
| [Avoid repeated full controller sweeps for stuck NPCs](https://github.com/LootJunkieAzza/NVForever/commit/0482f19e0f01e42f2bc7c9ac93982d61171eb547), Nov 7 | Targets severe stuck-actor physics spikes. Not included; needs a stuck-NPC test and validation that actors resume when surroundings change. |
| [Poll completed GPU work off the render thread and finish encoders earlier](https://github.com/LootJunkieAzza/NVForever/commit/b7e7c7bf5), Nov 7 | Could reduce render-thread CPU cost. Not included pending driver/CPU measurements; extra polling/threading needs PowerVR testing. Upstream reports render-thread savings but no overall frame-time change in its three test scenes. |
| [Bound redressing work and reuse equipment facts](https://github.com/LootJunkieAzza/NVForever/commit/e0a3981995b95bf3173addc7bb51e32b8ca269f4), Nov 6; [separate asset-read task pool](https://github.com/LootJunkieAzza/NVForever/commit/513176e9e), Nov 6 | Potential travel/actor-spawn stutter work. Not included until faults, I/O and memory pressure are measured. A new thread pool can increase pressure on this 1 GB device. |

The newer performance overlay also provides frame graphs, loaded counts and CPU/GPU
breakdowns (starting with [b016113a8](https://github.com/LootJunkieAzza/NVForever/commit/b016113a8)).
It would need adapting to the handheld/menu and the driver's supported timestamp
queries. Current 0.3 diagnostics use CPU-side API times and `/proc` telemetry instead.

A full upstream merge includes extensive unrelated gameplay, UI, shader and launcher
changes. Keep the current controlled candidate for comparison; backport individual
cost reductions as the device measurements identify their relevance.
