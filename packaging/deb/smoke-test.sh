#!/bin/bash
# 引数の .deb をまっさらな ubuntu:26.04 コンテナに入れて、起動と削除を確かめる
set -euo pipefail
DEB_DIR="$(cd "$(dirname "$1")" && pwd)"
DEB="$(basename "$1")"
docker run --rm -v "$DEB_DIR:/pkg:ro" ubuntu:26.04 bash -euxc '
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq "/pkg/$0" >/dev/null
  koe --version
  test -f /usr/share/applications/io.github.shuhei0866.koe.desktop
  test -f /usr/lib/systemd/user/koe.service
  test -f /usr/share/koe/config.toml
  test -f /usr/share/koe/dictionaries/default.toml
  # 初回起動で設定が作られることを確かめる（モデルがないので起動自体は失敗してよい）
  useradd -m tester
  su tester -c "cd ~ && timeout 10 koe" >/tmp/run.log 2>&1 || true
  grep -E "Created .*config.toml|Whisper model not found|No config" /tmp/run.log || tail -5 /tmp/run.log
  test -f /home/tester/.config/koe/config.toml
  grep -q "/usr/share/koe/dictionaries/default.toml" /home/tester/.config/koe/config.toml
  apt-get remove -y -qq koe >/dev/null
  test ! -e /usr/bin/koe
  echo SMOKE_OK
' "$DEB"
