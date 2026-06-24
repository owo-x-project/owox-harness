#!/usr/bin/env sh
# owox-harness 導入スクリプト (linux / macOS)。
# GitHub Releases から owox を取得し、SHA256SUMS で checksum 照合してから配置する
# (control/docs/decisions/20260621-Phase10-配布とrelease正本.md)。
#
# 使い方:
#   curl -fsSL https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/setup.sh | sh
#
# 環境変数:
#   OWOX_VERSION  取得する版 (例 owox-v0.1.0 または 0.1.0)。既定は最新の owox-v*
#   OWOX_BIN_DIR  配置先ディレクトリ。既定 $HOME/.local/bin
#   OWOX_REPO     リポジトリ。既定 owoDra/workspace
set -eu

REPO="${OWOX_REPO:-owo-x-project/owox-harness}"
BIN_DIR="${OWOX_BIN_DIR:-$HOME/.local/bin}"

err() {
	echo "owox setup: $1" >&2
	exit 1
}

# OS と CPU を target triple へ写像する。
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
Linux)
	case "$arch" in
	x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
	aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
	*) err "未対応の CPU: $arch (linux)" ;;
	esac
	;;
Darwin)
	case "$arch" in
	arm64 | aarch64) target="aarch64-apple-darwin" ;;
	*) err "未対応の CPU: $arch (macOS は Apple Silicon のみ配布)" ;;
	esac
	;;
*)
	err "未対応の OS: $os (Windows は install.ps1 を使う)"
	;;
esac
asset="owox-${target}.tar.gz"

# checksum ツールを選ぶ。sha256sum か shasum のどちらか。
if command -v sha256sum >/dev/null 2>&1; then
	sha_check() { sha256sum -c -; }
elif command -v shasum >/dev/null 2>&1; then
	sha_check() { shasum -a 256 -c -; }
else
	err "sha256sum も shasum も無く checksum 照合できない"
fi

# 版を解決する。未指定なら最新の owox-v* tag を Releases から拾う。
tag="${OWOX_VERSION:-}"
if [ -z "$tag" ]; then
	tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases" |
		grep -o '"tag_name"[ ]*:[ ]*"owox-v[^"]*"' |
		head -n1 |
		sed 's/.*"\(owox-v[^"]*\)".*/\1/')"
	[ -n "$tag" ] || err "最新の owox-v* リリースを見つけられない。OWOX_VERSION で指定する"
elif [ "${tag#owox-v}" = "$tag" ]; then
	# owox-v 接頭辞が無ければ補う (0.1.0 → owox-v0.1.0)。
	tag="owox-v${tag#v}"
fi

base="https://github.com/${REPO}/releases/download/${tag}"
echo "owox setup: ${tag} の ${asset} を取得する"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}" ||
	err "成果物を取得できない: ${base}/${asset}"
curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS" ||
	err "SHA256SUMS を取得できない"

# 自分の成果物の行だけ照合する (SHA256SUMS は全成果物を含む)。
line="$(cd "$tmp" && grep "  ${asset}\$" SHA256SUMS)" ||
	err "SHA256SUMS に ${asset} の行が無い"
echo "$line" | (cd "$tmp" && sha_check) ||
	err "checksum が一致しない。配置を中止する"

tar -xzf "${tmp}/${asset}" -C "$tmp" || err "展開できない"
[ -f "${tmp}/owox" ] || err "成果物に owox が無い"

mkdir -p "$BIN_DIR"
mv "${tmp}/owox" "${BIN_DIR}/owox"
chmod +x "${BIN_DIR}/owox"

echo "owox setup: ${BIN_DIR}/owox へ配置した"
"${BIN_DIR}/owox" --version || true

case ":${PATH}:" in
*":${BIN_DIR}:"*) ;;
*)
	echo "owox setup: PATH に ${BIN_DIR} を加える (例: export PATH=\"${BIN_DIR}:\$PATH\")"
	;;
esac
