#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 ]]; then
  echo "Usage: $0 VERSION LINUX_ARCHIVE OUTPUT_DIRECTORY" >&2
  exit 1
fi

export ARIO_VERSION="$1"
archive="$2"
output="$3"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
config="$(pwd)/packaging/nfpm.yaml"

tar -xzf "$archive" --strip-components=1 -C "$stage"
test -x "$stage/ario"
test -x "$stage/ario_daemon"
mkdir -p "$output"
output=$(cd "$output" && pwd)
(
  cd "$stage"
  nfpm package --config "$config" --packager deb --target "$output/"
  nfpm package --config "$config" --packager rpm --target "$output/"
)
(
  cd "$output"
  sha256sum ./*.deb ./*.rpm > linux-packages.sha256
)
