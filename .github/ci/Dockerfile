# The image the CI jobs of .github/workflows/test.yml run in, so that no job
# needs Ubuntu's mirrors at run time. Built by .github/workflows/ci-image.yml
# and tagged with tools/ci-image-tag.sh (a hash of this file, of
# tools/ci-apt-install.sh and of rust-toolchain.toml). Build context: the
# repository root.
#
# Pinned by tag, not by digest: no digest could be read without network
# access. To take a newer base image, change the comment below (it changes
# the hash, so the image is rebuilt).
# base: ubuntu:24.04 as of 2026-11-11
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
COPY tools/ci-apt-install.sh /usr/local/bin/ci-apt-install.sh
# clang, lld, llvm: tools/zcc, libc.a and the ports; zstd: the build archive;
# qemu-system-arm: the tests (qemu-system-aarch64); the rest: the ports' and
# the host tools' builds and the checks (gperf, flex and bison: NetSurf's libraries and nsgenbind, 9d; ruby for make check-ci-jobs' YAML
# parse, git for actions/checkout, curl for make fetch-ports; cmake for
# GoogleTest's build with ports/cmake/zoraya.cmake, TASK-30).
RUN ci-apt-install.sh \
        build-essential ca-certificates clang lld llvm zstd qemu-system-arm \
        git curl patch perl python3 ruby xz-utils bzip2 gzip unzip zip file \
        gawk pkg-config gperf flex bison sudo cmake \
    && rm -rf /var/lib/apt/lists/*

# The workspace is owned by the runner's user, the job runs as root: without
# this, git refuses ("dubious ownership").
RUN git config --system --add safe.directory '*'

# The pinned Rust toolchain, installed to fixed places that every user can
# write (cargo puts its registry cache under CARGO_HOME).
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
# rustup-init's download is retried; the toolchain, with the components and
# targets of rust-toolchain.toml, is installed from that file (run in /tmp).
RUN curl -fsSL --retry 5 --retry-all-errors --retry-delay 5 https://sh.rustup.rs -o /tmp/rustup-init.sh \
    && cd /tmp \
    && sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain none \
    && rustup toolchain install \
    && rustup show \
    && chmod -R a+rwX /usr/local/rustup /usr/local/cargo \
    && rm -f /tmp/rustup-init.sh /tmp/rust-toolchain.toml
