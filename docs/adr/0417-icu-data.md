# ADR-0417: ICU with its data linked in statically, filtered to the image's languages after a full first build

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0100, ADR-0163, ADR-0342, ADR-0363, ADR-0410, ADR-0413, ADR-0415
- **Evidence:** CP-21, CP-22, DM-007, DM-097, browser-engine/survey.md#libraries-and-interfaces, browser-engine/survey.md#measurements, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WPE requires ICU 70.1 (data, i18n, uc) and HarfBuzz with ICU (browser-engine/survey.md#wpe-webkit-in-detail);
JavaScript's `Intl`, line breaking, collation and normalisation in WebKit go through it. ICU looks for its data in a
shared library by default, or a `.dat` archive or files; `--with-data-packaging=static` builds it as one large static
library; the Data Build Tool filters it by locale, by feature and by resource (browser-engine/survey.md#libraries-and-interfaces).
**Measured**: `icudt79l.dat` of ICU 79.1 is 33,558,160 bytes; ICU's code is 630,850 lines (browser-engine/survey.md#measurements).
ICU 64's documentation names its largest parts: conversion 4.9 MiB, break dictionaries 2.8, collation 2.8, time zones
2.7, currencies 2.5, locales 2.4.

Zoraya has no `dlopen` (denied, ADR-0342 item 12) and no file mapping (browser-engine/survey.md#the-posix-library);
libc++ uses the C locale only (ADR-0363). The browser is one executable that is also its Web and network processes
(ADR-0410 item 5); with ADR-0415 item 2 its helpers are spawned from the UI process's own image, so the static data is
in memory once; without ADR-0415 item 2, once per process. ICU also accepts its data from the
application by a pointer, `udata_setCommonData()` (browser-engine/survey.md#libraries-and-interfaces).

## Decision drivers

1. **No dynamic loading** (DM-007's search-path problems avoided; `dlopen` denied).
2. **No hidden locale state from the environment** (CP-21, DM-097): which locales exist is fixed by the image.
3. **UTF-8 only** (CP-22): ICU's converters exist for the web's legacy encodings, not for Zoraya's own text.
4. **Size** in the image and in memory, measured before it is cut.
5. **The person's languages**: the keyboard layouts are US, Ukrainian, Russian and Czech (ADR-0163).

## Considered options

### Option A: ICU as Linux distributions ship it: a shared `libicudata.so` with all data

- **Description:** The default `library` packaging.
- **Evidence for:** Every locale; the distribution default.
- **Evidence against:** Shared libraries do not exist here (driver 1).
- **Cost:** Not available.
- **Precedent:** Linux, FreeBSD packages.

### Option B: Static data linked into each program (`--with-data-packaging=static`), shared through read-only segments, filtered after a full build

- **Description:** ICU and its data as static archives; the data is read-only data of the browser's executable, held
  once in memory with ADR-0415 item 2 (every process maps the one image's frames); the first build uses all of it,
  the next one filters it.
- **Evidence for:** Drivers 1 to 5; no run-time loading; the task's choice.
- **Evidence against:** 33.6 MB in the executable and in memory until filtered; without ADR-0415 item 2, once per
  process (about 100 MB for three processes with the full data).
- **Cost:** *(estimate)* An ICU port with a filter file; a test list for the filter.
- **Precedent:** Embedded ICU builds; Fuchsia's components that carry their ICU data (Fuchsia ships `icudtl.dat` with
  its components, the survey did not fetch its documentation, so this is `[outside evidence]`).

### Option C: One `.dat` archive loaded by the UI process into a memory object and given read-only to each helper

- **Description:** `archive` packaging; the UI process reads `/usr/share/icu/icudt79l.dat` into a memory descriptor
  (ADR-0413), passes it read-only to each helper (ADR-0412), and each calls ICU's API for data supplied by pointer.
- **Evidence for:** One copy in memory without ADR-0415 item 2; programs smaller on disk.
- **Evidence against:** A patch to WebKit's process start-up (each helper maps the descriptor and calls
  `udata_setCommonData` before ICU's first use); a descriptor more per helper; the data is a file in the image that the
  UI process reads at start.
- **Cost:** *(estimate)* 150 lines of patch, a descriptor more per helper.
- **Precedent:** Chromium's `icudtl.dat` loaded and passed to its child processes (`[outside evidence]`, not fetched).

## Recommendation

**Recommendation (not a decision): Option B**, with Option C if ADR-0415 item 2 is not accepted. B is chosen because it
needs no patch to WebKit and, with ADR-0415 item 2, holds the data once.

1. **Packaging.** ICU4C (pinned, Unicode-3.0) built static with `--with-data-packaging=static`, `--disable-shared`,
   `-DU_STATIC_IMPLEMENTATION`, no tools in the image; `U_CHARSET_IS_UTF8=1`.
2. **First build: all data.** The engine's first part links the full data (33.6 MB measured for 79.1), so failures are
   never the filter's.
3. **Then the filter**, an `ICU_DATA_FILTER_FILE` in `ports/icu/`: locales `root`, `en`, `uk`, `ru`, `cs` (with their
   scripts); features kept: normalisation, break iteration rules for all scripts, collation for the five locales,
   time zones (JavaScript's `Intl.DateTimeFormat` needs them), currencies and units for the five; dropped: break
   dictionaries for CJK and South-East Asian scripts until fonts for them exist (ADR-0416 item 3), converters other
   than those WebKit's text codecs ask ICU for (found by the first build's log of `ucnv_open` names). Its size is
   measured and recorded in this ADR's open questions when built; target *(estimate)* under 12 MiB.
4. **Locale for the engine.** The engine's language comes from `LANG` given by the description (ADR-0320's
   environment policy), one of the five; anything else falls back to `en`.

### Effect of the vision

For target 2 the locales an agent's browser has are the image's, never chosen by the environment it was given. Target
3: the person's languages sort, break and format correctly in pages. Networking: none. Threads: ICU's caches are
thread-safe upstream.

### Examples

`new Intl.DateTimeFormat('uk', {dateStyle: 'full'}).format(new Date(2026, 10, 14))` in a page gives
`субота, 14 листопада 2026 р.`; with locale `ja` it gives the English fallback until `ja` is added to the filter.

## Consequences

- **Positive.** No loader; the person's five locales complete; a measured size.
- **Negative.** Every other locale falls back to English; filtering needs a test list kept with the filter.
- **Follow-up.** The ICU port; the filter file and its tests; the size measurement.

## Invariants

- **INV-0417-1.** No engine program opens an ICU data file at run time. *Check:* the engine's test step runs with a
  table that has no `/usr/share/icu` and reports any refused path; none is reported.
- **INV-0417-2.** The locales in the built data are exactly those of the filter file. *Check:* a host test that lists
  the data's locales with ICU's own tool on the host build and compares with the filter.

## Open questions

- The filtered size, and which converters WebKit asks ICU for.
- Whether HarfBuzz's ICU integration needs anything beyond `uc`.

## Revisit triggers

- ADR-0415 item 2 rejected: Option C.
- A locale asked for by the person: a line in the filter.

## Architect's decision
