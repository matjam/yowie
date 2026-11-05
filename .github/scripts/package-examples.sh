#!/usr/bin/env bash
# Packages the release build of every example, plus yowie-info, into
# yowie-examples-<version>-<platform>.zip in the current directory, and
# prints the zip's name.
#
# Build first:
#   cargo build --release -p yowie --examples
#   cargo build --release -p yowie-tools --bin yowie-info
#
# Usage: .github/scripts/package-examples.sh <version> <platform>
set -euo pipefail

version="$1"
platform="$2"
name="yowie-examples-${version}-${platform}"

ext=""
case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*) ext=".exe" ;;
esac

rm -rf "$name" "$name.zip"
mkdir -p "$name/licenses"

examples=$(cargo metadata --no-deps --format-version 1 |
	jq -r '.packages[] | select(.name == "yowie") | .targets[]
		| select(.kind | index("example")) | .name' | tr -d '\r' | sort)
count=0
for example in $examples; do
	cp "target/release/examples/${example}${ext}" "$name/"
	count=$((count + 1))
done
cp "target/release/yowie-info${ext}" "$name/"

cp LICENSE "$name/licenses/yowie-MIT.txt"
cp crates/yowie-gfx/fonts/LICENSE "$name/licenses/go-fonts.txt"
cp examples/assets/fonts/*-OFL.txt "$name/licenses/"

cat >"$name/README.txt" <<EOF
Yowie ${version} examples for ${platform}

These are the ${count} example programs from
https://github.com/matjam/yowie/tree/main/examples, built in release mode.
Each program is self-contained. To run one, open a terminal in this folder:

  ./tetris        on Linux and macOS
  .\\tetris.exe    on Windows

Escape quits. The examples table in the repository's README.md lists what
each program shows and the flags it takes. Every program also takes
--shot <file.png> (render one frame without a window and save it) and
--seconds <n> (exit after n seconds).

To list the GPU adapters Yowie can use, run yowie-info. Run it first when an
example does not start.

macOS: a zip downloaded with a web browser is quarantined, and macOS refuses
to run the unsigned programs in it. To clear the flag, run:

  xattr -dr com.apple.quarantine ${name}

Linux: the programs need a Vulkan driver (mesa-vulkan-drivers on Debian and
Ubuntu, or the NVIDIA driver), the ALSA library (libasound2) and libudev.
Under a Wayland session they open a native Wayland window; otherwise an X11
window.

Windows: the programs need a GPU driver with DX12 or Vulkan.

The licenses folder holds the licences of Yowie and of the fonts built into
the programs.
EOF

if command -v zip >/dev/null 2>&1; then
	zip -qr9 "$name.zip" "$name"
else
	7z a -tzip -mx=9 "$name.zip" "$name" >/dev/null
fi
echo "$name.zip"
