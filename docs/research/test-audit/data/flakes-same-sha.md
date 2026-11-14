Jobs that failed and passed on the same commit (another attempt or another run).

| when | sha | job | failed in run (attempt) | job id | ref | took | passed in |
|---|---|---|---|---|---|---|---|
| 2026-11-04T15:01:09Z | 7d29d3b5 | `make test-posix POSIX_SCRIPTS="tests/posix/scenarios.script" POSIX_ACCELS="auto tcg"` | 41853386993 (1) | 126657492311 | stage-6e | 3:53 | 41851466645 (1), 41853389038 (1) |
| 2026-11-09T01:00:15Z | f984e215 | `make test-qemu-tcg-smp4-gicv2` | 42543187652 (1) | 128924963319 | main | 2:41 | 42623787078 (1), 42623787456 (1) |
| 2026-11-09T13:14:55Z | f984e215 | `make test-qemu-auto-el2` | 42623787078 (1) | 129184272142 | stage-7a | 2:28 | 42623787456 (1), 42543187652 (1) |
| 2026-11-09T13:14:55Z | f984e215 | `make test-qemu-auto-default` | 42623787078 (1) | 129184272200 | stage-7a | 2:34 | 42543187652 (1) |
| 2026-11-09T13:14:55Z | f984e215 | `make test-qemu-auto-default` | 42623787456 (1) | 129184322375 | zosh-1 | 2:43 | 42543187652 (1) |
| 2026-11-09T13:14:55Z | f984e215 | `make test-qemu-tcg-default` | 42623787456 (1) | 129184322396 | zosh-1 | 2:39 | 42623787078 (1), 42543187652 (1) |
| 2026-11-09T16:48:26Z | 75069d7f | `make test-tour-gui POSIX_ACCELS="auto"` | 42658458396 (1) | 129305507542 | candidate/test-tiers | 21:15 | 42658458396 (2) |
| 2026-11-09T19:10:16Z | 7d3d1158 | `make test-posix POSIX_SCRIPTS="tests/posix/scenarios.script" POSIX_ACCELS="auto tcg"` | 42680764773 (1) | 129376324909 | tools/test-tiers | 3:56 | 42680764773 (2) |
| 2026-11-09T19:47:01Z | 1b618437 | `make test-posix POSIX_SCRIPTS="tests/posix/scenarios.script" POSIX_ACCELS="auto tcg"` | 42686638904 (1) | 129401114952 | main | 4:22 | 42686638904 (2) |
| 2026-11-09T21:29:39Z | 91978190 | `make test-gui` | 42703006772 (1) | 129454663621 | fix/type-pacing | 5:07 | 42703006772 (2) |
| 2026-11-09T21:55:33Z | 495607f9 | `make test-posix POSIX_SCRIPTS="tests/posix/scenarios.script" POSIX_ACCELS="auto tcg"` | 42707071806 (1) | 129468590685 | work/7d | 4:06 | 42707071806 (2) |
| 2026-11-10T00:02:25Z | a3cf8456 | `make test-posix POSIX_SCRIPTS="tests/posix/scenarios.script" POSIX_ACCELS="auto tcg"` | 42725195527 (1) | 129530911193 | work/7g | 4:04 | 42725195527 (2) |
| 2026-11-11T15:19:20Z | 9ffd57c6 | `build` | 43047183567 (2) | 130652650934 | candidate/work-ci-image | 1:31 | 43047183567 (1) |
| 2026-11-12T00:42:02Z | e40f612c | `make test-qemu-tcg-smp4-gicv3` | 43129188232 (1) | 130890733349 | candidate/work-q4-fix | 4:11 | 43129188232 (2) |
| 2026-11-12T00:42:02Z | e40f612c | `make test-qemu-tcg-smp4-gicv2` | 43129188232 (1) | 130890733272 | candidate/work-q4-fix | 4:05 | 43129188232 (2) |
| 2026-11-12T00:42:02Z | e40f612c | `make test-qemu-auto-smp4-2g` | 43129188232 (1) | 130890733417 | candidate/work-q4-fix | 4:00 | 43129188232 (2) |
| 2026-11-12T23:39:11Z | 722cb6ed | `make test-net` | 43330650492 (1) | 131576013818 | main | 14:54 | 43330650492 (2) |
| 2026-11-13T00:31:26Z | 57ca9ef7 | `make test-net` | 43337731455 (1) | 131601474712 | candidate/work-arch-decisions-2-a | 4:58 | 43337731455 (2) |
| 2026-11-13T08:34:05Z | c1e1b344 | `make test-qemu-auto-default` | 43394224522 (1) | 131791750118 | main | 5:47 | 43403177489 (1), 43394224522 (2) |
| 2026-11-13T13:54:34Z | 936db096 | `make test-boot` | 43437795275 (1) | 131930599217 | main | 5:43 | 43437795275 (2) |
| 2026-11-13T21:34:00Z | 7adf9388 | `build` | 43511821485 (1) | 132183892800 | main | 6:10 | 43511821485 (2), 43505870979 (1) |
