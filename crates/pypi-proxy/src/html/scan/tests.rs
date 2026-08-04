use super::*;

#[test]
fn reads_attributes_and_text() {
    let a = anchors(r#"<a href="/x/f.whl" data-yanked="bad">f.whl</a>"#);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].attr("href"), Some("/x/f.whl"));
    assert_eq!(a[0].attr("data-yanked"), Some("bad"));
    assert_eq!(a[0].text, "f.whl");
    // Attribute names are matched case-insensitively.
    assert_eq!(a[0].attr("HREF"), Some("/x/f.whl"));
}

#[test]
fn handles_quoting_styles_and_bare_attributes() {
    let a = anchors("<a href='/a.whl' data-yanked rel=next>a.whl</a>");
    assert_eq!(a[0].attr("href"), Some("/a.whl"));
    // Present-but-valueless, which is how a reasonless yank is written.
    assert_eq!(a[0].attr("data-yanked"), Some(""));
    assert_eq!(a[0].attr("rel"), Some("next"));
}

#[test]
fn does_not_match_other_tags_beginning_with_a() {
    let a = anchors("<article><abbr>x</abbr><address>y</address></article>");
    assert!(a.is_empty());
}

#[test]
fn finds_every_anchor_on_a_page() {
    let page = "<html><body>\n<a href=\"1.whl\">1.whl</a><br/>\n\
                <a href=\"2.whl\">2.whl</a><br/>\n</body></html>";
    let a = anchors(page);
    assert_eq!(a.len(), 2);
    assert_eq!(a[1].text, "2.whl");
}

#[test]
fn tolerates_malformed_markup_without_losing_the_page() {
    // An unclosed anchor must not drop the anchors already scanned.
    let a = anchors("<a href=\"1.whl\">1.whl</a><a href=\"2.whl\">2.whl");
    assert_eq!(a.len(), 2);
    assert_eq!(a[1].attr("href"), Some("2.whl"));
    // An unterminated tag yields nothing rather than looping forever.
    assert!(anchors("<a href=\"x").is_empty());
}

#[test]
fn strips_nested_markup_from_link_text() {
    let a = anchors("<a href=\"x.whl\"><span>x.whl</span></a>");
    assert_eq!(a[0].text, "x.whl");
}
