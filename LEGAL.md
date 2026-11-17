# Legal notice

NVForever is a free, open-source project: a clean-room reimplementation of the engine
behaviour of *Fallout: New Vegas*, written from scratch in Rust. It is an engine only. To
do anything with it you need the data files of your own lawfully purchased copy of the
game.

This page describes how the project is run. It is not legal advice.

## Independence

NVForever is an independent, non-commercial project made by volunteers. It is not
affiliated with, endorsed by, sponsored by or otherwise connected to Bethesda Softworks,
ZeniMax Media, Microsoft, Obsidian Entertainment, Valve, or any other company involved in
making or publishing *Fallout: New Vegas* or the middleware it uses.

- NVForever contains no code from the original game, from its editor (the G.E.C.K.) or
  from the Gamebryo engine.
- NVForever is not a replacement for buying the game. It does not run without the
  original data files and does not help anyone obtain them.
- NVForever does not circumvent copy protection. The Steam executable's code section is
  encrypted and the project does not unpack it.
- NVForever does not modify the original game. It never writes into the game folder or
  into `Documents\My Games\FalloutNV`; the game's `Data` folder is read-only input
  ([docs/design/install-layout.md](docs/design/install-layout.md)).

## Trademark acknowledgements

*Fallout*, *Fallout: New Vegas* and related names and logos are trademarks or registered
trademarks of Bethesda Softworks LLC, a ZeniMax Media company, in the United States
and/or other countries. *Fallout: New Vegas* was developed by Obsidian Entertainment.

Gamebryo, Havok, SpeedTree, FaceGen, Bink, Steam, Windows and all other product and
company names mentioned in this repository are trademarks of their respective owners.

These names are used only to say what NVForever is compatible with and which file formats
and behaviours it describes. Their use does not imply any affiliation or endorsement, and
the project claims no rights in them. The name "NVForever" refers to this project only.

## No game files distributed

The downloadable engine contains no original game plugins, archives, meshes,
textures, animations, audio, saves or original game executable. Users supply their
own lawfully purchased game data. The release includes this project's executable;
the repository includes illustrative gameplay screenshots captured with separately
owned game data. Those screenshots are not game assets for installation or reuse.

Private reference outputs, dumps and test captures are not distributed. The
upstream parity/capture test tools are not included in this TrimUI source export.

What the repository does contain is our own description of the game's file formats and
behaviour (`docs/spec-notes/`, `schemas/`) and the tools that read your copy of the data.
Those descriptions name records, fields, Editor IDs and file paths where that is needed to
explain a format or cite an observation. Quotations from documentation are kept short and
cite their source; descriptions of scripting functions are written in our own words.

The engine itself shows a handful of short on-screen phrases that are not in the data
files and so cannot be read from your copy, such as a terminal's prompts. These are kept
to plain functional words and are tagged where they are defined (for the terminals,
[docs/spec-notes/hacking.md](docs/spec-notes/hacking.md) §8). Where the original's
wording names a brand from the game's fiction, NVForever uses its own wording instead,
and built-in labels the data files lack are written by the project, not transcribed
from the original. The
game's own dialogue, quest text, messages, names, notes and menu layouts are read at run
time from your game's files and are not in this repository.

## How we learnt the game's behaviour and recreated it

NVForever follows a clean-room process, in the spirit of OpenMW.

**Sources we use**

1. *Public documentation.* Community and vendor documentation of the file formats and
   the scripting language: the UESP format pages, the G.E.C.K. wiki, Microsoft's DDS
   documentation, the xEdit project's record definitions (Mozilla Public License 2.0) and
   the niftools `nif.xml` description of NIF blocks. xEdit's definitions and `nif.xml`
   are read as documentation and re-expressed in this project's own data format
   (`schemas/`); no code from either is included and none is generated from them
   ([schemas/README.md](schemas/README.md)).
2. *The data files of a lawfully purchased copy.* We read the game's own archives and
   plugins and record what is in them: counts, layouts that decode with nothing left
   over, files that can be rewritten byte for byte.
3. *The running original game.* Upstream observes the unmodified game through
   public interfaces and its script language. The upstream capture methodology is
   described in [the retained specification](docs/spec-notes/xnvse-capture.md).
   The capture plugin itself is not part of this distribution.

**Sources we do not use**

- No leaked source code or debug symbols, from any platform.
- No decompiled or disassembled game or editor code. Of the game's executable, only
  unencrypted data was read (version resource, import table, plain strings and the type
  names they include), to identify the game build and the third-party components it uses
  ([docs/spec-notes/layer0-install.md](docs/spec-notes/layer0-install.md)).
- No code or notes from other projects whose provenance does not meet these rules; such
  projects are not opened. Before this rule was tightened (2026-10-30), one such project,
  nv-rs, whose own policy admits leaked debug symbols and translated decompiled code, was
  used once, to cross-check NIF animation layouts already taken from `nif.xml` and the
  data. No code or layout was taken from it, and that cross-check has since been
  replaced by the data alone: every animation file in the game and its add-ons decodes
  exactly and re-encodes byte for byte ([docs/design/layer2.md](docs/design/layer2.md)).

**From notes to code**

- Every format and behaviour is first written down in plain language in
  [docs/spec-notes/](docs/spec-notes/). Each factual claim is tagged `[documented]` (with
  the source cited), `[observed]` (measured on a real install or in the running game) or
  `[hypothesis]` (not yet verified). Offsets, opcodes and field meanings are never
  invented.
- The implementation is written from those notes, not from the original program.
- Upstream uses independent readers and captures from the original game for
  parity checks. Those test tools and private reference materials are omitted from
  this export. This port's checks are documented in [BUILDING.md](BUILDING.md)
  and [trimui/FPS-WORK.md](trimui/FPS-WORK.md).

Contributions must follow the same provenance rules and be work the contributor has
the right to submit; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

NVForever is free software, licensed under the **GNU General Public License, version 3 or
(at your option) any later version** (`GPL-3.0-or-later`). The full text is in
[LICENSE](LICENSE). Copyright belongs to the NVForever contributors.

The program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

This licence covers NVForever's own code, schemas and documentation. It does not cover,
and grants no rights in, *Fallout: New Vegas* or its data files, which remain the property
of their owners and subject to the terms under which you bought the game.

Third-party components retain their own licences. Each downloadable binary and
source archive includes [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt), with
component versions, source locations, copyright notices and licence texts. The
locked TrimUI release graph includes normal and build dependencies conservatively;
development dependencies are excluded. Bundled font and native runtime notices
are included as well. The source includes the pinned supplemental texts and
machine-readable inventory under [licensing/](licensing/).

The generator and packaging checks are documented in [BUILDING.md](BUILDING.md).
Changing dependencies requires regenerating and reviewing the notices before release.
