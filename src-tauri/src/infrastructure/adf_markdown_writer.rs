// Markdown -> ADF / Confluence storage XHTML writer.
// Mirror of the ADF -> markdown reader in atlassian_client.rs (adf_node_to_markdown et al.).
// Hand-rolled line-based parser producing a shared Block/InlineSpan tree, then
// rendered independently to ADF `serde_json::Value` and Confluence storage XHTML.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Paragraph(String),
    Heading(u8, String),
    List {
        ordered: bool,
        items: Vec<ListItem>,
    },
    Code {
        language: Option<String>,
        content: String,
    },
    Blockquote(Vec<Block>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListItem {
    text: String,
    children: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InlineSpan {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Link { text: String, url: String },
}

pub(crate) fn markdown_to_adf(text: &str) -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": parse_blocks(text).iter().map(block_to_adf).collect::<Vec<_>>(),
    })
}

pub(crate) fn markdown_to_storage(text: &str) -> String {
    let mut out = String::new();
    for block in &parse_blocks(text) {
        block_to_storage(block, &mut out);
    }
    out
}

fn parse_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
        } else if fence_marker(line).is_some() && find_fence_end(&lines, i).is_some() {
            let lang = fence_marker(line).unwrap_or_default();
            let (block, next) = parse_code_block(&lines, i, lang);
            blocks.push(block);
            i = next;
        } else if let Some((level, content)) = parse_heading(line) {
            blocks.push(Block::Heading(level, content.to_string()));
            i += 1;
        } else if line.trim_start().starts_with('>') {
            let (block, next) = parse_blockquote(&lines, i);
            blocks.push(block);
            i = next;
        } else if list_item_prefix(line).is_some() {
            let (block, next) = parse_list(&lines, i);
            blocks.push(block);
            i = next;
        } else {
            let (block, next) = parse_paragraph(&lines, i);
            blocks.push(block);
            i = next;
        }
    }
    blocks
}

fn fence_marker(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("```")
}

fn find_fence_end(lines: &[&str], start: usize) -> Option<usize> {
    (start + 1..lines.len()).find(|&j| lines[j].trim() == "```")
}

fn parse_code_block(lines: &[&str], start: usize, lang: &str) -> (Block, usize) {
    let end = find_fence_end(lines, start).unwrap_or(lines.len().saturating_sub(1));
    let content = lines[(start + 1).min(end)..end].join("\n");
    let language = if lang.trim().is_empty() {
        None
    } else {
        Some(lang.trim().to_string())
    };
    (Block::Code { language, content }, end + 1)
}

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    let rest = rest.strip_prefix(' ')?;
    Some((hashes as u8, rest.trim()))
}

fn parse_blockquote(lines: &[&str], start: usize) -> (Block, usize) {
    let mut content_lines = Vec::new();
    let mut i = start;
    while i < lines.len() && lines[i].trim_start().starts_with('>') {
        let stripped = lines[i].trim_start().trim_start_matches('>');
        let stripped = stripped.strip_prefix(' ').unwrap_or(stripped);
        content_lines.push(stripped);
        i += 1;
    }
    let inner = content_lines.join("\n");
    (Block::Blockquote(parse_blocks(&inner)), i)
}

fn list_item_prefix(line: &str) -> Option<(usize, bool, &str)> {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    let rest = &line[indent..];
    if let Some(after) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
        return Some((indent, false, after));
    }
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let after_digits = &rest[digits.len()..];
    after_digits
        .strip_prefix(". ")
        .map(|after| (indent, true, after))
}

fn parse_list(lines: &[&str], start: usize) -> (Block, usize) {
    let mut items = Vec::new();
    let mut i = start;
    while i < lines.len() {
        if let Some((indent, ordered, text)) = list_item_prefix(lines[i]) {
            items.push((indent, ordered, text.to_string()));
            i += 1;
        } else {
            break;
        }
    }
    let base_indent = items[0].0;
    let ordered = items[0].1;
    let (list_items, _) = build_list_items(&items, base_indent);
    (
        Block::List {
            ordered,
            items: list_items,
        },
        i,
    )
}

fn build_list_items(items: &[(usize, bool, String)], base_indent: usize) -> (Vec<ListItem>, usize) {
    let mut result = Vec::new();
    let mut i = 0;
    while i < items.len() && items[i].0 == base_indent {
        let text = items[i].2.clone();
        i += 1;
        let mut children = Vec::new();
        if i < items.len() && items[i].0 > base_indent {
            let nested_indent = items[i].0;
            let nested_ordered = items[i].1;
            let (nested_items, consumed) = build_list_items(&items[i..], nested_indent);
            children.push(Block::List {
                ordered: nested_ordered,
                items: nested_items,
            });
            i += consumed;
        }
        result.push(ListItem { text, children });
    }
    (result, i)
}

fn parse_paragraph(lines: &[&str], start: usize) -> (Block, usize) {
    let mut collected = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty()
            || parse_heading(line).is_some()
            || list_item_prefix(line).is_some()
            || line.trim_start().starts_with('>')
            || (fence_marker(line).is_some() && find_fence_end(lines, i).is_some())
        {
            break;
        }
        collected.push(line);
        i += 1;
    }
    (Block::Paragraph(collected.join("\n")), i)
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&j| chars[j] == target)
}

fn find_str(chars: &[char], from: usize, target: &str) -> Option<usize> {
    let needle: Vec<char> = target.chars().collect();
    if needle.is_empty() || from + needle.len() > chars.len() {
        return None;
    }
    (from..=chars.len() - needle.len()).find(|&j| chars[j..j + needle.len()] == needle[..])
}

fn parse_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let close_bracket = find_char(chars, start + 1, ']')?;
    if chars.get(close_bracket + 1) != Some(&'(') {
        return None;
    }
    let close_paren = find_char(chars, close_bracket + 2, ')')?;
    let text: String = chars[start + 1..close_bracket].iter().collect();
    let url: String = chars[close_bracket + 2..close_paren].iter().collect();
    Some((text, url, close_paren + 1))
}

fn parse_inline(text: &str) -> Vec<InlineSpan> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '`' {
            // `end > i + 1` keeps an empty span (```` `` ````) literal instead of
            // swallowing both backticks — that is what makes an unterminated
            // ```` ```lang ```` fence degrade to its own raw text.
            if let Some(end) = find_char(&chars, i + 1, '`').filter(|end| *end > i + 1) {
                flush_text(&mut buf, &mut spans);
                spans.push(InlineSpan::Code(chars[i + 1..end].iter().collect()));
                i = end + 1;
                continue;
            }
        } else if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find_str(&chars, i + 2, "**") {
                flush_text(&mut buf, &mut spans);
                spans.push(InlineSpan::Bold(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        } else if c == '*' {
            if let Some(end) = find_char(&chars, i + 1, '*') {
                flush_text(&mut buf, &mut spans);
                spans.push(InlineSpan::Italic(chars[i + 1..end].iter().collect()));
                i = end + 1;
                continue;
            }
        } else if c == '[' {
            if let Some((text, url, next)) = parse_link(&chars, i) {
                flush_text(&mut buf, &mut spans);
                spans.push(InlineSpan::Link { text, url });
                i = next;
                continue;
            }
        }
        buf.push(c);
        i += 1;
    }
    flush_text(&mut buf, &mut spans);
    spans
}

fn flush_text(buf: &mut String, spans: &mut Vec<InlineSpan>) {
    if !buf.is_empty() {
        spans.push(InlineSpan::Text(std::mem::take(buf)));
    }
}

fn text_node(text: &str, marks: &[Value]) -> Option<Value> {
    if text.is_empty() {
        return None;
    }
    let mut node = json!({"type": "text", "text": text});
    if !marks.is_empty() {
        node["marks"] = json!(marks);
    }
    Some(node)
}

fn span_to_adf(span: &InlineSpan) -> Option<Value> {
    match span {
        InlineSpan::Text(t) => text_node(t, &[]),
        InlineSpan::Bold(t) => text_node(t, &[json!({"type": "strong"})]),
        InlineSpan::Italic(t) => text_node(t, &[json!({"type": "em"})]),
        InlineSpan::Code(t) => text_node(t, &[json!({"type": "code"})]),
        InlineSpan::Link { text, url } => {
            text_node(text, &[json!({"type": "link", "attrs": {"href": url}})])
        }
    }
}

fn inline_spans_to_adf(spans: &[InlineSpan]) -> Vec<Value> {
    spans.iter().filter_map(span_to_adf).collect()
}

fn paragraph_content_adf(text: &str) -> Vec<Value> {
    let mut content = Vec::new();
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            content.push(json!({"type": "hardBreak"}));
        }
        content.extend(inline_spans_to_adf(&parse_inline(line)));
    }
    content
}

fn list_item_to_adf(item: &ListItem) -> Value {
    let mut content = vec![json!({
        "type": "paragraph",
        "content": paragraph_content_adf(&item.text),
    })];
    content.extend(item.children.iter().map(block_to_adf));
    json!({"type": "listItem", "content": content})
}

fn block_to_adf(block: &Block) -> Value {
    match block {
        Block::Paragraph(text) => json!({
            "type": "paragraph",
            "content": paragraph_content_adf(text),
        }),
        Block::Heading(level, text) => json!({
            "type": "heading",
            "attrs": {"level": level},
            "content": inline_spans_to_adf(&parse_inline(text)),
        }),
        Block::List { ordered, items } => {
            let node_type = if *ordered {
                "orderedList"
            } else {
                "bulletList"
            };
            json!({
                "type": node_type,
                "content": items.iter().map(list_item_to_adf).collect::<Vec<_>>(),
            })
        }
        Block::Code { language, content } => {
            let mut node = json!({
                "type": "codeBlock",
                "content": if content.is_empty() {
                    Vec::new()
                } else {
                    vec![json!({"type": "text", "text": content})]
                },
            });
            if let Some(lang) = language {
                node["attrs"] = json!({"language": lang});
            }
            node
        }
        Block::Blockquote(children) => json!({
            "type": "blockquote",
            "content": children.iter().map(block_to_adf).collect::<Vec<_>>(),
        }),
    }
}

fn escape_xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_attr(text: &str) -> String {
    escape_xml_text(text)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn inline_spans_to_storage(spans: &[InlineSpan], out: &mut String) {
    for span in spans {
        match span {
            InlineSpan::Text(t) => out.push_str(&escape_xml_text(t)),
            InlineSpan::Bold(t) => {
                out.push_str("<strong>");
                out.push_str(&escape_xml_text(t));
                out.push_str("</strong>");
            }
            InlineSpan::Italic(t) => {
                out.push_str("<em>");
                out.push_str(&escape_xml_text(t));
                out.push_str("</em>");
            }
            InlineSpan::Code(t) => {
                out.push_str("<code>");
                out.push_str(&escape_xml_text(t));
                out.push_str("</code>");
            }
            InlineSpan::Link { text, url } => {
                out.push_str(r#"<a href=""#);
                out.push_str(&escape_xml_attr(url));
                out.push_str(r#"">"#);
                out.push_str(&escape_xml_text(text));
                out.push_str("</a>");
            }
        }
    }
}

fn paragraph_text_to_storage(text: &str, out: &mut String) {
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            out.push_str("<br/>");
        }
        inline_spans_to_storage(&parse_inline(line), out);
    }
}

fn block_to_storage(block: &Block, out: &mut String) {
    match block {
        Block::Paragraph(text) => {
            out.push_str("<p>");
            paragraph_text_to_storage(text, out);
            out.push_str("</p>");
        }
        Block::Heading(level, text) => {
            out.push_str(&format!("<h{level}>"));
            inline_spans_to_storage(&parse_inline(text), out);
            out.push_str(&format!("</h{level}>"));
        }
        Block::List { ordered, items } => {
            let tag = if *ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag}>"));
            for item in items {
                out.push_str("<li>");
                inline_spans_to_storage(&parse_inline(&item.text), out);
                for child in &item.children {
                    block_to_storage(child, out);
                }
                out.push_str("</li>");
            }
            out.push_str(&format!("</{tag}>"));
        }
        Block::Code { language, content } => {
            out.push_str(r#"<ac:structured-macro ac:name="code">"#);
            if let Some(lang) = language {
                out.push_str(r#"<ac:parameter ac:name="language">"#);
                out.push_str(&escape_xml_text(lang));
                out.push_str("</ac:parameter>");
            }
            out.push_str("<ac:plain-text-body>");
            out.push_str(&escape_xml_text(content));
            out.push_str("</ac:plain-text-body>");
            out.push_str("</ac:structured-macro>");
        }
        Block::Blockquote(children) => {
            out.push_str("<blockquote>");
            for child in children {
                block_to_storage(child, out);
            }
            out.push_str("</blockquote>");
        }
    }
}
