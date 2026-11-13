# ADR-0318: NetSurf 3.11: its libraries, its own fonts for now, and the pinned source set

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0109, ADR-0140, ADR-0141, ADR-0162, ADR-0310, ADR-0312, ADR-0317
- **Evidence:** CP-25, CP-27

## Context
TASK-27 9d asks for NetSurf's framebuffer front end with libcurl and OpenSSL from 9a, image libraries, and fonts "through
NetSurf's own or through `zoraya-text` behind a small C interface (decide, record)". Read in this session: the release
`netsurf-all-3.11.tar.gz` (9.1 MB, SHA-256 `4dea880f...44b7b`) holds the browser (`netsurf`), `libnsfb`, `libcss`,
`libdom`, `libhubbub`, `libparserutils`, `libwapcaplet`, `libnsbmp`, `libnsgif`, `libnslog`, `libnspsl`, `libnsutils`,
`libsvgtiny`, `libutf8proc`, `nsgenbind`, `libpencil`, `librosprite`, `librufl`, `buildsystem`; it needs libpng and a
JPEG library, libcurl and OpenSSL from outside. The framebuffer front end has `font_internal.c` (bitmap glyphs, no
dependency) and `font_freetype.c` (FreeType and fontconfig). The build needs perl, gperf, flex, bison and a
`pkg-config`; the Mac has the first four, not `pkg-config`.

## Decision drivers
1. Licences: permissive for code the system links, GPL-2 for NetSurf itself with ADR-0109's precedent for GPL ports (CLAUDE.md).
2. Cost: `zoraya-text` is a Rust shaper over the system's fonts (ADR-0162); NetSurf asks glyph widths hundreds of times per page.
3. Pin every source by SHA-256 (INV-0109-4).
4. Scope held by decision (CP-27: what waits is written down; CP-25). Cyrillic and Latin Extended text on a page matter to the architect (ADR-0162's reason), but 9d's test pages are ASCII.

## Considered options

### Option A: FreeType and fontconfig as on Linux
- **Evidence against:** two more large ports, font files on the volume, fontconfig's configuration search; far more than 9d.
- **Cost:** *(estimate)* weeks.

### Option B: NetSurf's internal bitmap font for 9d, `zoraya-text` later
- **Description:** `font_internal.c` as built, glyph tables compiled in; Latin-1 only. The decision is reversible: a later
  ADR replaces `font_internal.c` with a shim over `zoraya-text` once ADR-0317's library exists.
- **Evidence for:** No dependency, no I/O, no cost; the test pages are ASCII; the screens are deterministic for the GUI test.
- **Evidence against:** Cyrillic pages show placeholders (a known limitation, written in the design document).
- **Cost:** *(estimate)* none.
- **Precedent:** NetSurf on RISC OS-less framebuffers (fbcon builds) ships this.

### Option C: `zoraya-text` behind a C interface now (Redox's orbtk analogue)
- **Description:** a Rust static library exports measure, split and paint functions over `zoraya-text`.
- **Evidence for:** Full Unicode and the system's fonts. **Against:** the library needs the font files in the program's
  namespace (a `+r /system/fonts` grant, new), plus an interface of three functions with state; measured cost unknown.
- **Cost:** *(estimate)* 1 to 2 weeks with tests.

## Recommendation
Recommended: **Option B**, with the source set below. It is a recommendation, not a decision.

1. **Pinned set** (`ports/sources.lock`): `netsurf-all` 3.11 (SHA-256 `4dea880ff3c2f698bfd62c982b259340f9abcd7f67e6c8eb2b32c61f71644b7b`),
   libpng 1.6.50 (`4df396518620a7aa3651443e87d1b2862e4e88cad135a8b93423e01706232307`), IJG libjpeg 9f
   (`04705c110cb2469caa79fb71fba3d7bf834914706e9641a4589485c1f832565b`; it needs no cmake and has the `jpeglib.h`
   NetSurf wants; libjpeg-turbo's SIMD gains do not pay on this target), and `pkg-config` 0.29.2
   (`6fc69c01688c9458a57eb9a1664c9aba372ccda420a02bf4429fe610e7e7d591`) as a host tool built into `build/` only when the host has none.
2. **Built by `make netsurf`** (`ports/Makefile`), after `make apps`: the NetSurf libraries into `$(DEPS)`, then
   `netsurf` with `TARGET=framebuffer`, `NETSURF_FB_FRONTEND=zoraya`, `NETSURF_USE_CURL=YES`, `NETSURF_USE_OPENSSL`-less
   (curl's own OpenSSL), JPEG and PNG YES, no WebP, no video, no JS (NetSurf's duktape is off), no Lua; the result is
   `bin/netsurf` in `build/rootfs-apps`.
3. **Network and files.** NetSurf's own cache and cookies live in the data directory the shell designates; it reads no
   `Choices` file and no environment variable beyond `HOME`-less defaults (the front end is patched like curl's, INV-0310-4's rule).
4. **Licences** (`docs/dependencies.md`): NetSurf GPL-2.0, its libraries MIT, libpng the libpng licence, IJG's licence, pkg-config GPL-2.0 (host tool, not linked).

5. **As built by step 2 of 9d** (amendment; the recommendation stands, this records where reality differed).
   JavaScript is off, so `nsgenbind` is not built (the first open question: moot). libsvgtiny is not built: it needs
   libdom's XML binding, which needs expat, a port of its own; SVG images show as missing until a later ADR. The
   libraries are `libnslog libwapcaplet libparserutils libcss libhubbub libdom libnsbmp libnsgif libnsutils libutf8proc
   libnspsl libnsfb` with `WITH_EXPAT_BINDING=no`. The build also needs bison 3 or later (libnslog's grammar; macOS
   ships 2.3): the pinned bison 3.8.2 is built into `build/ports/host` only when the host's is older, as pkg-config is
   (the second open question: yes, `PKG_CONFIG_LIBDIR` set to `build/ports/deps/lib/pkgconfig` satisfies the search);
   zlib and libpng are built for the host too, because NetSurf's `convert_image` links them. One patch
   (`ports/patches/netsurf/0001`) drops a typed `%destructor`. The names `libc.a` hides or lacks (`atol`, `strncasecmp`,
   `modf`, `iconv*`, `dirfd`, `scandir`) come from `ports/apps/nsshim.c` linked into `netsurf` as libzshim.a is into
   curl, so the surface of `libc.a` does not change. libcurl's `Libs:` line is edited to name the static link's
   libraries. Resources go to `/usr/share/netsurf` (`NETSURF_FB_RESPATH`), which the toolchain set (`/usr` to read, ADR-0153 rule 10) already gives, so the description needs no grant for them.

## Consequences
- **Positive.** A browser with no font port; the set is small and pinned.
- **Negative.** Pages in Cyrillic show boxes until a later ADR (recorded in `docs/design.md` when 9d lands).
- **Follow-up.** The shim over `zoraya-text`; JavaScript is out of scope (NetSurf's duktape is a port of its own).

## Invariants
- **INV-0318-1.** Every archive `make netsurf` unpacks is in `ports/sources.lock` with its SHA-256 and verified by `make fetch-ports`. *Check:* `make check-ports`.
- **INV-0318-2.** No NetSurf code is compiled with `__linux__` defined and none reads a file outside the grants of its line. *Check:* the log check at the end of `make netsurf` and `make test-netsurf`.

## Open questions
- Does NetSurf 3.11's `nsgenbind` build for the host (it is a host tool run during the build)?
- Is `Makefile.config`'s `PKG_CONFIG` search satisfied by a `pkg-config` built into `build/`?

## Revisit triggers
- A page the architect names that shows boxes where text is expected; a measured text-measure cost above 5% of a page load.

## Architect's decision
