// Hand-written cases for constructs that matter in a manuscript but are missing from the CommonMark examples:
// footnotes, front matter, math, CJK typesetting, table edge cases, odd line endings, long lines, emoji.
import fs from "node:fs";
const C = [];
const add = (id, text, section) => C.push({ id, source: "edge", section, text });

// footnotes and notes
add("footnote-1", "正文[^1]。\n\n[^1]: 脚注内容\n\n下一段\n", "footnote");
add("footnote-multi", "正文[^a]。\n\n[^a]: 第一段\n\n    第二段仍属于脚注\n\n下一段\n", "footnote");
add("footnote-in-list", "- 项目[^x]\n\n[^x]: 脚注\n", "footnote");
add("footnote-end", "最后一段[^z]。\n\n[^z]: 末尾脚注", "footnote");
// front matter / metadata
add("front-yaml", "---\ntitle: 书名\nauthor: 某某\n---\n\n# 第一章\n\n正文\n", "front matter");
add("front-yaml-tight", "---\ntitle: x\n---\n正文紧跟\n", "front matter");
add("front-toml", "+++\ntitle = \"x\"\n+++\n\n# 题\n", "front matter");
add("rule-then-text", "---\n\n正文\n\n---\n\n正文二\n", "front matter");
// math, containers, definition lists, wiki links
add("math-block", "$$\nE = mc^2\n$$\n\n正文\n", "extension");
add("math-inline-only", "公式 $a_1 + b_2$ 与 $c_3$ 之间。\n\n下一段\n", "extension");
add("container", ":::note\n注意事项\n:::\n\n正文\n", "extension");
add("deflist", "术语\n: 定义一\n: 定义二\n\n下一段\n", "extension");
add("wikilink", "参见 [[另一章]] 的内容。\n\n[[单独一行]]\n", "extension");
// tables
add("table-pipes-in-code", "| a | b |\n|---|---|\n| `x|y` | c\\|d |\n\n后文\n", "table");
add("table-no-edge-pipes", "a | b\n--|--\n1 | 2\n\n后文\n", "table");
add("table-after-paragraph", "段落\n| a | b |\n|---|---|\n| 1 | 2 |\n\n后文\n", "table");
add("table-align", "| 左 | 中 | 右 |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n", "table");
add("table-cjk", "| 名称 | 说明 |\n| --- | --- |\n| 甲 | 第一项，含逗号 |\n| 乙 | 第二项 |\n", "table");
add("table-ragged", "| a | b | c |\n|---|---|---|\n| 1 |\n| 1 | 2 | 3 | 4 |\n", "table");
add("table-then-list", "| a | b |\n|---|---|\n| 1 | 2 |\n- 紧跟的列表\n", "table");
// lists
add("tasks", "- [ ] 待办\n- [x] 完成\n- [X] 大写\n", "list");
add("list-nested-2", "- 一\n  - 二\n    - 三\n- 四\n", "list");
add("list-nested-4", "1. 一\n    - 二\n        - 三\n2. 四\n", "list");
add("list-loose", "- 甲\n\n- 乙\n\n- 丙\n", "list");
add("list-code-in-item", "- 项\n\n  ```\n  code\n  ```\n\n- 项二\n", "list");
add("list-comment-split", "- a\n- b\n\n<!-- -->\n\n- c\n", "list");
add("list-start-number", "文字\n2. 不会打断段落\n", "list");
add("list-interrupt", "文字\n1. 会打断段落\n2. 第二项\n", "list");
add("para-then-list", "段落\n- a\n- b\n", "list");
add("para-then-quote", "段落\n> 引用\n", "list");
add("para-then-fence", "段落\n```\ncode\n```\n", "list");
add("para-then-heading", "段落\n# 标题\n", "list");
add("para-then-html", "段落\n<div>x</div>\n", "list");
add("list-lazy", "- 项目\n延续行\n- 下一项\n", "list");
add("list-indented-continuation", "1. 项\n   - 子项\n    续行(四空格)\n2. 项二\n", "list");
// headings and rules
add("atx-no-space", "#标题\n\n# 标题 #\n\n####### 七个\n\n###### 六个\n", "heading");
add("setext", "标题一\n======\n\n标题二\n------\n\n多行\n标题\n===\n", "heading");
add("setext-vs-rule", "Foo\n---\n\nFoo\n\n---\n", "heading");
add("rules", "***\n\n___\n\n- - -\n\n* * *\n", "heading");
add("heading-tab", "#\t标题\n\n#\t\t二\n", "heading");
// html
add("html-div-blank", "<div>\n\n*强调*\n\n</div>\n\n后文\n", "html");
add("html-comment-multi", "<!--\n多行\n注释\n-->\n\n后文\n", "html");
add("html-details", "<details>\n<summary>点开</summary>\n\n内容\n\n</details>\n", "html");
add("html-inline-start", "<span>行内</span> 之后还有文字\n\n后文\n", "html");
add("html-img-line", "<img src=\"a.png\" alt=\"x\">\n\n<img src=\"a.png\" alt=\"x\"> 文字\n\n后文\n", "html");
add("html-textarea", "<textarea>\n\n*foo*\n\n</textarea>\n", "html");
add("html-pre", "<pre>\n\n*foo*\n\n</pre>\n", "html");
// quotes and code
add("quote-lazy", "> 引用\n延续\n\n> 二\n\n> 三\n", "quote");
add("quote-nested-list", "> - 项\n>   > 嵌套\n> \n> 结束\n", "quote");
add("fence-tilde", "~~~\ncode\n~~~\n\n````\n```\n内含三反引号\n```\n````\n", "code");
add("fence-unclosed", "```\n没有结束\n\n仍在代码里\n", "code");
add("fence-info", "```mermaid\nflowchart LR\n  A --> B\n```\n\n```rust,ignore\nfn x() {}\n```\n", "code");
add("indented-code", "    缩进代码\n    第二行\n\n正文\n", "code");
add("code-after-list", "- 项\n\n      缩进代码?\n", "code");
// images, links, autolinks
add("image-title", "![封面](a.png \"标题\")\n\n[![徽章](b.svg)](https://x.y)\n", "inline-block");
add("autolink-line", "<https://example.com/a>\n\nhttps://example.com/bare\n", "inline-block");
add("linkref", "[链接][a]\n\n[a]: https://x.y \"标题\"\n[b]: https://z.w\n    \"另起一行的标题\"\n\n后文\n", "linkref");
add("linkref-between", "段落一\n[a]: https://x.y\n段落二\n", "linkref");
add("linkref-in-list", "- [a]: https://x.y\n- 项\n", "linkref");
// Chinese typesetting and Unicode
add("cjk-indent", "　　全角空格缩进的段首。\n　　第二行。\n\n　　第二段。\n", "cjk");
add("cjk-hardwrap", "这是一段在中间硬换行的\n中文文字，不应被拆开。\n\n下一段\n", "cjk");
add("cjk-punct-lead", "。句号开头\n\n、顿号开头\n", "cjk");
add("astral", "😀 表情开头的段落 👨‍👩‍👧\n\n- 🎉 列表项\n- 𠮷野家\n\n> 🧪 引用\n\n```\n𝒳 = 1\n```\n", "unicode");
// line endings and boundaries
add("crlf", "# 标题\r\n\r\n段落一\r\n续行\r\n\r\n- a\r\n- b\r\n\r\n```\r\ncode\r\n```\r\n", "line endings");
add("bom", "﻿# 标题\n\n正文\n", "line endings");
add("no-trailing-newline", "# 标题\n\n正文", "line endings");
add("tabs", "- a\n\t- b\n\t\t- c\n\n\t缩进代码\n", "line endings");
add("empty", "", "line endings");
add("whitespace-only", "  \n\n\t\n", "line endings");
add("trailing-spaces-hard-break", "第一行  \n第二行\\\n第三行\n\n新段\n", "line endings");
add("blank-lines-many", "一\n\n\n\n\n二\n\n\n三\n", "line endings");
add("long-line", "很长的一行" + "字".repeat(100000) + "\n\n后文\n", "line endings");
add("deep-nesting", ">".repeat(40) + " 深层引用\n\n" + "- ".repeat(20) + "深层列表\n", "line endings");

// A realistic chapter assembled from the above, with a title block and several footnotes
add("chapter", "---\ntitle: 第一卷\n---\n\n# 第一章 开端\n\n　　夜色渐深，城市的灯火一盏盏亮起[^1]。\n\n　　他推开门，雨声涌了进来。\n\n## 一\n\n> “你来了。”\n> 她没有回头。\n\n- [ ] 修改第三段\n- [x] 核对人名\n\n| 人物 | 出场 |\n|---|---|\n| 甲 | 第一章 |\n\n![封面](assets/cover.png \"封面\")\n\n[^1]: 参见第七章。\n", "chapter");

fs.writeFileSync("../corpus/edge.jsonl", C.map((d) => JSON.stringify(d)).join("\n") + "\n");
console.log(`${C.length} edge cases`);
