# 共享库依赖守卫 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 byteboyai 的共享库消费者(Digger、Dozer)一个 pre-push 守卫:发现依赖落后、拦住本地路径 `[patch]` 与未推送的依赖进入 `main`。

**Architecture:** 一个只用 Python 标准库的单文件脚本(正本在 bytehost 的 `tools/shared-deps-guard/`),扫描各工作区的 `Cargo.toml` / `Cargo.lock` 并用 `git ls-remote` 查远端;`pre-push` 钩子用 `--rev` 检查"将被推送的提交"。每个消费者把脚本 vendor 到自己的 `scripts/`。

**Tech Stack:** Python 3.11+(`tomllib`、`unittest`)、bash、git。

**Spec:** `docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`(Task 1 会追加"实现阶段修订"一节)。

## Global Constraints

- 脚本只用 Python 标准库;需要 Python 3.11+(`tomllib`)。本机是 3.13.1。
- 版本号只在各 `Cargo.toml` 里写一份;`shared-deps.toml` 只记录刻意固定的理由(`reason`、`review_by`),不重复版本号。
- 只检查 `github.com/byteboyai/*` 的 git 依赖,不管 crates.io。
- commit 时不检查;只在 pre-push 触发。推 `main` 或 tag:`block` 级问题阻断;推其他分支:`block` 降级为警告。
- 联网失败(超时、鉴权)只产生 `ref-unverified` 警告并放行;只有远端**明确**没有该 ref 才产生 `ref-missing`。
- 推送(`git push`)、打 tag、往 bytehost/dozer 的共享分支合并,都是对外动作:**每一个都要先得到用户明确同意**。计划里用 `GATE` 标出。
- Dozer 主工作区(`../dozer`)有他人未提交改动,Digger 主工作区(`../digger`)正停在别的会话的 `chore/align-deps-2026-10` 分支且有未提交改动:**一律不碰**,只在新建的 worktree 里改。
- 输出文案与代码注释用中文,和仓库现有风格一致(参考 `scripts/check-host-deps.sh`)。

## Review Focus

1. **同一个工作区里的传递依赖拉进两份共享 crate**(例如 Digger 用 bytehost `v0.4.1`,而依赖的 `dozer-panel-*` 自己用 `v0.4.0`)——只看 `Cargo.toml` 发现不了,期望 `Cargo.lock` 里报 `lock-duplicate`。Task 3 的测试覆盖。
2. **排除在主工作区之外、自带 `[workspace]` 的 spike 用旧版本**(Dozer 的 `spike/v2-excalidraw` 用 bytehost `v0.2.0`)——期望不和主工作区互相冲突,不误报 `ref-mismatch`。Task 2、3 的测试覆盖。
3. **开发时工作区的 `Cargo.lock` 带本地路径(`.cargo/config.toml` 里的 `[patch]`),但已提交的内容是干净的**——期望 pre-push 放行(检查被推送的提交,不是工作区)。Task 5 的 `--rev` 测试覆盖。
4. **非语义化 tag**(`libs-2026-10-16`)与按 rev 固定的依赖——期望只检查存在性,不报"落后",也不因无法比较而崩溃。Task 4 的测试覆盖。
5. **远端查询失败或某个 rev 不在任何分支的末端**——期望 rev 走"按完整 sha 取提交"的兜底;短 rev 或网络失败时只警告,绝不误阻断。Task 4、5 的测试覆盖。

---

## 文件结构

**bytehost 仓库(正本),新增于 worktree `../bytehost-shared-deps-guard`:**

| 文件 | 职责 |
|---|---|
| `tools/shared-deps-guard/check_shared_deps.py` | 全部逻辑:解析、规则、联网实现、CLI |
| `tools/shared-deps-guard/test_check_shared_deps.py` | 单元测试,不联网 |
| `tools/shared-deps-guard/check-shared-deps.sh` | 薄包装,解析仓库根并调用脚本 |
| `tools/shared-deps-guard/pre-push` | 钩子:按推送目标决定阻断还是降级 |
| `tools/shared-deps-guard/dev-patch.example.toml` | 本地联调 `[patch]` 模板 |
| `tools/shared-deps-guard/test_hook.sh` | 钩子端到端测试(真实 `git push` 到本地裸仓库) |
| `tools/shared-deps-guard/README.md` | 正本说明与 vendor 步骤 |

**每个消费者(Digger、Dozer)vendor 的副本:**

| 文件 | 来源 |
|---|---|
| `scripts/check_shared_deps.py` | 正本,`SOURCE_TAG` 改成来源 tag |
| `scripts/check-shared-deps.sh` | 正本 |
| `scripts/dev-patch.example.toml` | 正本 |
| `.githooks/pre-push` | 正本 |
| `shared-deps.toml` | 各仓库自己写(固定理由) |

测试命令(在 bytehost worktree 里):`python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' -v`。注意用 `discover -s`,它会把该目录加进 `sys.path`;脚本本身用 `python3 -I` 运行时不依赖当前目录。

---

### Task 1: 修订 spec

实现前对真实仓库做了验证,发现 spec 有四处需要澄清。先把它们记进 spec,避免实现和文档不一致。

**Files:**
- Create worktree: `../digger-shared-deps-guard`(分支 `feat/shared-deps-guard`,已从 `main` 创建;若不存在见 Step 1)
- Modify: `docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`(文件末尾追加一节)

**Interfaces:**
- Produces: spec 第 12 节,后续任务引用它作为 `lock-duplicate`、按工作区分组、`--rev` 的依据。

- [ ] **Step 1: 确认在 digger worktree 里**

```bash
cd /Users/chrischiang/Projects/CoralProjects/byteboy
git -C digger worktree list | grep digger-shared-deps-guard \
  || git -C digger worktree add ../digger-shared-deps-guard -b feat/shared-deps-guard main
cd digger-shared-deps-guard && git status -sb | head -3
```
Expected: 分支是 `feat/shared-deps-guard`,工作区干净。

- [ ] **Step 2: 在 spec 末尾追加第 12 节**

在文件末尾追加(保持前面所有内容不变):

````markdown

## 12. 实现阶段修订(2026-10-16)

实现前拿真实仓库验证,发现需要澄清四点。它们细化第 3、4、5 节,不改变整体方案。

1. **按工作区分组(细化第 3、4 节)。** 每个 `Cargo.toml` 归属向上最近一个声明了 `[workspace]` 的目录(cargo 的规则);向上没有工作区的包自成一组。`ref-mismatch`、`lock-not-git`、`lock-duplicate` 都只在同一工作区内部判断,因为只有同一份 `Cargo.lock` 里的版本冲突才会拉进两份同名 crate。Dozer 的 `spike/v2-excalidraw` 自带 `[workspace]` 且用 bytehost `v0.2.0`,不应和主工作区冲突。
2. **新增规则 `lock-duplicate`(block,细化第 4 节)。** 同一个共享 crate 在 `Cargo.lock` 里有多个 git 来源。典型成因是传递依赖:Digger 用 bytehost `v0.4.1`,而它依赖的 `dozer-panel-*` 自己用 `v0.4.0`,于是解析出两份 `bytehost-core`,`PanelId` 之类类型互不兼容、byteui 的全局主题状态分裂。只看 `Cargo.toml` 发现不了。Dozer 的 `scripts/check-shared-pins.sh`(提交 `0ad8a334`)是它的早期、只覆盖 `crates/*/Cargo.toml` 的版本,新脚本落地后取代它。
3. **pre-push 检查被推送的提交,不是工作区(细化第 5 节)。** 开发时 `.cargo/config.toml` 里的本地 `[patch]` 会让工作区的 `Cargo.lock` 带本地路径,这是正常的开发状态。脚本新增 `--rev <提交>`:用 `git archive` 取出该提交里的 `Cargo.toml` / `Cargo.lock` / `shared-deps.toml` 再检查。钩子对每个被推送的引用分别检查(删除引用跳过)。
4. **补充规则与限制(细化第 4 节、第 11 节)。** 新增两条 `info` 级提示,不影响退出码:`ahead-of-remote`(本地 `main` 领先 `origin/main` N 个提交,用本地引用,不联网)与 `guard-source`(bytehost 有比 vendor 来源更新的语义化 tag,正本可能有更新)。非语义化 tag(如 `libs-2026-10-16`)只检查存在性,不比较落后。rev 固定的依赖先按"是否为某个远端引用的末端"判断,否则用 `git fetch --depth=1 --filter=tree:0 <完整 sha>` 兜底;短 rev 无法确认,只警告。
````

- [ ] **Step 3: 提交**

```bash
git add docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md
git commit -m "docs(spec): shared-deps guard, revisions found while validating on real repos

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: bytehost worktree、脚本骨架与解析

**Files:**
- Create worktree: `../bytehost-shared-deps-guard`(分支 `feat/shared-deps-guard`,从 `origin/main`)
- Create: `tools/shared-deps-guard/check_shared_deps.py`
- Create: `tools/shared-deps-guard/test_check_shared_deps.py`

**Interfaces:**
- Produces(后续任务依赖这些名字和签名):
  - `Issue(level, rule, message)`、`Dep(crate, repo, ref_kind, ref, manifest, graph=".")`、`Pin(reason, review_by)`(都是 frozen dataclass)
  - 常量 `BLOCK="block"`、`WARN="warn"`、`INFO="info"`,`SOURCE_TAG`,`CANONICAL_REPO`
  - `repo_name(url) -> str | None`、`find_manifests(root) -> list[Path]`
  - `collect_deps(root) -> list[Dep]`(带 `graph`)、`find_local_patches(root) -> list[(manifest, repo, crate)]`
  - `load_lock(root) -> list[dict]`、`load_pins(root) -> dict[str, Pin]`

- [ ] **Step 1: 创建 worktree**

```bash
cd /Users/chrischiang/Projects/CoralProjects/byteboy
git -C bytehost fetch -q origin
git -C bytehost worktree add ../bytehost-shared-deps-guard -b feat/shared-deps-guard origin/main
cd bytehost-shared-deps-guard && mkdir -p tools/shared-deps-guard && git status -sb | head -2
```
Expected: 分支 `feat/shared-deps-guard`,干净。

- [ ] **Step 2: 写失败的测试**

创建 `tools/shared-deps-guard/test_check_shared_deps.py`:

```python
import datetime
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_shared_deps as g  # noqa: E402

# 以下两份由真实 Cargo.toml 裁剪而来:Digger 把依赖集中在 [workspace.dependencies],
# Dozer 则在每个 crate 里各写各的。
DIGGER_ROOT_TOML = """\
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
bytehost-core = { git = "https://github.com/byteboyai/bytehost", tag = "v0.4.1" }
bytehost-desktop = { git = "https://github.com/byteboyai/bytehost", tag = "v0.4.1" }
byteui = { git = "https://github.com/byteboyai/byteui", tag = "v0.4.4" }
dozer-model = { git = "https://github.com/byteboyai/dozer", tag = "libs-2026-10-16" }
dozer-services = { git = "https://github.com/byteboyai/dozer", tag = "libs-2026-10-16" }
"""

DOZER_APP_TOML = """\
[package]
name = "dozer-app"
version = "0.1.0"

[dependencies]
bytehost-apps = { git = "https://github.com/byteboyai/bytehost", tag = "v0.4.0" }
bytehost-desktop = { git = "https://github.com/byteboyai/bytehost", tag = "v0.4.0", features = ["overlay"] }
byteui = { git = "https://github.com/byteboyai/byteui", tag = "v0.4.4" }
bytegit = { git = "https://github.com/byteboyai/bytegit", tag = "v0.7.0", features = ["watch"] }
tokio = { version = "1" }

[dev-dependencies]
bytegit = { git = "https://github.com/byteboyai/bytegit", tag = "v0.7.0", features = ["testutil"] }

[target.'cfg(target_os = "macos")'.dependencies]
bytehost-webview = { git = "https://github.com/byteboyai/bytehost", tag = "v0.4.0" }
"""


def write(root, rel, text):
    path = Path(root) / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


class RepoNameTests(unittest.TestCase):
    def test_accepts_plain_and_dot_git_and_lock_forms(self):
        for url in (
            "https://github.com/byteboyai/bytehost",
            "https://github.com/byteboyai/bytehost.git",
            "https://github.com/byteboyai/bytehost/",
            "git+https://github.com/byteboyai/bytehost?tag=v0.4.0#abc123",
        ):
            self.assertEqual(g.repo_name(url), "bytehost", url)

    def test_rejects_other_orgs_and_registries(self):
        self.assertIsNone(g.repo_name("https://github.com/someone/bytehost"))
        self.assertIsNone(g.repo_name("registry+https://github.com/rust-lang/crates.io-index"))


class CollectDepsTests(unittest.TestCase):
    def test_workspace_dependencies_shape(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "Cargo.toml", DIGGER_ROOT_TOML)
            deps = g.collect_deps(Path(tmp))
        by_crate = {d.crate: d for d in deps}
        self.assertEqual(set(by_crate), {"bytehost-core", "bytehost-desktop", "byteui",
                                         "dozer-model", "dozer-services"})
        self.assertEqual((by_crate["byteui"].repo, by_crate["byteui"].ref_kind, by_crate["byteui"].ref),
                         ("byteui", "tag", "v0.4.4"))
        self.assertEqual(by_crate["dozer-model"].ref, "libs-2026-10-16")

    def test_per_crate_shape_with_dev_and_target_tables(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "crates/dozer-app/Cargo.toml", DOZER_APP_TOML)
            deps = g.collect_deps(Path(tmp))
        self.assertEqual(sorted({d.crate for d in deps}),
                         ["bytegit", "bytehost-apps", "bytehost-desktop", "bytehost-webview", "byteui"])
        self.assertTrue(all(d.manifest == "crates/dozer-app/Cargo.toml" for d in deps))
        self.assertEqual(len([d for d in deps if d.crate == "bytegit"]), 2)  # deps + dev-deps

    def test_package_rename_and_missing_ref_and_foreign_git_and_skipped_dirs(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "Cargo.toml", """\
[dependencies]
core = { git = "https://github.com/byteboyai/bytehost", package = "bytehost-core" }
other = { git = "https://github.com/someone/else", tag = "v1.0.0" }
pinned = { git = "https://github.com/byteboyai/bytegit", rev = "abc1234def" }
""")
            write(tmp, "target/debug/Cargo.toml", """\
[dependencies]
ignored = { git = "https://github.com/byteboyai/bytehost", tag = "v9.9.9" }
""")
            deps = g.collect_deps(Path(tmp))
        self.assertEqual([(d.crate, d.ref_kind, d.ref) for d in deps],
                         [("bytehost-core", "none", ""), ("pinned", "rev", "abc1234def")])


class WorkspaceGraphTests(unittest.TestCase):
    def test_excluded_workspace_is_its_own_graph(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "Cargo.toml", '[workspace]\nmembers = ["crates/*"]\nexclude = ["spike/x"]\n')
            write(tmp, "crates/a/Cargo.toml", """\
[dependencies]
bytehost-core = { git = "https://github.com/byteboyai/bytehost", tag = "v0.4.1" }
""")
            write(tmp, "spike/x/Cargo.toml", """\
[workspace]
[dependencies]
bytehost-apps = { git = "https://github.com/byteboyai/bytehost", tag = "v0.2.0" }
""")
            write(tmp, "spike/y/Cargo.toml", """\
[dependencies]
byteui = { git = "https://github.com/byteboyai/byteui", tag = "v0.4.0" }
""")
            deps = {d.crate: d.graph for d in g.collect_deps(Path(tmp))}
        # spike/y 没有自己的 [workspace]:按 cargo 的规则归属向上最近的工作区(根)
        self.assertEqual(deps, {"bytehost-core": ".", "bytehost-apps": "spike/x", "byteui": "."})

    def test_package_without_any_workspace_ancestor_is_standalone(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "tools/a/Cargo.toml", """\
[dependencies]
byteui = { git = "https://github.com/byteboyai/byteui", tag = "v0.4.0" }
""")
            deps = {d.crate: d.graph for d in g.collect_deps(Path(tmp))}
        self.assertEqual(deps, {"byteui": "tools/a"})


class PatchLockPinLoadTests(unittest.TestCase):
    def test_local_patches_only_for_byteboyai_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "Cargo.toml", """\
[patch."https://github.com/byteboyai/bytehost"]
bytehost-core = { path = "../bytehost-chrome/crates/bytehost-core" }

[patch.crates-io]
foo = { path = "../foo" }

[patch."https://github.com/byteboyai/byteui"]
byteui = { git = "https://github.com/byteboyai/byteui", branch = "x" }
""")
            patches = g.find_local_patches(Path(tmp))
        self.assertEqual(patches, [("Cargo.toml", "bytehost", "bytehost-core")])

    def test_missing_lock_and_manifest_are_empty(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(g.load_lock(Path(tmp)), [])
            self.assertEqual(g.load_pins(Path(tmp)), {})

    def test_pins_parse_dates_and_strings(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "shared-deps.toml", """\
[pin.dozer]
reason = "等一次性同步"
review_by = 2026-11-15

[pin.bytegit]
reason = "x"
review_by = "2026-12-01"
""")
            pins = g.load_pins(Path(tmp))
        self.assertEqual(pins["dozer"], g.Pin("等一次性同步", datetime.date(2026, 11, 15)))
        self.assertEqual(pins["bytegit"].review_by, datetime.date(2026, 12, 1))



if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 3: 运行,确认失败**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -5`
Expected: FAIL,`ModuleNotFoundError: No module named 'check_shared_deps'`。

- [ ] **Step 4: 写实现**

创建 `tools/shared-deps-guard/check_shared_deps.py`:

```python
#!/usr/bin/env python3
"""shared-deps-guard:byteboyai 共享库依赖守卫(仅标准库)。

正本在 github.com/byteboyai/bytehost 的 tools/shared-deps-guard/,
消费者(Digger、Dozer)把本文件 vendor 到自己的 scripts/。
规格:digger 仓库 docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime
import io
import os
import re
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Protocol

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    sys.exit("check_shared_deps.py 需要 Python 3.11+(tomllib)")

# 正本所在仓库与 vendor 来源 tag。vendor 时把 SOURCE_TAG 改成来源 tag。
CANONICAL_REPO = "bytehost"
SOURCE_TAG = "unreleased"

SKIP_DIRS = {"target", ".git", "node_modules", ".dozer", ".digger", ".cargo"}
DEP_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")

BLOCK, WARN, INFO = "block", "warn", "info"


@dataclasses.dataclass(frozen=True)
class Issue:
    level: str
    rule: str
    message: str


@dataclasses.dataclass(frozen=True)
class Dep:
    crate: str  # 实际的 package 名(尊重 `package = "..."`)
    repo: str  # byteboyai 下的仓库名,如 "bytehost"
    ref_kind: str  # "tag" | "rev" | "branch" | "none"
    ref: str
    manifest: str  # 声明它的 Cargo.toml,相对 root
    graph: str = "."  # 所属工作区根目录(相对 root);同一工作区共用一份 Cargo.lock 和依赖图


@dataclasses.dataclass(frozen=True)
class Pin:
    reason: str
    review_by: datetime.date | None


_REPO_RE = re.compile(
    r"^(?:git\+)?https?://github\.com/byteboyai/([^/?#]+?)(?:\.git)?/?(?:[?#].*)?$"
)


def repo_name(url: str) -> str | None:
    """`https://github.com/byteboyai/bytehost(.git)` 之类 -> "bytehost";其他 -> None。"""
    m = _REPO_RE.match(url.strip())
    return m.group(1) if m else None


def find_manifests(root: Path) -> list[Path]:
    found: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        if "Cargo.toml" in filenames:
            found.append(Path(dirpath) / "Cargo.toml")
    return found


def _dep_tables(doc: dict):
    for name in DEP_SECTIONS:
        if isinstance(doc.get(name), dict):
            yield doc[name]
    ws = doc.get("workspace", {})
    if isinstance(ws.get("dependencies"), dict):
        yield ws["dependencies"]
    for cfg in doc.get("target", {}).values():
        for name in DEP_SECTIONS:
            if isinstance(cfg.get(name), dict):
                yield cfg[name]


def _load_all(root: Path) -> list[tuple[Path, dict]]:
    return [(m, tomllib.loads(m.read_text())) for m in find_manifests(root)]


def _graph_of(manifest: Path, root: Path, workspace_dirs: set[Path]) -> str:
    """最近的、声明了 [workspace] 的祖先目录;没有就是自己所在目录(独立包)。"""
    d = manifest.parent
    while True:
        if d in workspace_dirs:
            return str(d.relative_to(root))
        if d == root or d.parent == d:
            return str(manifest.parent.relative_to(root))
        d = d.parent


def collect_deps(root: Path) -> list[Dep]:
    docs = _load_all(root)
    workspace_dirs = {m.parent for m, doc in docs if "workspace" in doc}
    deps: list[Dep] = []
    for manifest, doc in docs:
        rel = str(manifest.relative_to(root))
        graph = _graph_of(manifest, root, workspace_dirs)
        for table in _dep_tables(doc):
            for key, spec in table.items():
                if not isinstance(spec, dict) or "git" not in spec:
                    continue
                repo = repo_name(spec["git"])
                if repo is None:
                    continue
                kind, ref = "none", ""
                for k in ("tag", "rev", "branch"):
                    if k in spec:
                        kind, ref = k, str(spec[k])
                        break
                deps.append(Dep(spec.get("package", key), repo, kind, ref, rel, graph))
    return deps


def find_local_patches(root: Path) -> list[tuple[str, str, str]]:
    """入库 Cargo.toml 里指向本地路径的 byteboyai `[patch]`:(manifest, repo, crate)。"""
    out: list[tuple[str, str, str]] = []
    for manifest, doc in _load_all(root):
        rel = str(manifest.relative_to(root))
        for source, crates in doc.get("patch", {}).items():
            repo = repo_name(source)
            if repo is None or not isinstance(crates, dict):
                continue
            for crate, spec in crates.items():
                if isinstance(spec, dict) and "path" in spec:
                    out.append((rel, repo, crate))
    return out


def load_lock(root: Path) -> list[dict]:
    lock = root / "Cargo.lock"
    if not lock.exists():
        return []
    return tomllib.loads(lock.read_text()).get("package", [])


def load_pins(root: Path) -> dict[str, Pin]:
    path = root / "shared-deps.toml"
    if not path.exists():
        return {}
    pins: dict[str, Pin] = {}
    for name, body in tomllib.loads(path.read_text()).get("pin", {}).items():
        review_by = body.get("review_by")
        if isinstance(review_by, str):
            review_by = datetime.date.fromisoformat(review_by)
        pins[name] = Pin(body.get("reason", ""), review_by)
    return pins
```

- [ ] **Step 5: 运行,确认通过**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' -v 2>&1 | tail -6`
Expected: `Ran 10 tests ... OK`。

- [ ] **Step 6: 提交**

```bash
git add tools/shared-deps-guard
git commit -m "feat(tools): shared-deps-guard scaffolding, manifest scanning per workspace

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 离线规则

规则:`local-patch`、`lock-not-git`、`lock-duplicate`、`ref-mismatch`(按工作区)、`pin-expired`、`manifest-stale`。

**Files:**
- Modify: `tools/shared-deps-guard/check_shared_deps.py`(末尾追加)
- Modify: `tools/shared-deps-guard/test_check_shared_deps.py`

**Interfaces:**
- Consumes: Task 2 的 `Issue`、`Dep`、`Pin`、`repo_name`、常量。
- Produces:
  - `check_local_patch(patches) -> list[Issue]`
  - `check_lock(deps, packages, lock_path="Cargo.lock") -> list[Issue]`
  - `check_lock_duplicates(packages, lock_path="Cargo.lock") -> list[Issue]`
  - `check_mismatch(deps) -> list[Issue]`
  - `check_pins(deps, pins, today) -> list[Issue]`

- [ ] **Step 1: 写失败的测试**

在测试文件里,把下面内容插到最后的 `if __name__ == "__main__":` **上方**:

```python


def dep(crate, repo="bytehost", kind="tag", ref="v0.4.0", manifest="Cargo.toml", graph="."):
    return g.Dep(crate, repo, kind, ref, manifest, graph)


class OfflineRuleTests(unittest.TestCase):
    def test_local_patch_blocks(self):
        issues = g.check_local_patch([("Cargo.toml", "bytehost", "bytehost-core")])
        self.assertEqual([(i.level, i.rule) for i in issues], [("block", "local-patch")])
        self.assertIn("bytehost-core", issues[0].message)
        self.assertEqual(g.check_local_patch([]), [])

    def test_local_patch_is_one_issue_per_manifest_and_repo(self):
        patches = [("Cargo.toml", "bytehost", "bytehost-core"), ("Cargo.toml", "bytehost", "bytehost-apps"),
                   ("Cargo.toml", "byteui", "byteui")]
        issues = g.check_local_patch(patches)
        self.assertEqual(len(issues), 2)
        self.assertIn("bytehost-apps, bytehost-core", issues[0].message)

    def test_lock_not_git_is_one_issue_listing_every_crate(self):
        deps = [dep("bytehost-core"), dep("bytehost-apps")]
        packages = [{"name": "bytehost-core", "version": "0.1.0"}, {"name": "bytehost-apps", "version": "0.1.0"}]
        issues = g.check_lock(deps, packages)
        self.assertEqual(len(issues), 1)
        self.assertIn("bytehost-apps, bytehost-core", issues[0].message)

    def test_lock_not_git_flags_shared_crate_without_git_source(self):
        deps = [dep("bytehost-core")]
        packages = [
            {"name": "bytehost-core", "version": "0.1.0"},  # 被本地 patch,没有 source
            {"name": "serde", "version": "1.0.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"name": "digger-desktop", "version": "0.1.0"},  # 本仓库成员,不是共享库
        ]
        issues = g.check_lock(deps, packages)
        self.assertEqual([(i.level, i.rule) for i in issues], [("block", "lock-not-git")])
        self.assertIn("bytehost-core", issues[0].message)

    def test_lock_with_git_source_passes(self):
        packages = [{"name": "bytehost-core", "version": "0.1.0",
                     "source": "git+https://github.com/byteboyai/bytehost?tag=v0.4.0#dc88133d"}]
        self.assertEqual(g.check_lock([dep("bytehost-core")], packages), [])

    def test_lock_duplicate_blocks_two_git_sources_for_one_crate(self):
        packages = [
            {"name": "bytehost-core", "version": "0.1.0",
             "source": "git+https://github.com/byteboyai/bytehost?tag=v0.4.0#dc88133d2e959355bd7d1bd4c6138177074c168e"},
            {"name": "bytehost-core", "version": "0.1.0",
             "source": "git+https://github.com/byteboyai/bytehost?tag=v0.4.1#79212c08834ca946148fd7977d3d56d72b34c008"},
            {"name": "byteui", "version": "0.4.4",
             "source": "git+https://github.com/byteboyai/byteui?tag=v0.4.4#aaaaaaaaaaaaaaaa"},
        ]
        issues = g.check_lock_duplicates(packages)
        self.assertEqual([(i.level, i.rule) for i in issues], [("block", "lock-duplicate")])
        self.assertIn("bytehost-core", issues[0].message)
        self.assertIn("bytehost?tag=v0.4.0#dc88133", issues[0].message)
        self.assertIn("bytehost?tag=v0.4.1#79212c0", issues[0].message)

    def test_lock_duplicate_ignores_registry_crates_and_single_sources(self):
        packages = [
            {"name": "syn", "version": "1.0.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"name": "syn", "version": "2.0.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"name": "byteui", "version": "0.4.4", "source": "git+https://github.com/byteboyai/byteui?tag=v0.4.4#abc"},
        ]
        self.assertEqual(g.check_lock_duplicates(packages), [])

    def test_mismatch_blocks_when_one_repo_has_two_refs(self):
        deps = [dep("dozer-model", "dozer", "rev", "aaa"), dep("dozer-services", "dozer", "rev", "bbb"),
                dep("bytehost-core")]
        issues = g.check_mismatch(deps)
        self.assertEqual([(i.level, i.rule) for i in issues], [("block", "ref-mismatch")])
        self.assertIn("dozer", issues[0].message)
        self.assertNotIn("bytehost", issues[0].message)

    def test_mismatch_passes_when_consistent_even_across_manifests(self):
        deps = [dep("bytehost-core", manifest="a/Cargo.toml"), dep("bytehost-apps", manifest="b/Cargo.toml")]
        self.assertEqual(g.check_mismatch(deps), [])

    def test_pins_expired_and_stale(self):
        deps = [dep("dozer-model", "dozer", "rev", "aaa")]
        pins = {"dozer": g.Pin("等同步", datetime.date(2026, 11, 15)),
                "gone": g.Pin("旧的", None)}
        today = datetime.date(2026, 11, 16)
        issues = g.check_pins(deps, pins, today)
        self.assertEqual(sorted((i.level, i.rule) for i in issues),
                         [("warn", "manifest-stale"), ("warn", "pin-expired")])

    def test_pin_not_expired_on_the_review_day(self):
        deps = [dep("dozer-model", "dozer", "rev", "aaa")]
        pins = {"dozer": g.Pin("等同步", datetime.date(2026, 11, 15))}
        self.assertEqual(g.check_pins(deps, pins, datetime.date(2026, 11, 15)), [])

    def test_mismatch_is_per_workspace(self):
        # 排除在主工作区之外的 spike 自带 [workspace]、自带 Cargo.lock,旧版本不会和主图冲突
        deps = [dep("bytehost-apps", ref="v0.4.1"),
                dep("bytehost-apps", ref="v0.2.0", manifest="spike/x/Cargo.toml", graph="spike/x")]
        self.assertEqual(g.check_mismatch(deps), [])
        deps.append(dep("bytehost-core", ref="v0.4.0", manifest="spike/x/Cargo.toml", graph="spike/x"))
        issues = g.check_mismatch(deps)
        self.assertEqual([i.rule for i in issues], ["ref-mismatch"])
        self.assertIn("spike/x", issues[0].message)
```

- [ ] **Step 2: 运行,确认失败**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -4`
Expected: FAIL,`AttributeError: module 'check_shared_deps' has no attribute 'check_local_patch'`。

- [ ] **Step 3: 写实现**

追加到 `check_shared_deps.py` 末尾:

```python


# ---------------------------------------------------------------- 离线规则


def check_local_patch(patches: list[tuple[str, str, str]]) -> list[Issue]:
    grouped: dict[tuple[str, str], list[str]] = {}
    for manifest, repo, crate in patches:
        grouped.setdefault((manifest, repo), []).append(crate)
    return [
        Issue(BLOCK, "local-patch",
              f"{manifest} 的 [patch] 把 {repo} 的 {', '.join(sorted(crates))} 指向本地路径;"
              "联调请改用不入库的 .cargo/config.toml(见 scripts/dev-patch.example.toml)")
        for (manifest, repo), crates in sorted(grouped.items())
    ]


def check_lock(deps: list[Dep], packages: list[dict], lock_path: str = "Cargo.lock") -> list[Issue]:
    shared = {d.crate for d in deps}
    local = sorted({pkg["name"] for pkg in packages
                    if pkg.get("name") in shared and not str(pkg.get("source", "")).startswith("git+")})
    if not local:
        return []
    return [Issue(BLOCK, "lock-not-git",
                  f"{lock_path} 里这些共享库不是来自远端 git(多半被本地 [patch] 覆盖):{', '.join(local)};"
                  f"移除覆盖后执行 cargo update -p {local[0]}(其余同理)还原")]


def check_lock_duplicates(packages: list[dict], lock_path: str = "Cargo.lock") -> list[Issue]:
    """同一个共享 crate 在锁文件里有多个 git 来源:依赖图里有两份,同名类型互不兼容。

    典型成因是传递依赖:本仓库用 bytehost v0.4.1,而依赖的 dozer-panel-* 自己用 v0.4.0。
    只看 Cargo.toml 发现不了,只有 Cargo.lock 里看得到。
    """
    sources: dict[str, set[str]] = {}
    for pkg in packages:
        source = str(pkg.get("source", ""))
        if repo_name(source) is not None:
            sources.setdefault(pkg["name"], set()).add(source)
    issues = []
    for name, found in sorted(sources.items()):
        if len(found) > 1:
            detail = ", ".join(sorted(_short_source(s) for s in found))
            issues.append(Issue(BLOCK, "lock-duplicate",
                                f"{name} 在 {lock_path} 里有 {len(found)} 份:{detail};"
                                "让传递依赖它的库和本仓库对齐到同一个 tag"))
    return issues


def _short_source(source: str) -> str:
    """git+https://github.com/byteboyai/bytehost?tag=v0.4.1#79212c08... -> bytehost?tag=v0.4.1#79212c0"""
    body = source.split("github.com/byteboyai/", 1)[-1]
    head, _, sha = body.partition("#")
    return f"{head}#{sha[:7]}" if sha else head


def check_mismatch(deps: list[Dep]) -> list[Issue]:
    # 只有同一个工作区(同一份 Cargo.lock)里的版本冲突才会拉进两份同名 crate
    by_repo: dict[tuple[str, str], dict[tuple[str, str], set[str]]] = {}
    for d in deps:
        by_repo.setdefault((d.graph, d.repo), {}).setdefault((d.ref_kind, d.ref), set()).add(d.crate)
    issues = []
    for (graph, repo), refs in sorted(by_repo.items()):
        if len(refs) > 1:
            where = "" if graph == "." else f"(工作区 {graph})"
            detail = "; ".join(
                f"{kind}={ref or '(无)'} <- {', '.join(sorted(crates))}"
                for (kind, ref), crates in sorted(refs.items()))
            issues.append(Issue(BLOCK, "ref-mismatch",
                                f"{repo}{where} 在同一依赖图里出现多个版本,会拉进两份同名 crate:{detail}"))
    return issues


def check_pins(deps: list[Dep], pins: dict[str, Pin], today: datetime.date) -> list[Issue]:
    repos = {d.repo for d in deps}
    issues = []
    for name, pin in sorted(pins.items()):
        if name not in repos:
            issues.append(Issue(WARN, "manifest-stale",
                                f"shared-deps.toml 里的 pin.{name} 在任何 Cargo.toml 中都已不存在"))
        elif pin.review_by is not None and today > pin.review_by:
            issues.append(Issue(WARN, "pin-expired",
                                f"{name} 的 pin 已过 review_by({pin.review_by}):{pin.reason}"))
    return issues
```

- [ ] **Step 4: 运行,确认通过**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -4`
Expected: `Ran 22 tests ... OK`。

- [ ] **Step 5: 提交**

```bash
git add tools/shared-deps-guard
git commit -m "feat(tools): shared-deps-guard offline rules (local-patch, lock-not-git, lock-duplicate, ref-mismatch, pins)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 远端规则

规则:`ref-missing`、`ref-unverified`、`behind`、`guard-source`。联网部分隔离在 `Remote` 协议后面,测试用 `FakeRemote`。

**Files:**
- Modify: `tools/shared-deps-guard/check_shared_deps.py`(末尾追加)
- Modify: `tools/shared-deps-guard/test_check_shared_deps.py`

**Interfaces:**
- Consumes: Task 2、3 的类型与常量。
- Produces:
  - 协议 `Remote`:`refs(repo) -> dict[str, str] | None`(键如 `refs/tags/v0.4.0`、`refs/heads/main`,值是 sha;不可达时 `None`)、`has_commit(repo, rev) -> bool | None`
  - `parse_semver(tag) -> tuple[int,int,int] | None`、`latest_semver_tag(refs) -> str | None`、`describe_gap(cur, latest) -> str`
  - `check_remote(deps, pins, remote) -> list[Issue]`、`check_guard_source(remote, source_tag=SOURCE_TAG) -> list[Issue]`

- [ ] **Step 1: 写失败的测试**

插到测试文件最后的 `if __name__` **上方**:

```python


class FakeRemote:
    """refs_by_repo: {"bytehost": {"refs/tags/v0.4.0": "sha", ...}};缺失的仓库视为无法联网。"""

    def __init__(self, refs_by_repo, commits=None):
        self.refs_by_repo = refs_by_repo
        self.commits = commits or {}

    def refs(self, repo):
        return self.refs_by_repo.get(repo)

    def has_commit(self, repo, rev):
        return self.commits.get((repo, rev))


def tags(*names, **extra):
    refs = {f"refs/tags/{n}": f"sha-{n}" for n in names}
    refs.update(extra)
    return refs


class SemverTests(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(g.parse_semver("v0.4.1"), (0, 4, 1))
        self.assertEqual(g.parse_semver("1.2.3"), (1, 2, 3))
        self.assertIsNone(g.parse_semver("libs-2026-10-16"))
        self.assertIsNone(g.parse_semver("v1.2.3-rc1"))

    def test_latest_ignores_peeled_and_non_semver_and_compares_numerically(self):
        refs = tags("v0.9.0", "v0.10.0", "libs-2026-10-16", **{"refs/tags/v0.10.0^{}": "x",
                                                              "refs/heads/v9.9.9": "y"})
        self.assertEqual(g.latest_semver_tag(refs), "v0.10.0")
        self.assertIsNone(g.latest_semver_tag(tags("libs-2026-10-16")))

    def test_gap_wording(self):
        self.assertEqual(g.describe_gap((0, 2, 0), (0, 4, 0)), "2 个 minor")
        self.assertEqual(g.describe_gap((0, 4, 0), (1, 0, 0)), "1 个 major")
        self.assertEqual(g.describe_gap((0, 4, 0), (0, 4, 3)), "3 个 patch")


class RemoteRuleTests(unittest.TestCase):
    def levels(self, issues):
        return sorted((i.level, i.rule) for i in issues)

    def test_missing_tag_blocks(self):
        remote = FakeRemote({"bytehost": tags("v0.4.0")})
        issues = g.check_remote([dep("bytehost-core", ref="v0.9.9")], {}, remote)
        self.assertIn(("block", "ref-missing"), self.levels(issues))

    def test_missing_branch_blocks_and_existing_branch_passes(self):
        remote = FakeRemote({"bytehost": {"refs/heads/main": "s"}})
        self.assertEqual(g.check_remote([dep("a", kind="branch", ref="main")], {}, remote), [])
        issues = g.check_remote([dep("a", kind="branch", ref="gone")], {}, remote)
        self.assertEqual(self.levels(issues), [("block", "ref-missing")])

    def test_rev_matching_a_remote_tip_passes_without_fetch(self):
        remote = FakeRemote({"dozer": {"refs/heads/main": "7cdbffd0178c50891d4c39f8260224e6b76b79e5"}})
        d = dep("dozer-model", "dozer", "rev", "7cdbffd0178c50891d4c39f8260224e6b76b79e5")
        self.assertEqual(g.check_remote([d], {"dozer": g.Pin("x", None)}, remote), [])

    def test_rev_not_at_a_tip_falls_back_to_has_commit(self):
        sha = "67ab0e657d75135b882ace2f3aa7a56617776c60"
        d = dep("dozer-model", "dozer", "rev", sha)
        refs = {"dozer": {"refs/heads/main": "ffffffffffffffffffffffffffffffffffffffff"}}
        pins = {"dozer": g.Pin("x", None)}
        found = g.check_remote([d], pins, FakeRemote(refs, {("dozer", sha): True}))
        missing = g.check_remote([d], pins, FakeRemote(refs, {("dozer", sha): False}))
        unknown = g.check_remote([d], pins, FakeRemote(refs, {}))
        self.assertEqual(found, [])
        self.assertEqual(self.levels(missing), [("block", "ref-missing")])
        self.assertEqual(self.levels(unknown), [("warn", "ref-unverified")])

    def test_unreachable_remote_warns_and_never_blocks(self):
        issues = g.check_remote([dep("bytehost-core")], {}, FakeRemote({}))
        self.assertEqual(self.levels(issues), [("warn", "ref-unverified")])

    def test_behind_warns_with_gap(self):
        remote = FakeRemote({"bytehost": tags("v0.2.0", "v0.4.0")})
        issues = g.check_remote([dep("bytehost-core", ref="v0.2.0")], {}, remote)
        self.assertEqual(self.levels(issues), [("warn", "behind")])
        self.assertIn("v0.2.0 -> 最新 v0.4.0", issues[0].message)
        self.assertIn("2 个 minor", issues[0].message)

    def test_up_to_date_is_silent(self):
        remote = FakeRemote({"bytehost": tags("v0.2.0", "v0.4.0")})
        self.assertEqual(g.check_remote([dep("bytehost-core", ref="v0.4.0")], {}, remote), [])

    def test_pinned_repo_is_never_behind(self):
        remote = FakeRemote({"bytehost": tags("v0.2.0", "v0.4.0")})
        pins = {"bytehost": g.Pin("等同步", None)}
        self.assertEqual(g.check_remote([dep("bytehost-core", ref="v0.2.0")], pins, remote), [])

    def test_non_semver_tag_is_checked_for_existence_only(self):
        remote = FakeRemote({"dozer": tags("libs-2026-10-16", "v0.5.153")})
        d = dep("dozer-model", "dozer", "tag", "libs-2026-10-16")
        self.assertEqual(g.check_remote([d], {}, remote), [])

    def test_guard_source_info_only_when_canonical_has_newer_semver_tag(self):
        remote = FakeRemote({"bytehost": tags("v0.4.1", "v0.5.0")})
        issues = g.check_guard_source(remote, "v0.4.1")
        self.assertEqual([(i.level, i.rule) for i in issues], [("info", "guard-source")])
        self.assertEqual(g.check_guard_source(remote, "v0.5.0"), [])
        self.assertEqual(g.check_guard_source(remote, "unreleased"), [])
        self.assertEqual(g.check_guard_source(FakeRemote({}), "v0.4.1"), [])
```

- [ ] **Step 2: 运行,确认失败**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -4`
Expected: FAIL,`AttributeError: ... no attribute 'parse_semver'`。

- [ ] **Step 3: 写实现**

追加到 `check_shared_deps.py` 末尾:

```python


# ---------------------------------------------------------------- 远端规则


class Remote(Protocol):
    def refs(self, repo: str) -> dict[str, str] | None:
        """{"refs/tags/v1": sha, "refs/heads/main": sha, ...};无法联网时 None。"""

    def has_commit(self, repo: str, rev: str) -> bool | None:
        """远端是否有该提交;无法确定时 None。"""


_SEMVER_RE = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)$")


def parse_semver(tag: str) -> tuple[int, int, int] | None:
    m = _SEMVER_RE.match(tag)
    return (int(m[1]), int(m[2]), int(m[3])) if m else None


def latest_semver_tag(refs: dict[str, str]) -> str | None:
    best: tuple[tuple[int, int, int], str] | None = None
    for ref in refs:
        if not ref.startswith("refs/tags/") or ref.endswith("^{}"):
            continue
        tag = ref[len("refs/tags/"):]
        ver = parse_semver(tag)
        if ver is not None and (best is None or ver > best[0]):
            best = (ver, tag)
    return best[1] if best else None


def describe_gap(cur: tuple[int, int, int], latest: tuple[int, int, int]) -> str:
    if latest[0] != cur[0]:
        return f"{latest[0] - cur[0]} 个 major"
    if latest[1] != cur[1]:
        return f"{latest[1] - cur[1]} 个 minor"
    return f"{latest[2] - cur[2]} 个 patch"


def check_remote(deps: list[Dep], pins: dict[str, Pin], remote: Remote) -> list[Issue]:
    issues: list[Issue] = []
    by_repo: dict[str, set[tuple[str, str]]] = {}
    for d in deps:
        by_repo.setdefault(d.repo, set()).add((d.ref_kind, d.ref))

    for repo, wanted in sorted(by_repo.items()):
        refs = remote.refs(repo)
        if refs is None:
            issues.append(Issue(WARN, "ref-unverified",
                                f"无法联网查询 {repo} 的远端引用,本次未验证其 tag/rev 是否存在"))
            continue
        shas = set(refs.values())
        for kind, ref in sorted(wanted):
            if kind == "tag" and f"refs/tags/{ref}" not in refs:
                issues.append(Issue(BLOCK, "ref-missing", f"{repo} 的 tag {ref} 在远端不存在,先推送再依赖"))
            elif kind == "branch" and f"refs/heads/{ref}" not in refs:
                issues.append(Issue(BLOCK, "ref-missing", f"{repo} 的分支 {ref} 在远端不存在,先推送再依赖"))
            elif kind == "rev" and not (len(ref) >= 7 and any(s.startswith(ref) for s in shas)):
                found = remote.has_commit(repo, ref)
                if found is False:
                    issues.append(Issue(BLOCK, "ref-missing",
                                        f"{repo} 的 rev {ref[:12]} 在远端不存在,先推送再依赖"))
                elif found is None:
                    issues.append(Issue(WARN, "ref-unverified",
                                        f"无法确认 {repo} 的 rev {ref[:12]} 是否已在远端"))

        # 落后:只比较语义化 tag;rev 与 `libs-2026-10-16` 这类非语义化 tag 只查存在性
        if repo in pins:
            continue
        current = [(parse_semver(ref), ref) for kind, ref in wanted
                   if kind == "tag" and parse_semver(ref) is not None]
        latest_tag = latest_semver_tag(refs)
        if current and latest_tag:
            cur_ver, cur_tag = max(current)
            latest_ver = parse_semver(latest_tag)
            if latest_ver > cur_ver:
                issues.append(Issue(WARN, "behind",
                                    f"{repo} {cur_tag} -> 最新 {latest_tag}"
                                    f"(落后 {describe_gap(cur_ver, latest_ver)})"))
    return issues


def check_guard_source(remote: Remote, source_tag: str = SOURCE_TAG) -> list[Issue]:
    """提示 vendor 的本脚本可能不是正本最新版(只能提示,不会自动更新)。"""
    current = parse_semver(source_tag)
    refs = remote.refs(CANONICAL_REPO)
    if current is None or refs is None:
        return []
    latest = latest_semver_tag(refs)
    if latest and parse_semver(latest) > current:
        return [Issue(INFO, "guard-source",
                      f"本脚本 vendor 自 {CANONICAL_REPO} {source_tag},现在有 {latest};"
                      "如果 tools/shared-deps-guard 有更新,请重新 vendor")]
    return []
```

- [ ] **Step 4: 运行,确认通过**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -4`
Expected: `Ran 35 tests ... OK`。

- [ ] **Step 5: 提交**

```bash
git add tools/shared-deps-guard
git commit -m "feat(tools): shared-deps-guard remote rules (ref-missing, behind, guard-source)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 联网实现、`--rev` 与命令行入口

**Files:**
- Modify: `tools/shared-deps-guard/check_shared_deps.py`(末尾追加,含 `if __name__` 入口)
- Modify: `tools/shared-deps-guard/test_check_shared_deps.py`

**Interfaces:**
- Consumes: Task 2-4 的全部。
- Produces:
  - `GitRemote(timeout=30)`:实现 `Remote`;`refs` 用 `git ls-remote --tags --heads` 并缓存;`has_commit` 只接受 40 位 sha,`git fetch --depth=1 --filter=tree:0`
  - `parse_ls_remote(output)`、`classify_fetch_failure(stderr) -> bool | None`
  - `ahead_of_remote(root, branch="main") -> Issue | None`
  - `export_rev(repo, rev, dest) -> bool`
  - `downgrade_blocks(issues)`、`render(issues) -> str`、`run(root, remote, today, git_root=None)`、`main(argv=None, remote=None, today=None) -> int`
  - 命令行:`--root PATH`、`--rev REV`、`--offline`、`--downgrade-blocks`;退出码 0(无阻断)/ 1(有阻断)/ 2(读不到 `--rev`)

- [ ] **Step 1: 写失败的测试**

插到测试文件最后的 `if __name__` **上方**:

```python


class GitRemoteHelperTests(unittest.TestCase):
    def test_parse_ls_remote_keeps_peeled_entries(self):
        out = ("aaa\trefs/heads/main\n"
               "bbb\trefs/tags/v0.4.0\n"
               "ccc\trefs/tags/v0.4.0^{}\n")
        self.assertEqual(g.parse_ls_remote(out),
                         {"refs/heads/main": "aaa", "refs/tags/v0.4.0": "bbb", "refs/tags/v0.4.0^{}": "ccc"})
        self.assertEqual(g.parse_ls_remote(""), {})

    def test_fetch_failure_classification(self):
        self.assertIs(g.classify_fetch_failure("fatal: remote error: upload-pack: not our ref abc"), False)
        self.assertIs(g.classify_fetch_failure("fatal: couldn't find remote ref abc"), False)
        self.assertIsNone(g.classify_fetch_failure("fatal: unable to access: LibreSSL SSL_connect: SSL_ERROR_SYSCALL"))
        self.assertIsNone(g.classify_fetch_failure("fatal: could not read Username"))

    def test_short_rev_is_unverifiable_without_network(self):
        self.assertIsNone(g.GitRemote().has_commit("dozer", "67ab0e6"))


class RenderAndDowngradeTests(unittest.TestCase):
    ISSUES = [g.Issue(g.INFO, "i", "info msg"), g.Issue(g.WARN, "w", "warn msg"),
              g.Issue(g.BLOCK, "b", "block msg")]

    def test_render_sorts_blocks_first_and_counts(self):
        text = g.render(self.ISSUES)
        lines = text.splitlines()
        self.assertTrue(lines[0].startswith("BLOCK"))
        self.assertTrue(lines[1].startswith("WARN"))
        self.assertTrue(lines[2].startswith("INFO"))
        self.assertEqual(lines[-1], "shared-deps: 1 个阻断, 1 个警告")
        self.assertEqual(g.render([]), "shared-deps: 没有发现问题")

    def test_downgrade_turns_only_blocks_into_warnings(self):
        out = g.downgrade_blocks(self.ISSUES)
        self.assertEqual([i.level for i in out], ["info", "warn", "warn"])
        self.assertIn("降级", out[2].message)


class MainTests(unittest.TestCase):
    TODAY = datetime.date(2026, 10, 16)

    def project(self, tmp, cargo=DIGGER_ROOT_TOML, lock=None, pins=None):
        write(tmp, "Cargo.toml", cargo)
        if lock is not None:
            write(tmp, "Cargo.lock", lock)
        if pins is not None:
            write(tmp, "shared-deps.toml", pins)

    def remote(self):
        return FakeRemote({
            "bytehost": tags("v0.4.0", "v0.4.1"),
            "byteui": tags("v0.4.4"),
            "dozer": tags("libs-2026-10-16"),
        })

    def run_main(self, tmp, *flags, remote=None):
        import contextlib
        import io
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = g.main(["--root", tmp, *flags], remote=remote or self.remote(), today=self.TODAY)
        return code, buf.getvalue()

    def test_clean_project_exits_zero(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.project(tmp)
            code, out = self.run_main(tmp)
        self.assertEqual(code, 0, out)
        self.assertIn("没有发现问题", out)

    def test_local_patch_blocks_and_downgrade_lets_it_through(self):
        patched = DIGGER_ROOT_TOML + """
[patch."https://github.com/byteboyai/bytehost"]
bytehost-core = { path = "../bytehost-chrome/crates/bytehost-core" }
"""
        with tempfile.TemporaryDirectory() as tmp:
            self.project(tmp, cargo=patched)
            strict_code, strict_out = self.run_main(tmp)
            lenient_code, lenient_out = self.run_main(tmp, "--downgrade-blocks")
        self.assertEqual(strict_code, 1)
        self.assertIn("BLOCK  local-patch", strict_out)
        self.assertEqual(lenient_code, 0)
        self.assertIn("WARN   local-patch", lenient_out)

    def test_lock_pointing_at_local_path_blocks(self):
        lock = '[[package]]\nname = "bytehost-core"\nversion = "0.1.0"\n'
        with tempfile.TemporaryDirectory() as tmp:
            self.project(tmp, lock=lock)
            code, out = self.run_main(tmp)
        self.assertEqual(code, 1)
        self.assertIn("lock-not-git", out)

    def test_offline_skips_remote_rules(self):
        cargo = DIGGER_ROOT_TOML.replace("v0.4.1", "v9.9.9")
        with tempfile.TemporaryDirectory() as tmp:
            self.project(tmp, cargo=cargo)
            online, _ = self.run_main(tmp)
            offline, out = self.run_main(tmp, "--offline")
        self.assertEqual(online, 1)  # tag 不存在
        self.assertEqual(offline, 0, out)

    def test_unreachable_remote_never_blocks(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.project(tmp)
            code, out = self.run_main(tmp, remote=FakeRemote({}))
        self.assertEqual(code, 0, out)
        self.assertIn("ref-unverified", out)

    def test_expired_pin_warns_but_exits_zero(self):
        pins = '[pin.dozer]\nreason = "等同步"\nreview_by = 2026-10-01\n'
        with tempfile.TemporaryDirectory() as tmp:
            self.project(tmp, pins=pins)
            code, out = self.run_main(tmp)
        self.assertEqual(code, 0, out)
        self.assertIn("pin-expired", out)


class RevModeTests(unittest.TestCase):
    def git(self, tmp, *args):
        import subprocess
        subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                       check=True, capture_output=True)

    def test_rev_checks_committed_content_not_the_working_tree(self):
        import contextlib
        import io
        good_lock = ('[[package]]\nname = "bytehost-core"\nversion = "0.1.0"\n'
                     'source = "git+https://github.com/byteboyai/bytehost?tag=v0.4.1#abc"\n')
        dev_lock = '[[package]]\nname = "bytehost-core"\nversion = "0.1.0"\n'  # 本地 patch 下的样子
        remote = FakeRemote({"bytehost": tags("v0.4.1"), "byteui": tags("v0.4.4"), "dozer": tags("libs-2026-10-16")})
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "Cargo.toml", DIGGER_ROOT_TOML)
            write(tmp, "Cargo.lock", good_lock)
            self.git(tmp, "init", "-q", "-b", "main")
            self.git(tmp, "add", "-A")
            self.git(tmp, "commit", "-q", "-m", "init")
            write(tmp, "Cargo.lock", dev_lock)  # 未提交的本地联调状态

            def run(*flags):
                buf = io.StringIO()
                with contextlib.redirect_stdout(buf):
                    code = g.main(["--root", tmp, *flags], remote=remote, today=datetime.date(2026, 10, 16))
                return code, buf.getvalue()

            worktree_code, worktree_out = run()
            rev_code, rev_out = run("--rev", "HEAD")
            bad_code, _ = run("--rev", "no-such-rev")
        self.assertEqual(worktree_code, 1, worktree_out)
        self.assertIn("lock-not-git", worktree_out)
        self.assertEqual(rev_code, 0, rev_out)
        self.assertEqual(bad_code, 2)


class AheadHintTests(unittest.TestCase):
    def test_reports_commits_not_yet_on_origin_main_even_in_rev_mode(self):
        import contextlib
        import io
        import subprocess
        with tempfile.TemporaryDirectory() as tmp:
            remote_dir, work = f"{tmp}/remote.git", f"{tmp}/work"
            subprocess.run(["git", "init", "-q", "--bare", remote_dir], check=True)
            subprocess.run(["git", "init", "-q", "-b", "main", work], check=True)

            def git(*args):
                subprocess.run(["git", "-C", work, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                               check=True, capture_output=True)

            write(work, "Cargo.toml", "[workspace]\nmembers = []\n")
            git("add", "-A")
            git("commit", "-q", "-m", "one")
            git("remote", "add", "origin", remote_dir)
            git("push", "-q", "origin", "main")
            git("commit", "-q", "--allow-empty", "-m", "two")
            git("commit", "-q", "--allow-empty", "-m", "three")
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                code = g.main(["--root", work, "--rev", "HEAD", "--offline"], today=datetime.date(2026, 10, 16))
        self.assertEqual(code, 0)
        self.assertIn("INFO   ahead-of-remote", buf.getvalue())
        self.assertIn("领先 origin/main 2 个提交", buf.getvalue())
```

- [ ] **Step 2: 运行,确认失败**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -4`
Expected: FAIL,`AttributeError: ... no attribute 'parse_ls_remote'`。

- [ ] **Step 3: 写实现**

追加到 `check_shared_deps.py` 末尾(这一段包含文件末尾的 `if __name__ == "__main__"` 入口):

```python


# ---------------------------------------------------------------- 联网实现


def parse_ls_remote(output: str) -> dict[str, str]:
    refs: dict[str, str] = {}
    for line in output.splitlines():
        sha, _, ref = line.partition("\t")
        if ref:
            refs[ref] = sha
    return refs


_MISSING_MARKERS = ("not our ref", "couldn't find remote ref", "no such remote ref")


def classify_fetch_failure(stderr: str) -> bool | None:
    """git fetch 失败的原因:明确"远端没有该提交" -> False;其他(网络、鉴权) -> None。"""
    low = stderr.lower()
    return False if any(m in low for m in _MISSING_MARKERS) else None


class GitRemote:
    """用 `git ls-remote` / `git fetch` 查询 github.com/byteboyai/<repo>。"""

    def __init__(self, timeout: int = 30):
        self.timeout = timeout
        self._cache: dict[str, dict[str, str] | None] = {}
        self._env = {**os.environ, "GIT_TERMINAL_PROMPT": "0"}

    @staticmethod
    def url(repo: str) -> str:
        return f"https://github.com/byteboyai/{repo}.git"

    def _git(self, args: list[str], cwd: str | None = None) -> subprocess.CompletedProcess | None:
        try:
            return subprocess.run(["git", *args], cwd=cwd, env=self._env, capture_output=True,
                                  text=True, timeout=self.timeout)
        except (OSError, subprocess.TimeoutExpired):
            return None

    def refs(self, repo: str) -> dict[str, str] | None:
        if repo not in self._cache:
            proc = self._git(["ls-remote", "--tags", "--heads", self.url(repo)])
            ok = proc is not None and proc.returncode == 0
            self._cache[repo] = parse_ls_remote(proc.stdout) if ok else None
        return self._cache[repo]

    def has_commit(self, repo: str, rev: str) -> bool | None:
        if len(rev) != 40:  # 远端只按完整 sha 取提交;短 rev 无法确认
            return None
        with tempfile.TemporaryDirectory() as tmp:
            if self._git(["init", "--bare", "-q", tmp]) is None:
                return None
            # tree:0 只取提交对象本身:Dozer 这样的大仓库从 15 秒降到 1.5 秒
            proc = self._git(["fetch", "--depth=1", "--filter=tree:0", "-q", self.url(repo), rev], cwd=tmp)
        if proc is None:
            return None
        if proc.returncode == 0:
            return True
        return classify_fetch_failure(proc.stderr)


def ahead_of_remote(root: Path, branch: str = "main") -> Issue | None:
    """本地 branch 领先 origin/branch 的提交数(只用本地引用,不联网)。"""
    try:
        proc = subprocess.run(
            ["git", "-C", str(root), "rev-list", "--count", f"origin/{branch}..{branch}"],
            capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.returncode != 0 or not proc.stdout.strip().isdigit() or int(proc.stdout) == 0:
        return None
    return Issue(INFO, "ahead-of-remote",
                 f"本地 {branch} 领先 origin/{branch} {int(proc.stdout)} 个提交;"
                 "被其他仓库依赖的提交要先推送,否则对方会引用到远端不存在的 rev")


# ---------------------------------------------------------------- 入口

_ORDER = {BLOCK: 0, WARN: 1, INFO: 2}


def downgrade_blocks(issues: list[Issue]) -> list[Issue]:
    return [Issue(WARN, i.rule, i.message + "(推送目标不是 main/tag,阻断降级为警告)")
            if i.level == BLOCK else i for i in issues]


def render(issues: list[Issue]) -> str:
    lines = [f"{i.level.upper():5}  {i.rule:16}  {i.message}"
             for i in sorted(issues, key=lambda i: _ORDER[i.level])]
    blocks = sum(i.level == BLOCK for i in issues)
    warns = sum(i.level == WARN for i in issues)
    lines.append(f"shared-deps: {blocks} 个阻断, {warns} 个警告" if issues else "shared-deps: 没有发现问题")
    return "\n".join(lines)


WANTED_FILES = {"Cargo.toml", "Cargo.lock", "shared-deps.toml"}


def export_rev(repo: Path, rev: str, dest: Path) -> bool:
    """把某个提交里的 Cargo.toml / Cargo.lock / shared-deps.toml 取到 dest,用来检查"将被推送的内容"。"""
    ls = subprocess.run(["git", "-C", str(repo), "ls-tree", "-r", "--name-only", "-z", rev],
                        capture_output=True, text=True)
    if ls.returncode != 0:
        return False
    paths = [p for p in ls.stdout.split("\0") if p and p.rsplit("/", 1)[-1] in WANTED_FILES]
    if not paths:
        return True
    arc = subprocess.run(["git", "-C", str(repo), "archive", rev, "--", *paths], capture_output=True)
    if arc.returncode != 0:
        return False
    with tarfile.open(fileobj=io.BytesIO(arc.stdout)) as tf:
        try:
            tf.extractall(dest, filter="data")
        except TypeError:  # Python < 3.11.4 没有 filter 参数
            tf.extractall(dest)
    return True


def run(root: Path, remote: Remote | None, today: datetime.date,
        git_root: Path | None = None) -> list[Issue]:
    deps = collect_deps(root)
    pins = load_pins(root)
    issues = [
        *check_local_patch(find_local_patches(root)),
        *check_mismatch(deps),
        *check_pins(deps, pins, today),
    ]
    for graph in sorted({d.graph for d in deps}):
        lock_path = "Cargo.lock" if graph == "." else f"{graph}/Cargo.lock"
        packages = load_lock(root / graph)
        issues += check_lock([d for d in deps if d.graph == graph], packages, lock_path)
        issues += check_lock_duplicates(packages, lock_path)
    if remote is not None:
        issues += check_remote(deps, pins, remote)
        issues += check_guard_source(remote)
    ahead = ahead_of_remote(git_root or root)
    if ahead is not None:
        issues.append(ahead)
    return issues


def main(argv: list[str] | None = None, remote: Remote | None = None,
         today: datetime.date | None = None) -> int:
    parser = argparse.ArgumentParser(description="byteboyai 共享库依赖守卫")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--rev", help="检查该提交里的内容,而不是工作区(pre-push 用它检查将被推送的提交)")
    parser.add_argument("--offline", action="store_true", help="跳过所有联网检查")
    parser.add_argument("--downgrade-blocks", action="store_true",
                        help="把阻断降级为警告(推送非 main/tag 时由 pre-push 使用)")
    args = parser.parse_args(argv)

    if remote is None and not args.offline:
        remote = GitRemote()
    root = args.root.resolve()
    remote = None if args.offline else remote
    today = today or datetime.date.today()
    if args.rev:
        with tempfile.TemporaryDirectory() as tmp:
            if not export_rev(root, args.rev, Path(tmp)):
                print(f"shared-deps: 无法读取提交 {args.rev}", file=sys.stderr)
                return 2
            issues = run(Path(tmp), remote, today, git_root=root)
    else:
        issues = run(root, remote, today)
    if args.downgrade_blocks:
        issues = downgrade_blocks(issues)
    print(render(issues))
    return 1 if any(i.level == BLOCK for i in issues) else 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: 运行,确认通过**

Run: `python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -4`
Expected: `Ran 48 tests ... OK`。

- [ ] **Step 5: 用真实仓库验证联网路径(单元测试没覆盖的部分)**

```bash
cd /Users/chrischiang/Projects/CoralProjects/byteboy/bytehost-shared-deps-guard
python3 -I tools/shared-deps-guard/check_shared_deps.py --root ../dozer --rev origin/main
python3 - <<'EOF'
import sys; sys.path.insert(0, "tools/shared-deps-guard")
import check_shared_deps as g
r = g.GitRemote()
print("tags:", sorted(k for k in r.refs("bytehost") if "^" not in k and "tags" in k)[-3:])
print("bogus commit  ->", r.has_commit("dozer", "0123456789abcdef0123456789abcdef01234567"))   # 期望 False
print("short rev     ->", r.has_commit("dozer", "67ab0e6"))                                    # 期望 None
print("missing repo  ->", r.has_commit("no-such-repo-xyz", "0123456789abcdef0123456789abcdef01234567"))  # 期望 None
EOF
```
Expected:
- 第一条命令在 Dozer 的 `origin/main` 上不应有 `BLOCK`(可能有 `behind` 警告,取决于 bytehost 最新 tag)。
- `tags` 打印出 bytehost 最新的几个 tag(含 `v0.4.1`)。
- 三个 `has_commit` 结果依次是 `False`、`None`、`None`。
- 如果第一条出现 `ref-unverified`,说明本次网络不稳;重试一次,仍失败再停下来报告。

- [ ] **Step 6: 提交**

```bash
git add tools/shared-deps-guard
git commit -m "feat(tools): shared-deps-guard git remote, --rev mode and CLI

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 包装脚本、钩子、联调模板与说明

**Files:**
- Create: `tools/shared-deps-guard/check-shared-deps.sh`
- Create: `tools/shared-deps-guard/pre-push`
- Create: `tools/shared-deps-guard/dev-patch.example.toml`
- Create: `tools/shared-deps-guard/test_hook.sh`
- Create: `tools/shared-deps-guard/README.md`

**Interfaces:**
- Consumes: Task 5 的命令行(`--root`、`--rev`、`--downgrade-blocks`、`--offline`)。
- Produces: 消费者 vendor 的四个文件(`.py`、`check-shared-deps.sh`、`pre-push`、`dev-patch.example.toml`)。约定:消费者里钩子调用 `<仓库根>/scripts/check-shared-deps.sh`。

- [ ] **Step 1: 写包装脚本**

创建 `tools/shared-deps-guard/check-shared-deps.sh`:

```bash
#!/usr/bin/env bash
# 共享库依赖守卫入口。规格:digger 仓库 docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md
#
# 用法:scripts/check-shared-deps.sh [--offline] [--rev <提交>] [--downgrade-blocks]
#   无参数        检查工作区,输出完整报告(含落后提示),有阻断时退出码为 1
#   --rev <提交>  检查该提交里的内容(pre-push 用它检查将被推送的提交)
#   --offline     跳过联网检查
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(git -C "$here" rev-parse --show-toplevel)"
exec python3 -I "$here/check_shared_deps.py" --root "$root" "$@"
```

- [ ] **Step 2: 写 pre-push 钩子**

创建 `tools/shared-deps-guard/pre-push`:

```bash
#!/usr/bin/env bash
# 共享库依赖守卫的 pre-push 钩子。启用(每个克隆一次):git config core.hooksPath .githooks
#
# git 通过 stdin 传入每个被推送的引用:<local ref> <local sha> <remote ref> <remote sha>
# 推 main 或 tag:阻断类问题拦下推送;推其他分支:同样的检查,阻断降级为警告。
# 确认要绕过时:git push --no-verify
set -u
root="$(git rev-parse --show-toplevel)"
zero="0000000000000000000000000000000000000000"
status=0
while read -r _local_ref local_sha remote_ref _remote_sha; do
  [ "$local_sha" = "$zero" ] && continue # 删除远端引用,没有内容可检查
  args=(--rev "$local_sha")
  case "$remote_ref" in
    refs/heads/main | refs/tags/*) ;;
    *) args+=(--downgrade-blocks) ;;
  esac
  echo "shared-deps: 检查 $remote_ref" >&2
  "$root/scripts/check-shared-deps.sh" "${args[@]}" || status=1
done
if [ "$status" -ne 0 ]; then
  echo "pre-push: 共享库依赖检查未通过,已拦下推送。修复后重试;确认要绕过时用 git push --no-verify" >&2
fi
exit "$status"
```

- [ ] **Step 3: 写联调模板**

创建 `tools/shared-deps-guard/dev-patch.example.toml`:

```toml
# 本地联调共享库时的 [patch] 模板。
#
# 用法:复制到 .cargo/config.toml(已被 .gitignore 忽略,不会入库),
#       取消注释并把路径改成你的本地 worktree。联调结束删掉即可。
#
# 不要把这些 [patch] 写进入库的 Cargo.toml:pre-push 会在推 main / tag 时拦下它。
# 注意:cargo 会把被覆盖的共享库在 Cargo.lock 里记成本地路径。
#       不要提交这样的 Cargo.lock(pre-push 同样会检查被推送提交里的 Cargo.lock)。

# [patch."https://github.com/byteboyai/bytehost"]
# bytehost-core = { path = "../bytehost-worktree/crates/bytehost-core" }
# bytehost-desktop = { path = "../bytehost-worktree/crates/bytehost-desktop" }
# bytehost-apps = { path = "../bytehost-worktree/crates/bytehost-apps" }

# [patch."https://github.com/byteboyai/byteui"]
# byteui = { path = "../byteui-worktree" }

# [patch."https://github.com/byteboyai/dozer"]
# # 同一个仓库的 crate 要一起指,否则依赖图里会有两份 dozer-model
# dozer-model = { path = "../dozer-worktree/crates/dozer-model" }
# dozer-services = { path = "../dozer-worktree/crates/dozer-services" }
# dozer-store-project = { path = "../dozer-worktree/crates/dozer-store-project" }
```

- [ ] **Step 4: 写钩子端到端测试**

创建 `tools/shared-deps-guard/test_hook.sh`:

```bash
#!/usr/bin/env bash
# pre-push 钩子的端到端测试:真实 git push 到本地裸仓库。不联网(用例里没有 byteboyai 的 git 依赖)。
# 运行:tools/shared-deps-guard/test_hook.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

git init -q --bare "$tmp/remote.git"
git init -q -b main "$tmp/work"
cd "$tmp/work"
git config user.name t && git config user.email t@t
mkdir scripts .githooks
cp "$here/check_shared_deps.py" "$here/check-shared-deps.sh" scripts/
cp "$here/pre-push" .githooks/pre-push
chmod +x scripts/check-shared-deps.sh .githooks/pre-push
git config core.hooksPath .githooks
git remote add origin "$tmp/remote.git"

printf '[workspace]\nmembers = []\n' > Cargo.toml
git add -A && git commit -q -m "clean"

fail() { echo "FAIL: $1" >&2; exit 1; }
pushes() { git push -q "$@" >/dev/null 2>&1; }

pushes origin main || fail "干净的 main 应该能推送"

printf '[workspace]\nmembers = []\n\n[patch."https://github.com/byteboyai/bytehost"]\nbytehost-core = { path = "../x" }\n' > Cargo.toml
git commit -qam "dirty: local patch"
pushes origin main && fail "带本地 [patch] 的 main 应该被拦下"
pushes origin HEAD:refs/heads/feature/x || fail "非 main 分支只警告,不应被拦"
git tag v9.9.9
pushes origin v9.9.9 && fail "带本地 [patch] 的 tag 应该被拦下"
pushes origin --delete feature/x || fail "删除远端分支不应触发检查"

git reset -q --hard HEAD~1
git commit -q --allow-empty -m "clean again"
pushes origin main || fail "清理后的 main 应该能推送"

echo "OK: pre-push 钩子行为符合预期"
```

- [ ] **Step 5: 写 README**

创建 `tools/shared-deps-guard/README.md`:

````markdown
# shared-deps-guard

byteboyai 共享库(bytehost、byteui、bytegit、dozer 的共享 crate)的消费者依赖守卫。
规格:digger 仓库 `docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`。

**这里是正本。** 消费者(Digger、Dozer)把下面四个文件 vendor 到自己的仓库,不要在消费者里直接改它们。

| 正本文件 | 消费者里的位置 |
|---|---|
| `check_shared_deps.py` | `scripts/check_shared_deps.py` |
| `check-shared-deps.sh` | `scripts/check-shared-deps.sh` |
| `dev-patch.example.toml` | `scripts/dev-patch.example.toml` |
| `pre-push` | `.githooks/pre-push` |

## 它检查什么

| 规则 | 级别 | 含义 |
|---|---|---|
| `local-patch` | block | 入库的 `Cargo.toml` 里有指向本地路径的 `[patch]` |
| `lock-not-git` | block | `Cargo.lock` 里共享库不是来自远端 git(被本地 patch 覆盖) |
| `lock-duplicate` | block | 同一共享 crate 在 `Cargo.lock` 里有多个 git 来源(传递依赖版本不一致) |
| `ref-mismatch` | block | 同一工作区内同一共享库的 tag/rev 不一致 |
| `ref-missing` | block | 固定的 tag / 分支 / rev 在远端明确不存在 |
| `behind` | warn | 没有 pin 的共享库落后于远端最新语义化 tag |
| `pin-expired` / `manifest-stale` | warn | `shared-deps.toml` 的 pin 已过期 / 已无对应依赖 |
| `ref-unverified` | warn | 联网失败或短 rev,无法确认(放行) |
| `ahead-of-remote` / `guard-source` | info | 本地 main 领先远端 / 正本可能有新版 |

版本号只在各 `Cargo.toml` 里写一份;`shared-deps.toml` 只记录刻意固定的理由:

```toml
[pin.dozer]
reason = "等一次性同步所有共享面板"
review_by = 2026-11-15
```

## 用法

```bash
scripts/check-shared-deps.sh                      # 检查工作区,完整报告
scripts/check-shared-deps.sh --rev HEAD           # 检查已提交内容
scripts/check-shared-deps.sh --offline            # 跳过联网检查
scripts/check-shared-deps.sh --downgrade-blocks   # 阻断降级为警告
git config core.hooksPath .githooks               # 启用 pre-push(每个克隆一次)
```

pre-push 对每个被推送的引用检查它的提交:推 `main` 或 tag 时阻断,推其他分支只警告。
联网失败只警告并放行。确认要绕过时:`git push --no-verify`。

本地联调共享库:把 `dev-patch.example.toml` 复制成 `.cargo/config.toml`(应被 `.gitignore`),
不要把 `[patch]` 写进入库的 `Cargo.toml`。

## vendor 到消费者

在消费者仓库根目录,`TAG` 是发布了本目录的 bytehost tag,`SRC` 指向本地 bytehost 克隆:

```bash
TAG=v0.4.2; SRC=../bytehost
for f in check_shared_deps.py check-shared-deps.sh dev-patch.example.toml; do
  git -C $SRC show "${TAG}:tools/shared-deps-guard/$f" > "scripts/$f"
done
mkdir -p .githooks && git -C $SRC show "${TAG}:tools/shared-deps-guard/pre-push" > .githooks/pre-push
perl -pi -e "s/^SOURCE_TAG = \"unreleased\"/SOURCE_TAG = \"$TAG\"/" scripts/check_shared_deps.py
chmod +x scripts/check-shared-deps.sh .githooks/pre-push
```

## 测试

```bash
python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' -v   # 单元测试,不联网
tools/shared-deps-guard/test_hook.sh                                        # 钩子端到端,真实 git push 到本地裸仓库
```
````

- [ ] **Step 6: 设置可执行位并运行钩子测试**

```bash
chmod +x tools/shared-deps-guard/check-shared-deps.sh tools/shared-deps-guard/pre-push tools/shared-deps-guard/test_hook.sh
tools/shared-deps-guard/test_hook.sh
python3 -m unittest discover -s tools/shared-deps-guard -p 'test_*.py' 2>&1 | tail -3
```
Expected: 先输出 `OK: pre-push 钩子行为符合预期`,再是 `Ran 48 tests ... OK`。

- [ ] **Step 7: 提交**

```bash
git add tools/shared-deps-guard
git commit -m "feat(tools): shared-deps-guard wrapper, pre-push hook, dev-patch template and docs

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: GATE,发布 bytehost 正本

这是**对外动作**(推送 + 打 tag),需要用户明确同意。执行者在这一步停下来,向用户展示下面的信息并等待回复。

- [ ] **Step 1: 汇总给用户确认**

```bash
cd /Users/chrischiang/Projects/CoralProjects/byteboy/bytehost-shared-deps-guard
git fetch -q origin && git log --oneline origin/main..HEAD
git diff --stat origin/main..HEAD | tail -3
git ls-remote --tags origin | sed 's|.*refs/tags/||' | grep -v '\^{}' | sort -V | tail -3
```
向用户说明:将把 `feat/shared-deps-guard` 快进合并到 bytehost `main`,推送,并打一个新 tag。**问用户 tag 用哪个**(建议在最新 tag 的基础上加一个 patch 位,因为只新增了 `tools/`,不改任何 crate;例如最新是 `v0.4.1` 就用 `v0.4.2`)。如果 `origin/main` 在此期间前进了,先 `git rebase origin/main` 再继续。

- [ ] **Step 2: 用户同意后发布**

```bash
TAG=v0.4.2   # 用用户确认的 tag
git -C ../bytehost fetch -q origin
git push origin feat/shared-deps-guard:main     # 必须是快进;被拒绝就停下来,不要 --force
git tag -a "$TAG" -m "shared-deps-guard: tools/shared-deps-guard"
git push origin "$TAG"
git ls-remote --tags origin "$TAG"
```
Expected: 推送成功;最后一行列出新 tag。

- [ ] **Step 3: 把本地 bytehost 主仓库同步到新 main(只快进)**

```bash
git -C ../bytehost status -sb | head -3   # 确认主工作区没有他人未提交改动再动
git -C ../bytehost merge --ff-only origin/main
```
如果 `../bytehost` 有未提交改动或不在 `main`,**跳过这一步**并告诉用户,不要强行处理。

---

### Task 8: Digger 接入

**Files(在 `../digger-shared-deps-guard`):**
- Create: `scripts/check_shared_deps.py`、`scripts/check-shared-deps.sh`、`scripts/dev-patch.example.toml`(vendor)
- Create: `.githooks/pre-push`(vendor)
- Create: `shared-deps.toml`
- Modify: `README.md`(追加一节)

**Interfaces:**
- Consumes: Task 7 发布的 tag(下文记作 `$TAG`)。

- [ ] **Step 1: vendor 四个文件并写入来源 tag**

```bash
cd /Users/chrischiang/Projects/CoralProjects/byteboy/digger-shared-deps-guard
TAG=v0.4.2   # 与 Task 7 一致
SRC=../bytehost
for f in check_shared_deps.py check-shared-deps.sh dev-patch.example.toml; do
  git -C $SRC show "${TAG}:tools/shared-deps-guard/$f" > "scripts/$f"
done
mkdir -p .githooks && git -C $SRC show "${TAG}:tools/shared-deps-guard/pre-push" > .githooks/pre-push
perl -pi -e "s/^SOURCE_TAG = \"unreleased\"/SOURCE_TAG = \"$TAG\"/" scripts/check_shared_deps.py
chmod +x scripts/check-shared-deps.sh .githooks/pre-push
grep -n '^SOURCE_TAG' scripts/check_shared_deps.py
```
Expected: 打印 `SOURCE_TAG = "v0.4.2"`(你的 tag)。

- [ ] **Step 2: 写 `shared-deps.toml`**

```toml
# 刻意固定在某个版本的 byteboyai 共享库;没有写在这里的,默认应该跟上最新 tag。
# 版本号本身只在各 Cargo.toml 里写一份,这里只记录"为什么固定"。
# 到 review_by 之后脚本会提醒重新决定(只提醒,不失败)。
#
# [pin.dozer]
# reason = "等一次性同步所有共享面板"
# review_by = 2026-11-15
```

- [ ] **Step 3: 在 `README.md` 末尾追加说明**

````markdown

## 共享库依赖守卫

`scripts/check-shared-deps.sh` 检查 bytehost / byteui / bytegit / dozer 等 `byteboyai/*` 共享库的依赖:拦下本地路径 `[patch]`、指向本地路径的 `Cargo.lock`、远端不存在的 tag/rev、同一共享库的多份版本,并提示落后。规则与设计见 `docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`。

- 启用 pre-push 钩子(每个克隆一次):`git config core.hooksPath .githooks`。推 `main` 或 tag 时阻断,推其他分支只警告;确认要绕过时用 `git push --no-verify`。
- 随时手动跑:`scripts/check-shared-deps.sh`(检查工作区)或 `scripts/check-shared-deps.sh --rev HEAD`(检查已提交内容)。
- 本地联调共享库:复制 `scripts/dev-patch.example.toml` 到 `.cargo/config.toml`(已被忽略,不入库)。
- 脚本正本在 bytehost 的 `tools/shared-deps-guard/`,本仓库是 vendor 副本(见 `scripts/check_shared_deps.py` 里的 `SOURCE_TAG`)。
````

- [ ] **Step 4: 运行并记录结果**

```bash
scripts/check-shared-deps.sh --rev HEAD
```
Expected:在 `main` 的状态上**会有阻断**(`local-patch`、`lock-not-git`):已提交的 `Cargo.toml` 仍带指向 `../bytehost-chrome` 等 worktree 的 `[patch]`。这是预期结果,**不要在本任务里修**(别的会话的 `chore/align-deps-2026-10` 分支正在对齐依赖;合并后阻断会消失)。把完整输出原样贴给用户。
如果出现 `ref-unverified`,重试一次;仍失败就如实报告。

- [ ] **Step 5: 验证钩子接线**

```bash
git config core.hooksPath .githooks
git push --dry-run origin main 2>&1 | head -5   # digger 目前没有 remote,会报错,只确认钩子文件可执行
ls -l .githooks/pre-push scripts/check-shared-deps.sh
```
Expected: 两个文件都有可执行位。(Digger 目前没有 git remote,钩子的端到端行为已由 `test_hook.sh` 覆盖。)

- [ ] **Step 6: 提交(不推送)**

```bash
git add scripts/check_shared_deps.py scripts/check-shared-deps.sh scripts/dev-patch.example.toml .githooks/pre-push shared-deps.toml README.md
git commit -m "build: vendor shared-deps-guard $TAG with pre-push hook

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Dozer 接入,并取代 `check-shared-pins.sh`

**Files(在 `../dozer-shared-deps-guard`):**
- Create: `scripts/check_shared_deps.py`、`scripts/check-shared-deps.sh`、`scripts/dev-patch.example.toml`、`.githooks/pre-push`、`shared-deps.toml`
- Delete: `scripts/check-shared-pins.sh`(取代它)
- Modify: `README.md`(追加一节)

**Interfaces:**
- Consumes: Task 7 的 `$TAG`。

- [ ] **Step 1: 创建 worktree(从远端 main,不是本地 main)**

```bash
cd /Users/chrischiang/Projects/CoralProjects/byteboy
git -C dozer fetch -q origin main
git -C dozer worktree add ../dozer-shared-deps-guard -b feat/shared-deps-guard origin/main
cd dozer-shared-deps-guard && git status -sb | head -2 && ls scripts/check-shared-pins.sh
```
Expected: 分支 `feat/shared-deps-guard`,干净,`check-shared-pins.sh` 存在。

- [ ] **Step 2: vendor(同 Task 8 Step 1)**

```bash
TAG=v0.4.2   # 与 Task 7 一致
SRC=../bytehost
for f in check_shared_deps.py check-shared-deps.sh dev-patch.example.toml; do
  git -C $SRC show "${TAG}:tools/shared-deps-guard/$f" > "scripts/$f"
done
mkdir -p .githooks && git -C $SRC show "${TAG}:tools/shared-deps-guard/pre-push" > .githooks/pre-push
perl -pi -e "s/^SOURCE_TAG = \"unreleased\"/SOURCE_TAG = \"$TAG\"/" scripts/check_shared_deps.py
chmod +x scripts/check-shared-deps.sh .githooks/pre-push
```

- [ ] **Step 3: 写 `shared-deps.toml`**(内容同 Task 8 Step 2)

- [ ] **Step 4: 确认新脚本覆盖了旧门禁,再删除旧门禁**

旧门禁的自测:构造"同一仓库两个 tag"必须失败。新脚本对应规则是 `ref-mismatch`,已由 Task 3 的测试覆盖。再用真实数据确认两者结论一致:

```bash
bash scripts/check-shared-pins.sh; echo "旧门禁 exit=$?"
scripts/check-shared-deps.sh --rev HEAD --offline; echo "新脚本 exit=$?"
git grep -n "check-shared-pins" -- . ':!scripts/check-shared-pins.sh'
```
Expected: 两者 exit 都是 0;`git grep` 若有引用(文档、脚本、CI 配置),把引用改成 `scripts/check-shared-deps.sh`;没有引用就不用改。

```bash
git rm scripts/check-shared-pins.sh
```

- [ ] **Step 5: 在 `README.md` 末尾追加说明**

与 Task 8 Step 3 相同的一节,把"bytehost / byteui / bytegit / dozer"改成"bytehost / byteui / bytegit",并加一句:`scripts/check-shared-pins.sh` 已被它取代。

- [ ] **Step 6: 运行并记录结果**

```bash
scripts/check-shared-deps.sh --rev HEAD
```
Expected: 无 `BLOCK`(可能有 `behind` 警告)。若出现 `BLOCK`,先判断是脚本误报还是真问题,不要为了让它通过去改规则;把输出贴给用户。

- [ ] **Step 7: 提交(不推送)**

```bash
git add -A scripts .githooks shared-deps.toml README.md
git commit -m "build: vendor shared-deps-guard $TAG, replacing check-shared-pins

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 10: 收尾

- [ ] **Step 1: 汇总给用户**

列出:bytehost 的 tag、Digger 与 Dozer 各自的 worktree 分支与提交、两份基线报告(Task 8 Step 4、Task 9 Step 6 的输出)。明确告知:
- 两个消费者分支**都没有推送**,是否合并、何时推送由用户决定。
- Digger 的钩子一旦启用,在别的会话的依赖对齐分支合并之前,推 `main` 会被拦下(预期行为)。
- Dozer 的 `check-shared-pins.sh` 被取代。
- 升级 Digger/Dozer 的共享库版本、处理 Dozer 未推送的提交,是脚本报告之后的独立任务,不在本计划内。

- [ ] **Step 2: 清理**

不要删除 worktree(`../bytehost-shared-deps-guard`、`../digger-shared-deps-guard`、`../dozer-shared-deps-guard`),分支合并后再清理。
