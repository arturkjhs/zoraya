#!/bin/sh
# Print the tag of the CI image for this checkout: the first 16 hex digits of
# the SHA-256 of what defines the image. Both .github/workflows/ci-image.yml
# (which builds ghcr.io/OWNER/REPO/ci:TAG) and test.yml (which looks for that
# tag) call this, so they cannot disagree. Run from the repository root.
set -eu
files=".github/ci/Dockerfile tools/ci-apt-install.sh rust-toolchain.toml"
if command -v sha256sum >/dev/null 2>&1; then
	# shellcheck disable=SC2086
	cat $files | sha256sum | cut -c1-16
else
	# shellcheck disable=SC2086
	cat $files | shasum -a 256 | cut -c1-16
fi
