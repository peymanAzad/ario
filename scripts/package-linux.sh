#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 ]]; then
  echo "Usage: $0 VERSION LINUX_ARCHIVE OUTPUT_DIRECTORY" >&2
  exit 1
fi

export ARIO_VERSION="$1"
# Pacman orders an alphabetic prerelease suffix before the final version.
# Strip SemVer build metadata and retain prerelease identifiers in pkgver.
arch_version="${ARIO_VERSION%%+*}"
if [[ "$arch_version" == *-* ]]; then
  arch_version="${arch_version%%-*}pre.${arch_version#*-}"
  arch_version="${arch_version//-/_}"
fi
export ARIO_ARCH_VERSION="$arch_version"
archive="$2"
output="$3"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
config="$(pwd)/packaging/nfpm.yaml"
arch_config="$(pwd)/packaging/nfpm-arch.yaml"

tar -xzf "$archive" --strip-components=1 -C "$stage"
test -x "$stage/ario"
test -x "$stage/ario_daemon"
mkdir -p "$output"
output=$(cd "$output" && pwd)
(
  cd "$stage"
  nfpm package --config "$config" --packager deb --target "$output/"
  nfpm package --config "$config" --packager rpm --target "$output/"
  nfpm package --config "$arch_config" --packager archlinux --target "$output/"
)
(
  cd "$output"
  sha256sum ./*.deb ./*.rpm ./*.pkg.tar.zst > linux-packages.sha256
)
