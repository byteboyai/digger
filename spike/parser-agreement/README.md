# parser-agreement spike

一次性验证 ADR-012 验证项 5:Rust 解析器(pulldown-cmark)与编辑器解析器(Lezer,CodeMirror 6 用的那个)对**顶层块边界**是否一致。随时可删。

```bash
./run-all.sh        # 从头跑完整条流水线(首次会下载 CommonMark/GFM 规范、安装 node 依赖、编译 Rust)
```

- `rust/` — `blocks`(pulldown-cmark)、`blocks-comrak`(comrak,对照用)、`gen`(随机文档,复用 block-identity 的生成器)。
- `node/extract.mjs` — 语料:CommonMark 0.31.2 的 652 个官方示例、GFM 规范里的扩展示例、本工作区里的全部 .md 文件。
- `node/edge.mjs` — 74 个手写的写作场景用例:脚注、YAML 头部、数学块、全角空格缩进、表格边界、CRLF、BOM、超长行、表情符号……
- `node/edits.mjs` — 模拟打字:对 150 篇文档各做 14 次随机编辑,每次编辑后检查 Lezer 增量树 = 完整解析,并把中间状态写成语料。
- `node/footnote.mjs` — 给 Lezer 补的脚注定义扩展(复合块)。
- `node/reference.mjs`、`triage*.mjs`、`conformance.mjs` — 用 commonmark.js(规范参考实现)裁判分歧,并给各解析器对官方示例打分。
- `node/check.mjs` — 带"已知偏差白名单"的检查:出现新的分歧就失败。

结果见 ADR-012「Parser agreement results」。语料和输出不入库(可重新生成)。
