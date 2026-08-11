use super::skill_markdown::split_frontmatter;

#[test]
fn split_frontmatter_round_trips_crlf_and_lf() {
    for (raw, frontmatter) in [
        ("---\nname: test\n---\nbody", "name: test"),
        ("---\r\nname: test\r\n---\r\nbody", "name: test\r"),
    ] {
        assert_eq!(split_frontmatter(raw), Some((frontmatter, "body")));
    }
}
