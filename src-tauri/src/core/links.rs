use crate::core::slug::slugify;

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub text: String,
    pub target_slug: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Text(String),
    Link { text: String, target_slug: String },
}

/// Split a body into ordered `Text` and `Link` runs.
///
/// A link is `[[inner]]` where `inner` trims to non-empty and slugifies to non-empty.
/// Empty (`[[]]`), whitespace-only, and unbalanced (`[[` with no closing `]]`) brackets are
/// left as plain text. The scan only matches ASCII `[`/`]`, so slice boundaries are always
/// valid UTF-8 and it never panics.
pub fn segment_body(body: &str) -> Vec<Segment> {
    let bytes = body.as_bytes();
    let mut segments = Vec::new();
    let mut text_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let opens = bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[';
        if opens {
            if let Some(rel) = body[i + 2..].find("]]") {
                let inner_raw = &body[i + 2..i + 2 + rel];
                let inner = inner_raw.trim();
                let slug = slugify(inner);
                // Reject if the inner span itself contains [[  — that means the opening
                // [[ was really an unbalanced bracket, not the start of a valid link.
                if !inner.is_empty() && !slug.is_empty() && !inner_raw.contains("[[") {
                    if text_start < i {
                        segments.push(Segment::Text(body[text_start..i].to_string()));
                    }
                    segments.push(Segment::Link {
                        text: inner.to_string(),
                        target_slug: slug,
                    });
                    i = i + 2 + rel + 2;
                    text_start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    if text_start < body.len() {
        segments.push(Segment::Text(body[text_start..].to_string()));
    }
    segments
}

/// Every `[[link]]` in the body, in order.
pub fn extract_links(body: &str) -> Vec<Link> {
    segment_body(body)
        .into_iter()
        .filter_map(|s| match s {
            Segment::Link { text, target_slug } => Some(Link { text, target_slug }),
            Segment::Text(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_text_and_links_in_order() {
        let segs = segment_body("See [[Vitamin D & Sleep]] today.");
        assert_eq!(
            segs,
            vec![
                Segment::Text("See ".into()),
                Segment::Link {
                    text: "Vitamin D & Sleep".into(),
                    target_slug: "vitamin-d-sleep".into()
                },
                Segment::Text(" today.".into()),
            ]
        );
    }

    #[test]
    fn body_with_no_links_is_one_text_segment() {
        assert_eq!(
            segment_body("plain body"),
            vec![Segment::Text("plain body".into())]
        );
    }

    #[test]
    fn adjacent_links_have_no_empty_text_between() {
        let segs = segment_body("[[Alpha]][[Beta]]");
        assert_eq!(
            segs,
            vec![
                Segment::Link {
                    text: "Alpha".into(),
                    target_slug: "alpha".into()
                },
                Segment::Link {
                    text: "Beta".into(),
                    target_slug: "beta".into()
                },
            ]
        );
    }

    #[test]
    fn unbalanced_and_empty_brackets_stay_text() {
        // No closing ]], empty inner, whitespace-only inner -> all plain text, no panic.
        assert_eq!(
            segment_body("a [[unclosed and [[]] and [[   ]] b"),
            vec![Segment::Text("a [[unclosed and [[]] and [[   ]] b".into())]
        );
    }

    #[test]
    fn extract_links_returns_only_links() {
        let links = extract_links("x [[Alpha]] y [[Beta Gamma]] z");
        assert_eq!(
            links,
            vec![
                Link {
                    text: "Alpha".into(),
                    target_slug: "alpha".into()
                },
                Link {
                    text: "Beta Gamma".into(),
                    target_slug: "beta-gamma".into()
                },
            ]
        );
    }

    #[test]
    fn trims_inner_whitespace_for_slug_and_text() {
        let segs = segment_body("[[  Spaced Title  ]]");
        assert_eq!(
            segs,
            vec![Segment::Link {
                text: "Spaced Title".into(),
                target_slug: "spaced-title".into()
            }]
        );
    }
}
