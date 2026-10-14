// A realistic Markdown chapter: Chinese prose, mixed syntax styles that WYSIWYG serializers normalize
// (setext headings, `_`/`__` emphasis, `1)` lists, `*` bullets, hard breaks, reference links, footnotes,
// tables, HTML comments), plus an image. Every MARK_* is unique and sits inside a plain-text run.
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

export const CORPUS = [
  "# 第一章 开端 MARK_H1",
  "",
  "夜色渐深，城市的灯火一盏盏亮起。这是普通段落 MARK_P1，包含*斜体*与**粗体MARK_BOLD**以及`代码`。",
  "",
  "Setext 标题 MARK_SETEXT",
  "======================",
  "",
  "另一段使用 _下划线斜体_ 和 __下划线粗体__ MARK_UNDER，行尾有硬换行  ",
  "第二行 MARK_BREAK。",
  "",
  "1) 第一项 MARK_OL1",
  "2) 第二项",
  "   - 嵌套项 MARK_NEST",
  "",
  "* 星号列表 MARK_UL",
  "* 第二项",
  "",
  "> 引用 MARK_QUOTE",
  "> > 嵌套引用 MARK_QUOTE2",
  "",
  "```rust",
  'fn main() { println!("MARK_CODE"); }',
  "```",
  "",
  "| 列A | 列B |",
  "|-----|:---:|",
  "| 单元 MARK_CELL | 2 |",
  "",
  "参见[链接][ref] MARK_REF 与脚注[^1]。",
  "",
  "[^1]: 脚注内容 MARK_FN",
  "",
  "<!-- 注释 MARK_COMMENT -->",
  "",
  `![封面图](data:image/png;base64,${PNG}) 图注 MARK_CAP`,
  "",
  '[ref]: https://example.com/ "标题"',
  "",
  "---",
  "",
  "结尾段 MARK_END",
  "",
].join("\n");

export const MARKERS = [
  "MARK_H1", "MARK_P1", "MARK_SETEXT", "MARK_UNDER", "MARK_BREAK", "MARK_OL1", "MARK_NEST", "MARK_UL",
  "MARK_QUOTE", "MARK_QUOTE2", "MARK_CODE", "MARK_CELL", "MARK_REF", "MARK_FN", "MARK_COMMENT", "MARK_CAP", "MARK_BOLD", "MARK_END",
];

/** A long chapter built by repeating the corpus (markers become non-unique; used for performance only). */
export function repeated(times: number): string {
  const parts: string[] = [];
  for (let i = 0; i < times; i++) parts.push(CORPUS.replace(/MARK_/g, `M${i}_`));
  return parts.join("\n");
}
