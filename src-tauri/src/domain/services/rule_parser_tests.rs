
use super::*;

#[test]
fn test_parse_frontmatter_with_paths() {
    let content = r#"---
paths:
  - "src/domain/**"
  - "src-tauri/src/application/**"
---

# Introduction

This is content.
"#;

    let result = RuleParser::parse_content(content).unwrap();
    assert_eq!(result.frontmatter.paths.len(), 2);
    assert_eq!(result.frontmatter.paths[0], "src/domain/**");
    assert_eq!(result.frontmatter.paths[1], "src-tauri/src/application/**");
}

#[test]
fn test_parse_no_frontmatter() {
    let content = r#"# Introduction

This is content without frontmatter.
"#;

    let result = RuleParser::parse_content(content).unwrap();
    assert!(result.frontmatter.paths.is_empty());
    assert!(!result.raw_content.is_empty());
}

#[test]
fn test_chunk_markdown() {
    let content = r#"# Title

Introduction text.

## Section 1

Content for section 1.

## Section 2

Content for section 2.
"#;

    let result = RuleParser::parse_content(content).unwrap();
    assert_eq!(result.chunks.len(), 3);
    assert_eq!(result.chunks[0].title, "Title");
    assert_eq!(result.chunks[0].level, 1);
    assert_eq!(result.chunks[1].title, "Section 1");
    assert_eq!(result.chunks[1].level, 2);
}

#[test]
fn test_parse_header() {
    assert_eq!(
        RuleParser::parse_header("# Title"),
        Some(("Title".to_string(), 1))
    );
    assert_eq!(
        RuleParser::parse_header("## Section"),
        Some(("Section".to_string(), 2))
    );
    assert_eq!(
        RuleParser::parse_header("### Subsection"),
        Some(("Subsection".to_string(), 3))
    );
    assert_eq!(RuleParser::parse_header("Not a header"), None);
    assert_eq!(RuleParser::parse_header(""), None);
}
