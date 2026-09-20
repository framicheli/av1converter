#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 || ! -x $1 || ! $2 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 BINARY VERSION OUTPUT_DIRECTORY" >&2
  exit 2
fi

binary=$1
version=$2
output=$3
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$output"

install -Dm755 "$binary" "$work/deb/usr/bin/av1converter"
install -Dm644 LICENSE "$work/deb/usr/share/doc/av1converter/copyright"
install -Dm644 packaging/debian/control "$work/deb/DEBIAN/control"
sed -i "s/@VERSION@/$version/" "$work/deb/DEBIAN/control"
dpkg-deb --build --root-owner-group "$work/deb" "$output/av1converter_${version}_amd64.deb"

mkdir -p "$work/rpm/SOURCES" "$work/rpm/SPECS"
install -m755 "$binary" "$work/rpm/SOURCES/av1converter"
install -m644 LICENSE "$work/rpm/SOURCES/LICENSE"
install -m644 packaging/rpm/av1converter.spec "$work/rpm/SPECS/av1converter.spec"
rpmbuild -bb \
  --define "_topdir $work/rpm" \
  --define "av1converter_version $version" \
  "$work/rpm/SPECS/av1converter.spec"
rpm=$(find "$work/rpm/RPMS" -type f -name '*.rpm' -print -quit)
test -n "$rpm"
cp "$rpm" "$output/"
