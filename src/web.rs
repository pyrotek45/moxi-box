//! Optional web access for the agent: keyless search via DuckDuckGo's HTML page and
//! plain-text page fetching. Both are off unless the user enables Web access in Settings.

use serde_json::Value;

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const LITE_SEARCH_URL: &str = "https://lite.duckduckgo.com/lite/";
const BING_SEARCH_URL: &str = "https://www.bing.com/search";
const MAX_RESULTS: usize = 8;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PAGE_CHARS: usize = 12_000;
// Many sites refuse unfamiliar clients outright, so requests look like an ordinary browser.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

pub fn definitions() -> Vec<Value> {
    vec![
        crate::tools::definition(
            "web_search",
            "Search the web. Returns titles, URLs, and snippets. Use fetch_url to read a result.",
            serde_json::json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        ),
        crate::tools::definition(
            "fetch_url",
            "Fetch an http(s) page and return its readable text, capped in length. Some sites block automated readers or need JavaScript; if one fails, try another result. For current weather, fetch https://wttr.in/City+Name?format=4 (plain text, no JavaScript).",
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
    // Search engines sometimes answer automated clients with a bot check instead of results,
    // which used to look like "no results" for every query. Try several before giving up.
    type Parser = fn(&str) -> Vec<(String, String, String)>;
    let engines: [(&str, &str, Parser); 3] = [
        ("DuckDuckGo", SEARCH_URL, parse_search_results),
        ("DuckDuckGo Lite", LITE_SEARCH_URL, parse_lite_results),
        ("Bing", BING_SEARCH_URL, parse_bing_results),
    ];
    let mut problems = Vec::new();
    let mut results = Vec::new();
    for (name, base, parse) in engines {
        let url = reqwest::Url::parse_with_params(base, &[("q", query)])
            .map_err(|error| error.to_string())?;
        match get_text(url).await {
            Ok(html) => {
                results = parse(&html);
                if !results.is_empty() {
                    break;
                }
                problems.push(if looks_like_bot_check(&html) {
                    format!("{name} asked for a bot check")
                } else {
                    format!("{name} returned no results")
                });
            }
            Err(error) => problems.push(format!("{name}: {error}")),
        }
    }
    if results.is_empty() {
        let blocked = problems
            .iter()
            .any(|problem| !problem.ends_with("no results"));
        return if blocked {
            Err(format!("Web search is unavailable right now ({}). This is a problem with the search services, not with your query. Use fetch_url on a site you know (for example official documentation, or https://wttr.in/City+Name?format=4 for weather) or tell the user search is unavailable.", problems.join("; ")))
        } else {
            Ok(format!(
                "No web results for {query:?}. Try different or fewer words."
            ))
        };
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
        "The page has no readable text; it probably needs JavaScript. Try another result."
            .to_owned()
    } else {
        text
    })
}

async fn get_text(url: reqwest::Url) -> Result<String, String> {
    use futures_util::StreamExt;
    let response = reqwest::Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.8",
        )
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.8")
        .send()
        .await
        .map_err(|error| format!("Web request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(blocked_message(status));
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

/// Explain an HTTP failure in terms the model can act on.
fn blocked_message(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 | 451 => format!("The site refused automated access ({status}). Try a different search result; for weather, https://wttr.in/City+Name?format=4 works."),
        429 => format!("The site is rate limiting requests ({status}). Try a different source."),
        404 | 410 => format!("The page was not found ({status}). Check the URL or search again."),
        _ => format!("The site returned {status}. Try a different source."),
    }
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

/// Result pages that ask for a human check instead of listing results.
fn looks_like_bot_check(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    [
        "anomaly",
        "captcha",
        "unusual traffic",
        "are you a robot",
        "challenge-form",
        "bots use",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// (title, url, snippet) from DuckDuckGo Lite's table layout.
fn parse_lite_results(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    for block in html.split("result-link").skip(1) {
        let Some(tag_start) = block.find('>') else {
            continue;
        };
        // The href sits in the same <a> tag, before or after the class attribute.
        let opening = html_tag_around(html, block);
        let Some(href) = opening.and_then(|tag| quoted_attribute(&tag, "href")) else {
            continue;
        };
        let title = block[tag_start + 1..]
            .split_once("</a>")
            .map_or(String::new(), |(title, _)| html_to_text(title));
        let snippet = block
            .split_once("result-snippet")
            .and_then(|(_, rest)| rest.split_once('>'))
            .and_then(|(_, rest)| rest.split_once("</td>"))
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

/// The full opening tag that contains the start of `block` (a slice of `html`).
fn html_tag_around(html: &str, block: &str) -> Option<String> {
    let offset = block.as_ptr() as usize - html.as_ptr() as usize;
    let start = html[..offset].rfind('<')?;
    let end = offset + block.find('>')?;
    Some(html[start..end].to_owned())
}

/// An attribute value in either quote style.
fn quoted_attribute(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let marker = format!("{name}={quote}");
        if let Some(start) = tag.find(&marker).map(|index| index + marker.len()) {
            let end = tag[start..].find(quote)? + start;
            return Some(tag[start..end].to_owned());
        }
    }
    None
}

/// (title, url, snippet) from Bing's result list.
fn parse_bing_results(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    for block in html.split("class=\"b_algo\"").skip(1) {
        let Some(heading) = block.split_once("<h2").map(|(_, rest)| rest) else {
            continue;
        };
        let Some(href) = quoted_attribute(heading, "href") else {
            continue;
        };
        let title = heading
            .split_once("<a")
            .and_then(|(_, rest)| rest.split_once('>'))
            .and_then(|(_, rest)| rest.split_once("</a>"))
            .map_or(String::new(), |(title, _)| html_to_text(title));
        let snippet = heading
            .split_once("<p")
            .and_then(|(_, rest)| rest.split_once('>'))
            .and_then(|(_, rest)| rest.split_once("</p>"))
            .map_or(String::new(), |(snippet, _)| html_to_text(snippet));
        let url = bing_url(&decode_entities(&href));
        if url.starts_with("http") && !title.is_empty() {
            results.push((title, url, snippet));
        }
        if results.len() >= MAX_RESULTS {
            break;
        }
    }
    results
}

/// Bing sometimes wraps links as /ck/a?...&u=a1<base64url of the address>.
fn bing_url(href: &str) -> String {
    let Ok(url) = reqwest::Url::parse(href) else {
        return href.to_owned();
    };
    if !url.path().starts_with("/ck/") {
        return href.to_owned();
    }
    url.query_pairs()
        .find(|(key, _)| key == "u")
        .and_then(|(_, value)| value.strip_prefix("a1").map(base64url_decode))
        .flatten()
        .unwrap_or_else(|| href.to_owned())
}

fn base64url_decode(text: &str) -> Option<String> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut bytes = Vec::new();
    for c in text.bytes().take_while(|c| *c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    String::from_utf8(bytes).ok()
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
    fn parses_fallback_engines_and_detects_bot_checks() {
        let lite = r#"<table><tr><td><a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.python.org%2F" class='result-link'>Welcome to <b>Python</b>.org</a></td></tr>
            <tr><td class='result-snippet'>The official home of the Python Programming Language.</td></tr>
            <tr><td><a rel="nofollow" href="https://docs.python.org/3/" class='result-link'>Python docs</a></td></tr></table>"#;
        let results = parse_lite_results(lite);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "Welcome to Python.org");
        assert_eq!(results[0].1, "https://www.python.org/");
        assert!(results[0].2.starts_with("The official home"));
        assert_eq!(results[1].1, "https://docs.python.org/3/");

        let bing = r#"<ol><li class="b_algo" data-x="1"><div class="b_tpcn">x</div><h2><a href="https://www.bing.com/ck/a?!&amp;&amp;p=1&amp;u=a1aHR0cHM6Ly93d3cucnVzdC1sYW5nLm9yZy8&amp;ntb=1" h="ID=1">Rust <strong>Programming</strong> Language</a></h2>
            <div class="b_caption"><p class="b_lineclamp2">A language empowering everyone.</p></div></li>
            <li class="b_algo"><h2><a href="https://example.com/">Example</a></h2></li></ol>"#;
        let results = parse_bing_results(bing);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "Rust Programming Language");
        assert_eq!(results[0].1, "https://www.rust-lang.org/");
        assert_eq!(results[0].2, "A language empowering everyone.");
        assert_eq!(results[1].1, "https://example.com/");

        assert!(looks_like_bot_check(
            "<form id=\"challenge-form\">Unfortunately, bots use DuckDuckGo too.</form>"
        ));
        assert!(!looks_like_bot_check("<html>results</html>"));
        assert!(parse_lite_results("<html>nothing</html>").is_empty());
    }

    #[test]
    fn page_text_drops_scripts_and_keeps_blocks() {
        let html =
            "<html><head><title>T</title><style>p{}</style></head><body><script>alert(1)</script>\
            <h1>Title</h1><p>First   para &amp; more</p><div>Second<br>line</div></body></html>";
        assert_eq!(html_to_text(html), "Title\nFirst para & more\nSecond\nline");
    }

    #[test]
    fn blocked_sites_suggest_an_alternative() {
        let message = blocked_message(reqwest::StatusCode::FORBIDDEN);
        assert!(message.contains("refused") && message.contains("different search result"));
        assert!(blocked_message(reqwest::StatusCode::NOT_FOUND).contains("not found"));
    }

    #[test]
    fn web_tools_reject_bad_input_without_network() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(search("  ")).is_err());
        assert!(runtime.block_on(fetch("file:///etc/passwd")).is_err());
        assert!(runtime.block_on(fetch("not a url")).is_err());
    }
}
