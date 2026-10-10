# The SeedLock logo

Copyright (c) 2026 The SeedLock authors. All rights reserved.

The SeedLock logo is **not** covered by the MIT license that applies to the rest of this repository (the `LICENSE`
file in the main folder). This covers every file in this folder (the logo files and `make_logo.py`, which draws
them) and the design they show: the bird cage with the seed inside, and the SeedLock lettering.

**What you may do:** show the logo, unchanged, to refer to SeedLock itself. For example, next to a link to this
repository, or in an article or review about SeedLock. Forks made to contribute to SeedLock may keep the logo as it
is.

**What needs the authors' written permission:** any other use. In particular, do not use the logo, or a logo made to
look like it, for other software or services, or in a way that suggests that a program is SeedLock, or is made or
endorsed by its authors, when it is not. A modified version that you distribute as a program of your own must not
carry the logo: remove it, and do not present your version as SeedLock.

## The files

| File | What it is for |
| --- | --- |
| `seedlock-logo.svg` | The logo, at the top of the main README |
| `seedlock-icon.svg`, `seedlock-icon.png` | The cage on its own (512 x 512): an avatar or an icon. It stays inside the circle GitHub crops avatars to |
| `seedlock-social-preview.svg`, `seedlock-social-preview.png` | GitHub's social preview (1280 x 640): Settings > General > Social preview |
| `make_logo.py` | Draws all of them: `python3 docs/logo/make_logo.py` (needs `fonttools`, `uharfbuzz`, IBM Plex Sans Bold and `rsvg-convert`; see the file) |

The colours are slate `#34454c`, porcelain `#fffefc` and copper `#bb7125`. The lettering is IBM Plex Sans Bold, used
under the SIL Open Font License; the logo files contain it as outlines, so no font is needed to show them. Each file
also carries this rights notice: as a comment in the SVG files, and as the "Copyright" text field in the PNG files.
