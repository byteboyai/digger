#!/usr/bin/env bash
# 门禁:Digger 的**领域 / 持久化 / 业务运行时**必须与 bytehost 宿主层解耦
# (规格 §4 所有权表、§11 验收 A1)。只有 desktop / app-host 适配层可以依赖 bytehost。
#
# 依赖方向(规格 §4):
#
#     digger-desktop / digger-app-host  (组合根与宿主适配)
#                    |
#                    v
#     digger-domain / digger-store / digger-runtime  (纯业务)
#
#     bytehost-*  <-- 允许被 desktop/app-host 依赖;绝不被业务层依赖
#
# 当前状态(Task 11):
#   - 只有 `digger-desktop` 这一个生产 crate 存在,它**就是**宿主适配层,
#     因此允许依赖 bytehost-*。
#   - 领域/持久化/业务运行时 crate 尚未创建。门禁在它们出现后**自动纳入**,
#     并通过下面的 COVERAGE GAP 列表显式记录当前未覆盖的职责。
#
# 运行:scripts/check-host-deps.sh
# 自检:scripts/check-host-deps.sh --self-test(注入非法依赖必须失败)

set -euo pipefail
cd "$(dirname "$0")/.."

# 宿主适配层白名单:允许依赖 bytehost-* 的 crate。
HOST_ADAPTER_CRATES="digger-desktop digger-app-host diggerd digger-hook digger-mcp"

# 业务层 crate 前缀:出现即不得依赖任何 bytehost-*。
BUSINESS_PREFIX="^digger-(domain|store|persistence|runtime|writing|editor|project|agent)(-.*)?$"

# 业务层禁用:bytehost-*(宿主层)、iced*(界面框架)、wry(webview)。
# digger-editor 这类 crate 必须保持纯库,harness 与将来的 markdown-core 才能复用(ADR-013 §2)。
bytehost_banned='^(bytehost-.*|iced|iced_.*|wry)$'

tree_of() {
  local pkg="$1"
  local out
  out=$(cargo tree -p "$pkg" -e normal,build --target all --prefix none 2>&1) || {
    echo "cargo tree 失败:" >&2
    echo "$out" >&2
    exit 1
  }
  echo "$out" | awk '{print $1}' | sort -u
}

# 枚举 workspace 里所有 digger-* 生产包。
mapfile -t packages < <(
  cargo metadata --no-deps --format-version 1 \
    | python3 -c 'import json,sys
m=json.load(sys.stdin)
for p in sorted(m["packages"], key=lambda p: p["name"]):
    print(p["name"])'
)

checked=0
for pkg in "${packages[@]}"; do
  # 只审视非白名单的 digger-* 业务包。
  if [[ "$pkg" =~ $BUSINESS_PREFIX ]]; then
    hits=$(tree_of "$pkg" | grep -E "$bytehost_banned" || true)
    if [ -n "$hits" ]; then
      echo "业务 crate $pkg 依赖了宿主层 / 界面框架 / webview(bytehost-*、iced*、wry;应经适配层注入,不得直连):" >&2
      echo "$hits" >&2
      exit 1
    fi
    checked=$((checked + 1))
  fi
done

echo "digger host-deps check: ok(审视了 $checked 个业务 crate)"

# 显式记录覆盖缺口:业务层 crate 尚未建立时,门禁尚无对象可查。
existing_business=0
for pkg in "${packages[@]}"; do
  [[ "$pkg" =~ $BUSINESS_PREFIX ]] && existing_business=$((existing_business + 1))
done
if [ "$existing_business" -eq 0 ]; then
  cat >&2 <<'GAP'
COVERAGE GAP(Task 11):领域 / 持久化 / 业务运行时 crate 尚未创建,本门禁当前
  只确认宿主适配层(digger-desktop)存在且不反向被依赖。以下职责在对应 crate
  建立后必须纳入并执行本门禁:
    - 领域模型与写作文档表示(ADR-006 / ADR-012)
    - Project 一致性岛(project.db,ADR-003)
    - 业务运行时(编码任务、Agent 会话、MCP/Skill 接入)
  新 crate 命名落在 BUSINESS_PREFIX 内即自动纳入。
GAP
fi

if [ "${1:-}" = "--self-test" ]; then
  echo "运行门禁变异测试…" >&2
  # 变异探针必须落在 workspace 根之下(cargo 要求成员在根内)。
  probe="crates/_host_deps_probe"
  root_bak="$(mktemp)"
  cp Cargo.toml "$root_bak"
  cleanup() {
    rm -rf "$probe" "$root_bak"
    cp "$root_bak" Cargo.toml 2>/dev/null || true
  }
  trap cleanup EXIT
  mkdir -p "$probe/src"
  echo "" > "$probe/src/lib.rs"
  abs_core="$(cd ../bytehost/crates/bytehost-core && pwd)"

  # 每种禁用依赖各注入一次:必须被拦下;恢复后必须通过。
  inject() {
    local label="$1" dep="$2"
    cat > "$probe/Cargo.toml" <<TOMLEOF
[package]
name = "digger-domain-probe"
version = "0.0.0"
edition = "2024"
publish = false
[dependencies]
$dep
TOMLEOF
    # 门禁按 `crates/*` 通配即可发现探针,无需改根 members。
    if bash "$0" >/dev/null 2>&1; then
      echo "变异测试失败:业务包依赖 $label 后门禁仍然通过" >&2
      exit 1
    fi
    echo "  变异通过:业务包→$label 被拦下" >&2
  }
  inject "bytehost-*" "bytehost-core = { path = \"$abs_core\" }"
  inject "iced" 'iced = { workspace = true }'
  inject "wry" 'wry = { workspace = true }'
  rm -rf "$probe"

  if ! bash "$0" >/dev/null 2>&1; then
    echo "变异测试失败:恢复后门禁未通过" >&2
    exit 1
  fi
  echo "门禁变异测试:ok(注入即失败、恢复即通过)" >&2
fi
