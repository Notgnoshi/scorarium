#!/bin/bash
set -o errexit
set -o pipefail
set -o nounset
set -o noclobber

dest="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)/crates/scorarium-web/assets"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# GitHub redirects releases/latest to releases/tag/vX.Y.Z
latest() {
    local url
    url=$(curl -fsSLo /dev/null -w '%{url_effective}' "https://github.com/twbs/$1/releases/latest")
    echo "${url##*/v}"
}

version=$(latest bootstrap)
curl -fsSL -o "$tmp/bootstrap.zip" "https://github.com/twbs/bootstrap/releases/download/v$version/bootstrap-$version-dist.zip"

# -j flattens the zip's css/ and js/ directories into dest, -o overwrites the previous version
unzip -q -o -j "$tmp/bootstrap.zip" \
    "bootstrap-$version-dist/css/bootstrap.min.css" \
    "bootstrap-$version-dist/js/bootstrap.bundle.min.js" \
    -d "$dest"
echo "Vendored Bootstrap v$version"

version=$(latest icons)
curl -fsSL -o "$tmp/icons.zip" "https://github.com/twbs/icons/releases/download/v$version/bootstrap-icons-$version.zip"

# The server serves assets out of a flat directory
unzip -q -o -j "$tmp/icons.zip" \
    "bootstrap-icons-$version/bootstrap-icons.min.css" \
    "bootstrap-icons-$version/fonts/bootstrap-icons.woff2" \
    -d "$dest"
sed -i 's|url("fonts/|url("|g' "$dest/bootstrap-icons.min.css"
echo "Vendored Bootstrap Icons v$version"
