#!/bin/sh
# Fetches the fonts the mockups use into build/fonts/, checking each
# archive's SHA-256, and extracts the files the renderer reads. Needs the
# network once; the mockup renderer and its tests never do. Licences are
# copied beside the fonts. Sources and hashes: docs/research/desktop/survey.md.
set -eu
out=${1:-build/fonts}
mkdir -p "$out"
fetch() {
    url=$1; file=$2; sum=$3
    if [ ! -f "$out/$file" ]; then
        curl -sSL -o "$out/$file" "$url"
    fi
    actual=$(shasum -a 256 "$out/$file" | cut -d' ' -f1)
    if [ "$actual" != "$sum" ]; then
        echo "fetch-fonts: $file: SHA-256 $actual, expected $sum" >&2
        exit 1
    fi
}
fetch https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip Inter-4.1.zip \
    9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e
fetch https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip JetBrainsMono-2.304.zip \
    6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf
fetch https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.tar.bz2 dejavu-fonts-ttf-2.37.tar.bz2 \
    fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
unzip -q -o "$out/Inter-4.1.zip" 'extras/ttf/Inter-Regular.ttf' 'extras/ttf/Inter-Medium.ttf' LICENSE.txt -d "$tmp/inter"
cp "$tmp/inter/extras/ttf/Inter-Regular.ttf" "$tmp/inter/extras/ttf/Inter-Medium.ttf" "$out/"
cp "$tmp/inter/LICENSE.txt" "$out/Inter-LICENSE.txt"
unzip -q -o "$out/JetBrainsMono-2.304.zip" 'fonts/ttf/JetBrainsMono-Regular.ttf' 'fonts/ttf/JetBrainsMono-Bold.ttf' OFL.txt -d "$tmp/jbm"
cp "$tmp/jbm/fonts/ttf/JetBrainsMono-Regular.ttf" "$tmp/jbm/fonts/ttf/JetBrainsMono-Bold.ttf" "$out/"
cp "$tmp/jbm/OFL.txt" "$out/JetBrainsMono-OFL.txt"
tar xjf "$out/dejavu-fonts-ttf-2.37.tar.bz2" -C "$tmp" \
    dejavu-fonts-ttf-2.37/ttf/DejaVuSans.ttf dejavu-fonts-ttf-2.37/ttf/DejaVuSans-Bold.ttf \
    dejavu-fonts-ttf-2.37/ttf/DejaVuSansMono.ttf dejavu-fonts-ttf-2.37/ttf/DejaVuSansMono-Bold.ttf \
    dejavu-fonts-ttf-2.37/LICENSE
cp "$tmp"/dejavu-fonts-ttf-2.37/ttf/*.ttf "$out/"
cp "$tmp/dejavu-fonts-ttf-2.37/LICENSE" "$out/DejaVu-LICENSE.txt"
echo "fonts in $out:"
ls "$out"/*.ttf
