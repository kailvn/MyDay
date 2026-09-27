#!/usr/bin/env bash
# 签名 Android release APK（zipalign + apksigner）。
#
# 用法: scripts/sign-android-apk.sh <unsigned.apk> <out.apk>
#
# 必需环境变量:
#   MYDAY_KEYSTORE      密钥库路径（如 ~/.keystores/myday-release.jks）
#   MYDAY_KEYSTORE_PASS 密钥库密码
#   MYDAY_KEY_ALIAS     密钥别名（默认 myday）
#   ANDROID_HOME        Android SDK 路径（默认 ~/Android/Sdk），取 build-tools 最新版
#
# 密钥库不进仓库；CI 中由 GitHub Secrets（ANDROID_KEYSTORE_BASE64 / ANDROID_KEYSTORE_PASS）提供。
set -euo pipefail

unsigned=${1:?用法: sign-android-apk.sh <unsigned.apk> <out.apk>}
out=${2:?用法: sign-android-apk.sh <unsigned.apk> <out.apk>}

: "${MYDAY_KEYSTORE:?请设置 MYDAY_KEYSTORE}"
: "${MYDAY_KEYSTORE_PASS:?请设置 MYDAY_KEYSTORE_PASS}"
alias=${MYDAY_KEY_ALIAS:-myday}
sdk=${ANDROID_HOME:-$HOME/Android/Sdk}

# 取版本号最大的 build-tools（含 zipalign / apksigner）
bt=$(ls -d "$sdk"/build-tools/*/ | sort -V | tail -1)

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$bt/zipalign" -f -p 4 "$unsigned" "$tmp/aligned.apk"
"$bt/apksigner" sign \
  --ks "$MYDAY_KEYSTORE" \
  --ks-key-alias "$alias" \
  --ks-pass "pass:$MYDAY_KEYSTORE_PASS" \
  --out "$out" \
  "$tmp/aligned.apk"
"$bt/apksigner" verify "$out"
echo "已签名: $out"
