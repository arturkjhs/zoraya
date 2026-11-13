# GitHub's free arm64 runners for the QEMU jobs (experiment, 2026-11-13)

Written on the architect's instruction (an exception to the rule that `docs/research/` is a historical record).

Question: do `ubuntu-24.04-arm` runners (4 vCPU, 16 GB, free for public repositories) make our QEMU jobs faster than
`ubuntu-24.04` (x86-64, everything under TCG)?

Answer: no. There is no KVM on them, so QEMU runs under TCG there too, and TCG on the arm runner (Neoverse-N2) was
1.3 to 1.8 times slower than on the x86 runner in every job measured. Nothing should move.

## Method

`.github/workflows/ci-arm.yml` (this branch only; `test.yml` and the main matrix are unchanged). A `workflow_dispatch`
needs the file on the default branch, which this experiment must not touch, so the workflow also starts on a push to
`work/ci-arm` (the architect's decision); no other branch or tag starts it. Run
https://github.com/arturkjhs/zoraya/actions/runs/43447464141 (a first run,
43447010764, failed at once on both legs because a `qemu-system-aarch64 --version` before the install exited 127 under
`bash -e`, and was cancelled).

Toolchain path chosen: install on the bare runner (`tools/ci-apt-install.sh` with the Dockerfile's package list, then
`rustup toolchain install`), not a multi-arch CI image. It is the cheaper path: the bare-runner fallback of `test.yml`
already exists, a multi-arch image would need a Dockerfile review and a second image build, and the experiment
needed no cache. Both legs are cold (no cargo, ports or apt cache) and run the same steps one after another in one job:
`make fetch-ports`, `make ci-build`, then the four commands as steps of their own (times are from
`gh run view --json jobs`, step start to completion).

## KVM

The `probe` job (same run, job `probe`, arm runner) reports: `aarch64`, Neoverse-N2, QEMU 8.2.2 (Ubuntu 24.04),
`ls: cannot access '/dev/kvm': No such file or directory`, and both `-accel kvm` probes (with and without
`gic-version=host`) exit 1 with `failed to initialize kvm: No such file or directory`. The runner is a VM without
nested virtualisation exposed to the job. So step 3 of the brief (a `kvm` accelerator in `tools/run-qemu.sh` and
`tools/test-runner`) was not done: it cannot be tested there.

For when KVM becomes available: `kvm` should count as hardware-accelerated like HVF. The runner's `runs_under_tcg`
already asks `run-qemu.sh --print-accel` and treats everything but `tcg` as native speed (`latency` bounds, screen poll),
so a `kvm` answer would get the strict limits; the guest-side limits keyed on the accelerator (t_bound, t_cpu) would
need the same decision then. Not changed here.

## Times

| Step | x86 TCG (ubuntu-24.04) | arm TCG (ubuntu-24.04-arm) | arm KVM |
|---|---|---|---|
| install QEMU, clang, tools | 0:32 | 0:16 | n/a |
| `make fetch-ports` | 0:20 | 0:28 | n/a |
| `make ci-build` | 5:40 | 6:06 | n/a |
| `make test-qemu-auto-default` | 3:13 | 4:48 | n/a |
| `make test-qemu-tcg-smp4-gicv3` | 3:44 | 5:54 | n/a |
| `make test-net` | 8:32 | 12:15 | n/a |
| `make test-posix` threads, `auto tcg` | 8:34 | 14:55 | n/a |
| whole job | 30:54 | 45:44 | n/a |

(`auto` is TCG on both: no HVF, no KVM.) The four test steps together: 24:03 on x86, 37:12 on arm. Ratios arm/x86:
1.49, 1.58, 1.44, 1.74. The build alone is 1.08 slower on arm. Install is faster on arm (it is a smaller download).
Queue time: both legs started within about 25 s of the push. One sample only; variance between runs was not measured.

## Recommendation

- Move nothing. The arm runners are slower for every QEMU job under TCG, and have no KVM; the only gain would be a
  possible future one if GitHub exposes `/dev/kvm` there (re-run the probe job to check; the workflow stays on this
  branch for that).
- Cost and risk if one moved anyway: the CI image is x86-64 only (a second, multi-arch image or the bare-runner
  install would be needed, with no cargo or ports cache shared with x86 since the keys differ by architecture);
  `tools/ci-apt-install.sh`'s extra mirrors are x86-only; results (timing-sensitive tests such as the `latency`
  bounds, `t_cpu`) would be measured on a different TCG host, which weakens reproducibility with the x86 baseline; the
  free arm queue was short in this one run but is not guaranteed.

## Limitations

One run per leg; cold caches; no `make test-qemu` configuration with KVM; QEMU 8.2.2 on both (the Ubuntu package),
not the 11.1.2 used on the Mac.
