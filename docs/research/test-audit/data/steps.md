
### steps of `build` jobs (successful jobs)

| step | n | median | p90 | worst |
|---|---|---|---|---|
| make ci-build | 254 | 3:21 | 8:10 | 23:43 |
| Initialize containers | 148 | 0:21 | 0:26 | 1:17 |
| Install the pinned Rust toolchain (rust-toolchain.toml) | 106 | 0:10 | 0:11 | 0:12 |
| Pack build/ and target/ | 254 | 0:09 | 0:14 | 0:19 |
| Install clang (zcc, libc.a and the ports) and zstd | 106 | 0:09 | 0:14 | 0:53 |
| Run actions/upload-artifact@v4 | 254 | 0:07 | 0:11 | 0:19 |
| Save the ports' archives | 2 | 0:04 | 0:04 | 0:04 |
| Stop containers | 148 | 0:03 | 0:05 | 0:06 |
| Restore the ports' archives | 14 | 0:02 | 0:03 | 0:03 |
| Run actions/checkout@v4 | 254 | 0:02 | 0:04 | 0:07 |
| Cache the ports' build | 165 | 0:02 | 0:04 | 0:11 |
| Post Run Swatinem/rust-cache@v2.7.5 | 165 | 0:02 | 0:03 | 0:04 |
| Set up job | 254 | 0:01 | 0:02 | 0:04 |
| Cache the ports' archives | 240 | 0:01 | 0:02 | 0:08 |
| Post Cache the ports' build | 165 | 0:01 | 0:06 | 0:10 |
| make fetch-ports | 254 | 0:00 | 0:01 | 0:45 |
| Post Cache the ports' archives | 240 | 0:00 | 0:01 | 0:09 |

### steps of `test` jobs (successful jobs)

| step | n | median | p90 | worst |
|---|---|---|---|---|
| Run | 8113 | 2:49 | 5:39 | 26:10 |
| Initialize containers | 5166 | 0:22 | 0:26 | 1:45 |
| Run actions/download-artifact@v4 | 8113 | 0:13 | 0:43 | 3:01 |
| Install QEMU, clang and zstd | 2947 | 0:12 | 0:23 | 7:36 |
| Install the pinned Rust toolchain (rust-toolchain.toml) | 2947 | 0:10 | 0:11 | 0:13 |
| Install the nightly toolchain and fetch the fuzz crates (ADR-0298) | 155 | 0:08 | 0:10 | 0:17 |
| Unpack, the sources older than what was built from them | 8113 | 0:06 | 0:12 | 1:58 |
| Run actions/checkout@v4 | 8113 | 0:02 | 0:04 | 2:33 |
| Set up job | 8113 | 0:01 | 0:02 | 0:09 |
| Stop containers | 5166 | 0:00 | 0:01 | 0:12 |

### steps of `check` jobs (successful jobs)

| step | n | median | p90 | worst |
|---|---|---|---|---|
| Initialize containers | 6397 | 0:22 | 0:26 | 3:10 |
| Install the pinned Rust toolchain (rust-toolchain.toml) | 2262 | 0:10 | 0:11 | 0:14 |
| Run actions/checkout@v4 | 8659 | 0:03 | 0:05 | 0:28 |
| Set up job | 8659 | 0:01 | 0:02 | 0:07 |
| Run Swatinem/rust-cache@v2.7.5 | 7149 | 0:01 | 0:01 | 0:06 |
| Run | 8605 | 0:00 | 0:05 | 0:48 |
| Post Run Swatinem/rust-cache@v2.7.5 | 7149 | 0:00 | 0:01 | 0:03 |
| Stop containers | 6397 | 0:00 | 0:01 | 0:03 |

### steps of `plan` jobs (successful jobs)

| step | n | median | p90 | worst |
|---|---|---|---|---|
| Install the pinned Rust toolchain (rust-toolchain.toml) | 196 | 0:10 | 0:11 | 0:13 |
| Run actions/checkout@v4 | 196 | 0:02 | 0:03 | 0:07 |
| Set up job | 196 | 0:01 | 0:01 | 0:03 |
| Plan | 196 | 0:01 | 0:01 | 0:03 |

### steps of `image` jobs (successful jobs)

| step | n | median | p90 | worst |
|---|---|---|---|---|
| Run actions/checkout@v4 | 447 | 0:02 | 0:04 | 0:08 |
| Find the image | 447 | 0:02 | 0:03 | 0:18 |
| Set up job | 447 | 0:01 | 0:01 | 0:04 |
