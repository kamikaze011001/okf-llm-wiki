use anyhow::Result;

pub enum Source {
    Url(String),
    Text(String),
}

pub fn classify(input: &str) -> Source {
    let t = input.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        Source::Url(t.to_string())
    } else {
        Source::Text(input.to_string())
    }
}

/// Extract readable text from an HTML document (paragraphs + headings).
pub fn html_to_text(html: &str) -> String {
    use scraper::{Html, Selector};
    let doc = Html::parse_document(html);
    let sel = Selector::parse("h1,h2,h3,p,li").unwrap();
    let mut out = Vec::new();
    for el in doc.select(&sel) {
        let txt: String = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if !txt.is_empty() {
            out.push(txt);
        }
    }
    out.join("\n\n")
}

pub async fn fetch_clean(input: &str) -> Result<String> {
    match classify(input) {
        Source::Text(t) => Ok(t),
        Source::Url(u) => {
            let html = reqwest::get(&u).await?.text().await?;
            Ok(html_to_text(&html))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_url_vs_text() {
        assert!(matches!(classify("https://a.com"), Source::Url(_)));
        assert!(matches!(classify("just a note"), Source::Text(_)));
    }
    #[test]
    fn strips_html_to_readable_text() {
        let html = "<html><body><nav>Home</nav><h1>Title</h1><p>Hello world.</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world."));
        assert!(!text.contains("<p>"));
    }
}
