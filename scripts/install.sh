#!/bin/sh
set -eu

REPOSITORY="rustyllh/CPAOps"
VERSION="${CPACTL_VERSION:-}"
INSTALL_DIR="${CPACTL_INSTALL_DIR:-}"
RUN_INSTALL=1

usage() {
  printf '%s\n' '用法：install.sh [--version vX.Y.Z] [--install-dir DIR] [--no-install]'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    --no-install) RUN_INSTALL=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

SYSTEM="$(uname -s)"
MACHINE="$(uname -m)"
case "$SYSTEM:$MACHINE" in
  Darwin:arm64)
    PLATFORM="darwin-arm64"
    INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
    CHECKSUM_TOOL="shasum"
    ;;
  Linux:x86_64|Linux:amd64)
    PLATFORM="linux-amd64"
    INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
    CHECKSUM_TOOL="sha256sum"
    [ "$(id -u)" -eq 0 ] || {
      printf '%s\n' '错误：Linux 系统级安装需要 root 权限，请使用 curl ... | sudo bash。' >&2
      exit 4
    }
    ;;
  *)
    printf '%s\n' '错误：仅支持 macOS Apple Silicon 或使用 glibc/systemd 的 Linux AMD64。' >&2
    exit 2
    ;;
esac
command -v curl >/dev/null || { printf '%s\n' '错误：需要 curl。' >&2; exit 2; }
command -v "$CHECKSUM_TOOL" >/dev/null || { printf '错误：需要 %s。\n' "$CHECKSUM_TOOL" >&2; exit 2; }
command -v tar >/dev/null || { printf '%s\n' '错误：需要 tar。' >&2; exit 2; }

if [ -z "$VERSION" ]; then
  VERSION="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPOSITORY/releases/latest" | sed 's#^.*/##')"
  [ -n "$VERSION" ] || { printf '%s\n' '错误：无法确定 cpactl 最新版本。' >&2; exit 1; }
fi

ASSET="cpactl-$VERSION-$PLATFORM.tar.gz"
BASE_URL="https://github.com/$REPOSITORY/releases/download/$VERSION"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT HUP INT TERM

printf '下载 cpactl %s…\n' "$VERSION"
curl -fL --retry 3 --output "$TEMP_DIR/$ASSET" "$BASE_URL/$ASSET"
curl -fL --retry 3 --output "$TEMP_DIR/checksums.txt" "$BASE_URL/checksums.txt"
if [ "$CHECKSUM_TOOL" = "sha256sum" ]; then
  (cd "$TEMP_DIR" && grep "  $ASSET$" checksums.txt | sha256sum -c -)
else
  (cd "$TEMP_DIR" && grep "  $ASSET$" checksums.txt | shasum -a 256 -c -)
fi

tar -xzf "$TEMP_DIR/$ASSET" -C "$TEMP_DIR"
[ -f "$TEMP_DIR/cpactl" ] || { printf '%s\n' '错误：发布包中缺少 cpactl。' >&2; exit 1; }
mkdir -p "$INSTALL_DIR"
install -m 755 "$TEMP_DIR/cpactl" "$INSTALL_DIR/.cpactl.new"
mv -f "$INSTALL_DIR/.cpactl.new" "$INSTALL_DIR/cpactl"
printf '已安装：%s/cpactl\n' "$INSTALL_DIR"

if [ "$SYSTEM" = "Darwin" ]; then
case ":${PATH}:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    PROFILE="${ZDOTDIR:-$HOME}/.zprofile"
    [ "${SHELL##*/}" = "bash" ] && PROFILE="$HOME/.bash_profile"
    MARKER='# cpactl install'
    grep -F "$MARKER" "$PROFILE" >/dev/null 2>&1 || {
      printf '\n%s\nexport PATH="%s:$PATH"\n' "$MARKER" "$INSTALL_DIR" >> "$PROFILE"
    }
    export PATH="$INSTALL_DIR:$PATH"
    printf '已将 %s 写入 %s；新终端会自动生效。\n' "$INSTALL_DIR" "$PROFILE"
    ;;
esac
fi

if [ -n "${http_proxy:-}${https_proxy:-}${all_proxy:-}${HTTP_PROXY:-}${HTTPS_PROXY:-}${ALL_PROXY:-}" ]; then
  "$INSTALL_DIR/cpactl" proxy set || printf '%s\n' '提示：未能保存代理，将继续使用当前环境变量。' >&2
fi

if [ "$RUN_INSTALL" -eq 1 ] && [ -t 0 ]; then
  printf '现在运行 cpactl install 安装服务？[Y/n] '
  read -r ANSWER
  case "$ANSWER" in n|N|no|NO) ;; *) exec "$INSTALL_DIR/cpactl" install ;; esac
fi

printf '%s\n' '完成。请重新打开终端后直接运行：cpactl -V'
