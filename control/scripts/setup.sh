#!/usr/bin/env sh
# owox-harness installer script (Linux / macOS).
# Downloads owox from GitHub Releases, verifies it with SHA256SUMS, then installs it
# See release distribution policy in control/docs/decisions/.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/setup.sh | sh
#
# Environment variables:
#   OWOX_VERSION  Version to download (for example, owox-v0.1.0 or 0.1.0). Defaults to the latest owox-v*
#   OWOX_BIN_DIR  Install directory. Defaults to $HOME/.local/bin
#   OWOX_REPO     Repository. Defaults to owoDra/workspace
set -eu

REPO="${OWOX_REPO:-owo-x-project/owox-harness}"
BIN_DIR="${OWOX_BIN_DIR:-$HOME/.local/bin}"

err() {
	echo "owox setup: $1" >&2
	exit 1
}

# Map the OS and CPU to a target triple.
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
Linux)
	case "$arch" in
	x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
	aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
	*) err "unsupported CPU: $arch (linux)" ;;
	esac
	;;
Darwin)
	case "$arch" in
	arm64 | aarch64) target="aarch64-apple-darwin" ;;
	*) err "unsupported CPU: $arch (macOS is distributed for Apple Silicon only)" ;;
	esac
	;;
*)
	err "unsupported OS: $os (use install.ps1 on Windows)"
	;;
esac
asset="owox-${target}.tar.gz"

# Select a checksum tool: sha256sum or shasum.
if command -v sha256sum >/dev/null 2>&1; then
	sha_check() { sha256sum -c -; }
elif command -v shasum >/dev/null 2>&1; then
	sha_check() { shasum -a 256 -c -; }
else
	err "neither sha256sum nor shasum is available; cannot verify checksum"
fi

# Resolve the version. If unset, use the latest owox-v* tag from Releases.
tag="${OWOX_VERSION:-}"
if [ -z "$tag" ]; then
	tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases" |
		grep -o '"tag_name"[ ]*:[ ]*"owox-v[^"]*"' |
		head -n1 |
		sed 's/.*"\(owox-v[^"]*\)".*/\1/')"
	[ -n "$tag" ] || err "could not find the latest owox-v* release. Set OWOX_VERSION"
elif [ "${tag#owox-v}" = "$tag" ]; then
	# Add the owox-v prefix if missing (0.1.0 -> owox-v0.1.0).
	tag="owox-v${tag#v}"
fi

base="https://github.com/${REPO}/releases/download/${tag}"
echo "owox setup: downloading ${asset} from ${tag}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}" ||
	err "could not download artifact: ${base}/${asset}"
curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS" ||
	err "could not download SHA256SUMS"

# Verify only this artifact line (SHA256SUMS contains all artifacts).
line="$(cd "$tmp" && grep "  ${asset}\$" SHA256SUMS)" ||
	err "SHA256SUMS has no line for ${asset}"
echo "$line" | (cd "$tmp" && sha_check) ||
	err "checksum mismatch. Aborting installation"

tar -xzf "${tmp}/${asset}" -C "$tmp" || err "could not extract artifact"
[ -f "${tmp}/owox" ] || err "artifact does not contain owox"

mkdir -p "$BIN_DIR"
mv "${tmp}/owox" "${BIN_DIR}/owox"
chmod +x "${BIN_DIR}/owox"

echo "owox setup: installed to ${BIN_DIR}/owox"
"${BIN_DIR}/owox" --version || true

case ":${PATH}:" in
*":${BIN_DIR}:"*) ;;
*)
	echo "owox setup: add ${BIN_DIR} to PATH (example: export PATH=\"${BIN_DIR}:\$PATH\")"
	;;
esac
