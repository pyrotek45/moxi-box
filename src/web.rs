//! Optional web access for the agent: keyless search via DuckDuckGo's HTML page and
//! plain-text page fetching. Both are off unless the user enables Web access in Settings.

use serde_json::Value;

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const MAX_RESULTS: usize = 8;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PAGE_CHARS: usize = 12_000;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) MoxiBox/0.1";

pub fn definitions() -> Vec<Value> {
    vec![
        crate::tools::definition(
            "web_search",
            "Search the web. Returns titles, URLs, and snippets. Use fetch_url to read a result.",
            serde_json::json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        ),
        crate::tools::definition(
            "fetch_url",
            "Fetch an http(s) page and return its readable text, capped in length.",
            serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),
        ),
    ]
}

pub fn is_web_tool(name: &str) -> bool {
    matches!(name, "web_search" | "fetch_url")
}

pub async fn search(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Search query must not be empty.".to_owned());
    }
    let url = reqwest::Url::parse_with_params(SEARCH_URL, &[("q", query)])
        .map_err(|error| error.to_string())?;
    let html = get_text(url).await?;
    let results = parse_search_results(&html);
    if results.is_empty() {
        return Ok(format!("No web results for {query:?}."));
    }
    Ok(results
        .iter()
        .enumerate()
        .map(|(index, (title, url, snippet))| {
            format!("{}. {title}\n   {url}\n   {snippet}", index + 1)
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

pub async fn fetch(url: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(url.trim())
        .map_err(|_| "Use a full http:// or https:// URL.".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http and https URLs can be fetched.".to_owned());
    }
    let html = get_text(url).await?;
    let mut text = html_to_text(&html);
    if text.chars().count() > MAX_PAGE_CHARS {
        text = text.chars().take(MAX_PAGE_CHARS).collect();
        text.push_str("\n[Page truncated.]");
    }
    Ok(if text.trim().is_empty() {
        "The page has no readable text.".to_owned()
    } else {
        text
    })
}

async fn get_text(url: reqwest::Url) -> Result<String, String> {
    use futures_util::StreamExt;
    let response = reqwest::Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("Web request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("The site returned {status}."));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Web request failed: {error}"))?;
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_PAGE_BYTES {
            body.truncate(MAX_PAGE_BYTES);
            break;
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// (title, url, snippet) for each result on DuckDuckGo's HTML results page.
fn parse_search_results(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    for block in html.split("class=\"result__a\"").skip(1) {
        let Some(href) = attribute(block, "href") else {
            continue;
        };
        let title = block
            .split_once('>')
            .and_then(|(_, rest)| rest.split_once("</a>"))
            .map_or(String::new(), |(title, _)| html_to_text(title));
        let snippet = block
            .split_once("class=\"result__snippet\"")
            .and_then(|(_, rest)| rest.split_once('>'))
            .and_then(|(_, rest)| rest.split_once("</a>"))
            .map_or(String::new(), |(snippet, _)| html_to_text(snippet));
        let url = result_url(&decode_entities(&href));
        if !url.is_empty() && !title.is_empty() {
            results.push((title, url, snippet));
        }
        if results.len() >= MAX_RESULTS {
            break;
        }
    }
    results
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_owned())
}

/// DuckDuckGo links go through a redirect carrying the real address in `uddg`.
fn result_url(href: &str) -> String {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_owned()
    };
    match reqwest::Url::parse(&absolute) {
        Ok(url) => url
            .query_pairs()
            .find(|(key, _)| key == "uddg")
            .map_or(absolute.clone(), |(_, target)| target.into_owned()),
        Err(_) => String::new(),
    }
}

/// Readable text from HTML: drops scripts, styles, and tags; keeps line breaks at blocks.
fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        rest = &rest[start..];
        let lower = rest.get(..10).unwrap_or(rest).to_ascii_lowercase();
        let skip = ["<script", "<style", "<noscript", "<svg", "<head"]
            .into_iter()
            .find(|tag| lower.starts_with(tag));
        if let Some(tag) = skip {
            let close = format!("</{}", &tag[1..]);
            rest = match rest.to_ascii_lowercase().find(&close) {
                Some(end) => rest[end..].split_once('>').map_or("", |(_, after)| after),
                None => "",
            };
            continue;
        }
        let Some(end) = rest.find('>') else {
            break;
        };
        let tag = rest[1..end].trim_start_matches('/').to_ascii_lowercase();
        if [
            "p", "br", "div", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "section", "article",
        ]
        .iter()
        .any(|block| tag == *block || tag.starts_with(&format!("{block} ")))
        {
            text.push('\n');
        }
        rest = &rest[end + 1..];
    }
    text.push_str(rest);
    decode_entities(&text)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duckduckgo_results_and_unwraps_redirects() {
        let html = r#"<div class="result"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org%2Fbook%2F&amp;rut=x">The Rust <b>Book</b></a>
            <a class="result__snippet" href="x">Learn Rust &amp; have fun.</a></div>
            <div class="result"><a class="result__a" href="https://example.com/">Example</a></div>"#;
        let results = parse_search_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "The Rust Book");
        assert_eq!(results[0].1, "https://doc.rust-lang.org/book/");
        assert_eq!(results[0].2, "Learn Rust & have fun.");
        assert_eq!(results[1].1, "https://example.com/");
        assert!(parse_search_results("<html>no results</html>").is_empty());
    }

    #[test]
    fn page_text_drops_scripts_and_keeps_blocks() {
        let html =
            "<html><head><title>T</title><style>p{}</style></head><body><script>alert(1)</script>\
            <h1>Title</h1><p>First   para &amp; more</p><div>Second<br>line</div></body></html>";
        assert_eq!(html_to_text(html), "Title\nFirst para & more\nSecond\nline");
    }

    #[test]
    fn web_tools_reject_bad_input_without_network() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(search("  ")).is_err());
        assert!(runtime.block_on(fetch("file:///etc/passwd")).is_err());
        assert!(runtime.block_on(fetch("not a url")).is_err());
    }
}
