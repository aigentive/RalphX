use serde_json::json;

use super::adf_markdown_writer::{markdown_to_adf, markdown_to_storage};
use super::atlassian_client::jira_rich_text;

#[test]
fn markdown_to_adf_plain_paragraph() {
    let doc = markdown_to_adf("Hello world");
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Hello world"}]}
            ]
        })
    );
}

#[test]
fn markdown_to_adf_heading_levels_one_through_six() {
    for level in 1..=6u8 {
        let input = format!("{} Section", "#".repeat(level as usize));
        let doc = markdown_to_adf(&input);
        assert_eq!(
            doc,
            json!({
                "type": "doc",
                "version": 1,
                "content": [
                    {
                        "type": "heading",
                        "attrs": {"level": level},
                        "content": [{"type": "text", "text": "Section"}]
                    }
                ]
            }),
            "level {level} heading mismatch"
        );
    }
}

#[test]
fn markdown_to_adf_seven_hashes_degrades_to_paragraph() {
    let doc = markdown_to_adf("####### Not a heading");
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "####### Not a heading"}]}
            ]
        })
    );
}

#[test]
fn markdown_to_adf_horizontal_rule_degrades_to_paragraph() {
    let doc = markdown_to_adf("---");
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "---"}]}
            ]
        })
    );
}

#[test]
fn markdown_to_adf_bullet_list_with_nesting() {
    let input = "- item one\n  - nested a\n  - nested b\n- item two";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "bulletList",
                    "content": [
                        {
                            "type": "listItem",
                            "content": [
                                {"type": "paragraph", "content": [{"type": "text", "text": "item one"}]},
                                {
                                    "type": "bulletList",
                                    "content": [
                                        {
                                            "type": "listItem",
                                            "content": [
                                                {"type": "paragraph", "content": [{"type": "text", "text": "nested a"}]}
                                            ]
                                        },
                                        {
                                            "type": "listItem",
                                            "content": [
                                                {"type": "paragraph", "content": [{"type": "text", "text": "nested b"}]}
                                            ]
                                        }
                                    ]
                                }
                            ]
                        },
                        {
                            "type": "listItem",
                            "content": [
                                {"type": "paragraph", "content": [{"type": "text", "text": "item two"}]}
                            ]
                        }
                    ]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_ordered_list_simple() {
    let doc = markdown_to_adf("1. first\n2. second\n3. third");
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "orderedList",
                    "content": [
                        {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "first"}]}]},
                        {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "second"}]}]},
                        {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "third"}]}]}
                    ]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_fenced_code_with_language() {
    let input = "```rust\nfn main() {}\n```";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "codeBlock",
                    "attrs": {"language": "rust"},
                    "content": [{"type": "text", "text": "fn main() {}"}]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_fenced_code_without_language() {
    let input = "```\nplain code\n```";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "codeBlock",
                    "content": [{"type": "text", "text": "plain code"}]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_blockquote_multiline() {
    let input = "> Quoted line one\n> Quoted line two";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "blockquote",
                    "content": [
                        {
                            "type": "paragraph",
                            "content": [
                                {"type": "text", "text": "Quoted line one"},
                                {"type": "hardBreak"},
                                {"type": "text", "text": "Quoted line two"}
                            ]
                        }
                    ]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_inline_marks_bold_italic_code_link() {
    let input = "This is **bold**, *italic*, `code`, and a [link](https://example.com).";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "paragraph",
                    "content": [
                        {"type": "text", "text": "This is "},
                        {"type": "text", "text": "bold", "marks": [{"type": "strong"}]},
                        {"type": "text", "text": ", "},
                        {"type": "text", "text": "italic", "marks": [{"type": "em"}]},
                        {"type": "text", "text": ", "},
                        {"type": "text", "text": "code", "marks": [{"type": "code"}]},
                        {"type": "text", "text": ", and a "},
                        {
                            "type": "text",
                            "text": "link",
                            "marks": [{"type": "link", "attrs": {"href": "https://example.com"}}]
                        },
                        {"type": "text", "text": "."}
                    ]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_empty_and_whitespace_only_never_panics() {
    assert_eq!(
        markdown_to_adf(""),
        json!({"type": "doc", "version": 1, "content": []})
    );
    assert_eq!(
        markdown_to_adf("   \n\t\n   "),
        json!({"type": "doc", "version": 1, "content": []})
    );
}

#[test]
fn markdown_to_adf_unterminated_fence_degrades_to_paragraph() {
    let input = "```rust\nfn main() {}";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "paragraph",
                    "content": [
                        {"type": "text", "text": "```rust"},
                        {"type": "hardBreak"},
                        {"type": "text", "text": "fn main() {}"}
                    ]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_malformed_link_degrades_to_plain_text() {
    let input = "See [broken link(no closing paren";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "See [broken link(no closing paren"}]}
            ]
        })
    );
}

#[test]
fn markdown_to_storage_escapes_ampersand_lt_gt() {
    let input = "5 < 10 & 10 > 5, <script>bad()</script>";
    let html = markdown_to_storage(input);
    assert_eq!(
        html,
        "<p>5 &lt; 10 &amp; 10 &gt; 5, &lt;script&gt;bad()&lt;/script&gt;</p>"
    );
    assert!(!html.contains("<script>"));
}

#[test]
fn markdown_to_storage_escapes_link_href_attribute() {
    let input = "[click](http://example.com/a&b\"c)";
    let html = markdown_to_storage(input);
    assert_eq!(
        html,
        "<p><a href=\"http://example.com/a&amp;b&quot;c\">click</a></p>"
    );
}

#[test]
fn markdown_to_storage_headings_paragraphs_and_inline_marks() {
    let html = markdown_to_storage("# Title");
    assert_eq!(html, "<h1>Title</h1>");

    let html = markdown_to_storage("This is **bold** and *italic* and `code`.");
    assert_eq!(
        html,
        "<p>This is <strong>bold</strong> and <em>italic</em> and <code>code</code>.</p>"
    );
}

#[test]
fn markdown_to_storage_bullet_list_with_nesting() {
    let input = "- item one\n  - nested a\n  - nested b\n- item two";
    let html = markdown_to_storage(input);
    assert_eq!(
        html,
        "<ul><li>item one<ul><li>nested a</li><li>nested b</li></ul></li><li>item two</li></ul>"
    );
}

#[test]
fn markdown_to_storage_ordered_list() {
    let html = markdown_to_storage("1. one\n2. two");
    assert_eq!(html, "<ol><li>one</li><li>two</li></ol>");
}

#[test]
fn markdown_to_storage_blockquote_multiline() {
    let input = "> Quoted line one\n> Quoted line two";
    let html = markdown_to_storage(input);
    assert_eq!(
        html,
        "<blockquote><p>Quoted line one<br/>Quoted line two</p></blockquote>"
    );
}

#[test]
fn markdown_to_storage_code_block_with_and_without_language() {
    let with_lang = markdown_to_storage("```rust\nfn main() {}\n```");
    assert_eq!(
        with_lang,
        "<ac:structured-macro ac:name=\"code\"><ac:parameter ac:name=\"language\">rust</ac:parameter><ac:plain-text-body>fn main() {}</ac:plain-text-body></ac:structured-macro>"
    );

    let without_lang = markdown_to_storage("```\nplain code\n```");
    assert_eq!(
        without_lang,
        "<ac:structured-macro ac:name=\"code\"><ac:plain-text-body>plain code</ac:plain-text-body></ac:structured-macro>"
    );
}

#[test]
fn markdown_to_storage_empty_and_whitespace_only_never_panics() {
    assert_eq!(markdown_to_storage(""), "");
    assert_eq!(markdown_to_storage("   \n\t\n   "), "");
}

// Round-trip via the reader's `jira_rich_text` (adf_node_to_markdown mirror). Each case uses a
// single top-level block so the reader's single-`\n` block joiner never enters the comparison,
// and every line is already free of trailing whitespace / blank-line runs so `normalize_markdown`
// (applied internally by `jira_rich_text`) is a no-op — making direct string equality valid.
#[test]
fn round_trip_plain_paragraph() {
    let input = "Hello world";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn round_trip_multiline_paragraph_via_hard_break() {
    let input = "Line one\nLine two";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn round_trip_heading() {
    let input = "## Section";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn round_trip_bullet_list_no_nesting() {
    let input = "- item one\n- item two";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn round_trip_ordered_list_no_nesting() {
    let input = "1. first\n2. second";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn round_trip_blockquote_multiline() {
    let input = "> Quoted line one\n> Quoted line two";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn round_trip_inline_marks_bold_italic_code_link() {
    let input = "This is **bold**, *italic*, `code`, and a [link](https://example.com).";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, input);
}

#[test]
fn markdown_to_adf_deeply_nested_blockquote_bounds_recursion() {
    let input = format!("{}x", "> ".repeat(50_000));
    let doc = markdown_to_adf(&input);
    let rendered = serde_json::to_string(&doc).expect("doc serializes");
    assert!(
        rendered.contains('x'),
        "degraded document must still contain the literal text"
    );
}

#[test]
fn markdown_to_storage_deeply_nested_blockquote_bounds_recursion() {
    let input = format!("{}x", "> ".repeat(50_000));
    let html = markdown_to_storage(&input);
    assert!(
        html.contains('x'),
        "degraded storage output must still contain the literal text"
    );
}

#[test]
fn markdown_to_adf_three_level_blockquote_still_nests_exactly_three() {
    let input = "> > > deep";
    let doc = markdown_to_adf(input);
    assert_eq!(
        doc,
        json!({
            "type": "doc",
            "version": 1,
            "content": [
                {
                    "type": "blockquote",
                    "content": [
                        {
                            "type": "blockquote",
                            "content": [
                                {
                                    "type": "blockquote",
                                    "content": [
                                        {
                                            "type": "paragraph",
                                            "content": [{"type": "text", "text": "deep"}]
                                        }
                                    ]
                                }
                            ]
                        }
                    ]
                }
            ]
        })
    );
}

#[test]
fn markdown_to_adf_deeply_indented_list_degrades_without_losing_item_text() {
    let depth = 40;
    let mut lines = Vec::new();
    for level in 0..depth {
        lines.push(format!("{}- item{level}", "  ".repeat(level)));
    }
    let input = lines.join("\n");
    let doc = markdown_to_adf(&input);
    let rendered = serde_json::to_string(&doc).expect("doc serializes");
    for level in 0..depth {
        assert!(
            rendered.contains(&format!("item{level}")),
            "expected item{level} text to survive the depth cap"
        );
    }
}

#[test]
fn round_trip_fenced_code_language_is_lost_known_reader_asymmetry() {
    // The reader (`adf_node_to_markdown`, atlassian_client.rs) always emits a bare ``` fence with
    // no language, regardless of `attrs.language`. This writer intentionally still emits
    // `attrs.language` in the ADF (option (a) from the task): the round trip holds modulo the
    // code-fence language tag, which is lost on the way back through the reader.
    let input = "```rust\nfn go() {}\n```";
    let rendered = jira_rich_text(&markdown_to_adf(input)).markdown;
    assert_eq!(rendered, "```\nfn go() {}\n```");
    assert_ne!(rendered, input);
}
