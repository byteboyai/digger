//! Top-level Markdown blocks with source ranges (the "source map" half of Markdown Core).

use pulldown_cmark::{Event, Options, Parser, Tag};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Heading(u8),
    Paragraph,
    List,
    Quote,
    Code,
    Table,
    Rule,
    Html,
    Footnote,
    Other,
}

#[derive(Clone, Debug)]
pub struct Parsed {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

fn kind_of(tag: &Tag) -> Kind {
    match tag {
        Tag::Heading { level, .. } => Kind::Heading(*level as u8),
        Tag::Paragraph => Kind::Paragraph,
        Tag::List(_) => Kind::List,
        Tag::BlockQuote(_) => Kind::Quote,
        Tag::CodeBlock(_) => Kind::Code,
        Tag::Table(_) => Kind::Table,
        Tag::HtmlBlock => Kind::Html,
        Tag::FootnoteDefinition(_) => Kind::Footnote,
        _ => Kind::Other,
    }
}

/// CommonMark + GFM tables, footnotes, strikethrough, task lists.
pub fn parse_blocks(text: &str) -> Vec<Parsed> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut open: Option<(Kind, usize)> = None;
    for (ev, range) in Parser::new_ext(text, opts).into_offset_iter() {
        match ev {
            Event::Start(tag) => {
                if depth == 0 {
                    open = Some((kind_of(&tag), range.start));
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    if let Some((kind, start)) = open.take() {
                        push(&mut out, text, kind, start, range.end);
                    }
                }
            }
            Event::Rule if depth == 0 => push(&mut out, text, Kind::Rule, range.start, range.end),
            _ => {}
        }
    }
    out
}

fn push(out: &mut Vec<Parsed>, text: &str, kind: Kind, start: usize, mut end: usize) {
    while end > start && text.as_bytes()[end - 1] == b'\n' {
        end -= 1;
    }
    out.push(Parsed { kind, start, end, text: text[start..end].to_string() });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_chapter_into_top_level_blocks_with_exact_ranges() {
        let t = "# 标题\n\n第一段。\n\n- a\n- b\n\n> 引用\n\n```\ncode\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n\n末段\n";
        let b = parse_blocks(t);
        let kinds: Vec<_> = b.iter().map(|x| x.kind.clone()).collect();
        assert_eq!(kinds, [Kind::Heading(1), Kind::Paragraph, Kind::List, Kind::Quote, Kind::Code, Kind::Table, Kind::Rule, Kind::Paragraph]);
        for x in &b {
            assert_eq!(&t[x.start..x.end], x.text);
        }
        assert_eq!(b[2].text, "- a\n- b");
    }

    #[test]
    fn setext_heading_and_reference_definitions() {
        let b = parse_blocks("Setext\n======\n\n[ref]: https://x.y\n\n正文\n");
        assert_eq!(b.len(), 2, "the reference definition is not a block");
        assert_eq!(b[0].kind, Kind::Heading(1));
    }
}
