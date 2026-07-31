#!/usr/bin/env bash
# 编译缓存清理脚本（基于 cargo-sweep）
#
# 解决的问题：Rust 的 target/ 目录会因多 feature 组合、debug 调试符号、
# 以及依赖变更不回收而无限增长。本脚本用 cargo-sweep 按"访问时间"或
# "体积上限"自动修剪陈旧产物，避免手动 cargo clean 后全量重编。
#
# 用法：
#   ./scripts/clean-cache.sh                 # 删除 30 天未使用的产物（实际删除）
#   ./scripts/clean-cache.sh --dry-run       # 仅预览，不删除
#   ./scripts/clean-cache.sh --max 10G       # 将 target 体积控制在 10G 以内
#   ./scripts/clean-cache.sh --days 14       # 自定义天数阈值
#   ./scripts/clean-cache.sh -h              # 帮助
#
# 依赖：cargo-sweep（cargo install cargo-sweep），需 ~/.cargo/bin 在 PATH 中。
set -euo pipefail

# 切到仓库根目录
cd "$(dirname "$0")/.." || exit 1

TAURI_DIR="src-tauri"
DRY_RUN=""
MAXSIZE=""
DAYS=30

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN="--dry-run" ;;
    --max)     MAXSIZE="$2"; shift ;;
    --days)    DAYS="$2"; shift ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "未知参数: $1" >&2; exit 1 ;;
  esac
  shift
done

# cargo-sweep 作用于当前 crate 的 target 目录
cd "$TAURI_DIR"

if [[ -n "$MAXSIZE" ]]; then
  echo ">> cargo sweep: 将 target 体积控制在 ${MAXSIZE} 以内"
  # shellcheck disable=SC2086
  cargo sweep $DRY_RUN --maxsize "$MAXSIZE"
else
  echo ">> cargo sweep: 删除 ${DAYS} 天未使用的产物"
  # shellcheck disable=SC2086
  cargo sweep $DRY_RUN --time "$DAYS"
fi

# 复核体积
echo ">> 当前 src-tauri 体积："
du -sh "$(pwd)/target" 2>/dev/null || echo "（无 target 目录）"
