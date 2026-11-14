# ADR-0363: Locale support for C++: the C locale only; musl's names accepted with the C rules

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0100, ADR-0360, ADR-0366
- **Evidence:** CP-27

## Context
libc++'s `<locale>`, `<iostream>` and `<regex>` are built on the C library's `*_l` functions (`newlocale`, `strtod_l`,
`isdigit_l`, `strftime_l` and the others) and cannot be built without localization: `_LIBCPP_HAS_LOCALIZATION 0` removes
`<iostream>` itself. musl implements locales as the C locale plus UTF-8 for the multibyte functions; it accepts any name for
`newlocale` and `setlocale` and keeps the name, with the C rules for numbers, dates and collation. Zoraya has no locale data
on the volume. Measured (`make test-cxx`): `std::locale()` is `"C"`, `std::locale("POSIX")` constructs, and
`std::locale("de_DE.UTF-8")` constructs, is named `de_DE.UTF-8` and has `'.'` as its decimal point.

## Decision drivers
1. `iostream` is required (TASK-30), so localization is on.
2. No locale data or service exists; adding one is a design of its own.
3. Honesty about what a name gives (ADR-0320's driver 4 for the CPython port).
4. No change to musl's code where its behaviour can be stated (CP-27).

## Considered options
### Option A: localization on, the C locale only, musl's behaviour for other names (the recommendation)
- **Evidence for:** drivers 1 and 2; no code; what musl systems (Alpine) do without locale packages.
- **Evidence against:** a program that asks for `de_DE.UTF-8` gets no error and the C rules (driver 3): the name is not checked.
- **Cost:** none.
- **Precedent:** Alpine Linux, Chimera Linux.
### Option B: localization off (`_LIBCPP_HAS_LOCALIZATION 0`)
- **Evidence against:** no `<iostream>`, `<fstream>`, `<regex>` or `<locale>` (driver 1).
### Option C: refuse every name but `C`, `POSIX` and `C.UTF-8` (a libposix `newlocale` that says `ENOENT`)
- **Evidence for:** driver 3: `std::locale("de_DE.UTF-8")` would throw `runtime_error`, as on glibc without the locale.
- **Evidence against:** changes musl's behaviour for C programs too; some programs set `LANG`-named locales at start and fail
  where they ran before.
- **Cost:** *(estimate)* 20 lines in libposix or a musl patch.
### Option D: locale data on the volume (glibc's or ICU's), as Linux distributions ship
- **Evidence against:** driver 2; a browser engine brings ICU for its own text handling, not through the C library.
- **Precedent:** glibc's `locale-gen`; Haiku's locale kit (alternatives-postmortem) is a system service with its own data.

## Recommendation
**Recommendation (not a decision): Option A.** C++ programs get the C locale; a named locale is accepted by musl and behaves
as `C` (with UTF-8 multibyte functions). Option C is the follow-up if a port relies on a locale name being refused.

## Consequences
- **Positive.** `iostream`, `fstream`, `regex` and `locale` work; nothing new to maintain.
- **Negative.** Formatting by locale (`std::put_money`, a German decimal comma, collation) is never what the name says.
- **Follow-up.** ICU for the browser's dependencies, which brings its own locale data.

## Invariants
- **INV-0363-1.** The global C++ locale of a program is `"C"`. *Check:* `make test-cxx` (`cxxtest iostream`).

## Open questions
- Whether `newlocale` should refuse names it has no data for (Option C), for C and C++ alike.

## Revisit triggers
- Locale data on the volume, or a port that needs a refused name.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

As recommended.
