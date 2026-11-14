| job | n | job median | job worst | Run step median | Run step worst |
|---|---|---|---|---|---|
| `make test-fuzz FUZZ_SECONDS=600 FUZZ_JOBS=5` | 78 | 11:33 | 12:38 | 10:28 | 10:38 |
| `make test-fuzz-net FUZZ_SECONDS=600 FUZZ_JOBS=3` | 77 | 11:29 | 12:53 | 10:23 | 10:30 |
| `make test-posix POSIX_SCRIPTS="tests/posix/threads.script" POSIX_ACCELS="auto tcg"` | 40 | 11:02 | 15:00 | 9:18 | 13:20 |
| `make test-posix-threadtest3` | 34 | 10:02 | 12:59 | 8:49 | 11:20 |
| `make test-net` | 117 | 9:51 | 14:15 | 8:54 | 12:33 |
| `make test-gui GUI_RUNS="rooms:auto:1 rooms:tcg:1 limits:auto:1 limits:tcg:1 spinners:auto:1 spinners:tcg:1 files:auto:1 files:tcg:1 files-manage:auto:1 files-manage:tcg:1"` | 77 | 9:47 | 11:25 | 8:37 | 10:39 |
| `make test-gui GUI_RUNS="terminal:auto:2 terminal:tcg:2 windows:auto:2 wheel:auto:1 wheel:tcg:1"` | 9 | 8:21 | 9:42 | 6:36 | 8:35 |
| `make test-apps` | 40 | 7:42 | 13:33 | 6:32 | 11:41 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w9-term.script" POSIX_ACCELS="tcg"` | 118 | 7:23 | 11:11 | 6:26 | 9:04 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w9-term.script" POSIX_ACCELS="auto"` | 120 | 7:11 | 9:56 | 6:15 | 8:52 |
| `make test-gui GUI_RUNS="terminal:auto:1 terminal:tcg:1 windows:auto:1 windows:tcg:1 layouts:auto:1 layouts:tcg:1 settings:auto:1 settings:tcg:1 net:auto:1 net:tcg:1"` | 83 | 7:04 | 9:15 | 5:51 | 7:42 |
| `make test-gui GUI_RUNS="terminal:auto:1920x1200 files-manage:auto:2 terminal:auto:1600x1000 wheel:auto:2"` | 4 | 6:52 | 8:46 | 5:47 | 7:40 |
| `make test-unit` | 114 | 5:44 | 7:06 | 4:46 | 6:02 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w8-j1.script" POSIX_ACCELS="auto"` | 121 | 5:44 | 8:17 | 4:49 | 7:00 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w8-pipe-j4.script" POSIX_ACCELS="auto"` | 122 | 5:35 | 8:03 | 4:11 | 5:40 |
| `make test-tour-gui POSIX_ACCELS="tcg"` | 116 | 5:29 | 12:32 | 4:34 | 8:28 |
| `make test-tour-gui POSIX_ACCELS="auto"` | 118 | 5:29 | 7:38 | 4:26 | 5:21 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w8-fifo-j4.script" POSIX_ACCELS="auto"` | 119 | 5:27 | 7:37 | 4:14 | 5:44 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w8-pipe-j2.script" POSIX_ACCELS="auto"` | 119 | 5:26 | 7:35 | 4:08 | 5:48 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w8-fifo-j2.script" POSIX_ACCELS="auto"` | 121 | 5:20 | 7:03 | 4:04 | 5:45 |
| `make test-python` | 19 | 5:12 | 27:08 | 4:11 | 26:10 |
| `make test-qemu-auto-smp4-2g` | 120 | 5:04 | 7:54 | 4:01 | 6:03 |
| `make test-qemu-tcg-smp4-gicv3` | 122 | 5:00 | 7:53 | 4:00 | 6:11 |
| `build` | 138 | 4:55 | 25:01 | - | - |
| `make test-python-regr` | 19 | 4:53 | 6:11 | 3:43 | 4:45 |
| `make test-qemu-tcg-smp4-gicv2` | 120 | 4:46 | 7:30 | 3:39 | 5:17 |
| `make test-qemu-tcg-default` | 123 | 4:44 | 7:11 | 3:32 | 5:13 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w9-int.script" POSIX_ACCELS="auto tcg"` | 119 | 4:43 | 6:03 | 3:37 | 4:59 |
| `make test-qemu-auto-default` | 121 | 4:30 | 6:51 | 3:33 | 5:21 |
| `make test-qemu-tcg-el2` | 123 | 4:28 | 6:46 | 3:22 | 5:00 |
| `make test-qemu-auto-el2` | 120 | 4:25 | 6:31 | 3:21 | 5:40 |
| `make test-netsurf` | 27 | 4:25 | 9:11 | 3:00 | 7:45 |
| `make test-posix POSIX_SCRIPTS="tests/posix/w10.script" POSIX_ACCELS="auto tcg"` | 121 | 3:43 | 5:53 | 2:39 | 3:30 |
| `make test-posix POSIX_SCRIPTS="tests/posix/hello.script tests/posix/w5.script" POSIX_ACCELS="auto tcg"` | 123 | 3:21 | 4:56 | 2:28 | 2:47 |
| `make test-posix-threads` | 43 | 3:11 | 4:29 | 1:58 | 3:28 |
| `make test-files` | 135 | 3:10 | 6:17 | 2:09 | 2:43 |
| `make test-posix-gui` | 120 | 2:58 | 4:40 | 2:01 | 2:58 |
| `make test-boot` | 132 | 2:37 | 4:16 | 1:40 | 2:01 |
| `make test-posix POSIX_SCRIPTS="tests/posix/tour.script" POSIX_ACCELS="auto"` | 119 | 2:31 | 4:08 | 1:29 | 1:53 |
| `make test-posix POSIX_SCRIPTS="tests/posix/tour.script" POSIX_ACCELS="tcg"` | 119 | 2:30 | 3:41 | 1:27 | 1:53 |
| `make test-hand-run` | 73 | 2:26 | 3:37 | 1:22 | 1:51 |
| `make test-posix POSIX_SCRIPTS="tests/posix/zosh.script" POSIX_ACCELS="auto tcg"` | 119 | 2:25 | 4:56 | 1:30 | 1:55 |
| `make test-posix POSIX_SCRIPTS="tests/posix/scenarios.script" POSIX_ACCELS="auto tcg"` | 123 | 2:24 | 4:49 | 1:30 | 1:46 |
| `make test-posix POSIX_SCRIPTS="tests/posix/invariants.script" POSIX_ACCELS="auto tcg"` | 123 | 2:13 | 3:26 | 1:17 | 1:36 |
| `make test-crash` | 124 | 2:09 | 3:17 | 1:09 | 1:25 |
| `make test-libposix` | 122 | 1:54 | 3:34 | 0:55 | 1:28 |
| `make test-keeper` | 83 | 1:48 | 3:09 | 0:48 | 1:00 |
| `make test-posix-suites` | 43 | 1:43 | 3:35 | 0:44 | 1:00 |
| `plan` | 161 | 0:16 | 0:27 | - | - |
| `image` | 423 | 0:08 | 0:30 | - | - |
