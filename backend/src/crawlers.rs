use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

const NO_INDEX: &str = "noindex, nofollow, noarchive, nosnippet, noimageindex, notranslate";
const ROBOTS_TXT: &str = "User-agent: *\nDisallow: /\n";

/// Lower-case fragments of the User-Agent header of known crawlers, bots, scrapers,
/// AI/SEO fetchers, link-preview fetchers, monitors, scanners and HTTP libraries.
const BOT_MARKERS: &[&str] = &[
    // generic markers; "bot" also covers Googlebot, bingbot, Applebot, GPTBot, ...
    "bot",
    "crawl",
    "spider",
    "scrape",
    "fetch",
    "slurp",
    "archiver",
    "preview",
    "monitor",
    "checker",
    "scanner",
    "validator",
    "headless",
    // search engines
    "google",
    "bingpreview",
    "msnbot",
    "yandex",
    "baidu",
    "duckduck",
    "sogou",
    "exabot",
    "seznam",
    "naver",
    "yeti",
    "qwant",
    "ecosia",
    "mojeek",
    "petalsearch",
    "coccoc",
    // SEO / marketing / archive
    "ahrefs",
    "semrush",
    "mj12",
    "dotbot",
    "rogerbot",
    "screaming frog",
    "sistrix",
    "blexbot",
    "ia_archiver",
    "archive.org",
    "wayback",
    "heritrix",
    // AI and data collection
    "gpt",
    "openai",
    "claude",
    "anthropic",
    "perplexity",
    "ccbot",
    "bytespider",
    "cohere",
    "diffbot",
    "omgili",
    "imagesift",
    "youbot",
    "meta-external",
    "facebookexternalhit",
    // social / chat link previews
    "facebook",
    "twitter",
    "linkedin",
    "pinterest",
    "slack",
    "discord",
    "telegram",
    "whatsapp",
    "skype",
    "embedly",
    "quora",
    "vkshare",
    "redditbot",
    // monitoring and testing
    "uptime",
    "pingdom",
    "statuscake",
    "newrelic",
    "datadog",
    "lighthouse",
    "gtmetrix",
    "pagespeed",
    "chrome-lighthouse",
    // automation tools and HTTP libraries
    "curl",
    "wget",
    "libwww",
    "python",
    "urllib",
    "aiohttp",
    "httpx",
    "go-http-client",
    "java/",
    "apache-httpclient",
    "okhttp-bot",
    "node-fetch",
    "axios",
    "undici",
    "got (",
    "scrapy",
    "phantomjs",
    "selenium",
    "puppeteer",
    "playwright",
    "webdriver",
    "postman",
    "insomnia",
    "restsharp",
    "guzzle",
    "libcurl",
    "perl",
    "ruby",
    "php/",
    // vulnerability scanners
    "nmap",
    "nikto",
    "masscan",
    "zgrab",
    "sqlmap",
    "nuclei",
    "censys",
    "shodan",
    "expanse",
];

pub fn is_bot(user_agent: Option<&str>) -> bool {
    let Some(agent) = user_agent.map(str::trim).filter(|agent| !agent.is_empty()) else {
        return true;
    };
    let agent = agent.to_lowercase();
    BOT_MARKERS.iter().any(|marker| agent.contains(marker))
}

fn blank_page() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!doctype html><html><head><title></title></head><body></body></html>",
    )
        .into_response()
}

/// Answers `robots.txt` for everyone, serves bots an empty page, and marks every
/// response as not to be indexed.
pub async fn filter(request: Request, next: Next) -> Response {
    let mut response = if request.uri().path() == "/robots.txt" {
        ([(header::CONTENT_TYPE, "text/plain")], ROBOTS_TXT).into_response()
    } else {
        let agent = request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok());
        if is_bot(agent) {
            blank_page()
        } else {
            next.run(request).await
        }
    };
    let headers = response.headers_mut();
    headers.insert("x-robots-tag", HeaderValue::from_static(NO_INDEX));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bots_but_not_browsers() {
        for bot in [
            "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
            "Mozilla/5.0 (compatible; bingbot/2.0)",
            "Mozilla/5.0 AppleWebKit/537.36 (compatible; GPTBot/1.0)",
            "curl/8.5.0",
            "python-requests/2.31",
            "Mozilla/5.0 (X11; Linux x86_64) HeadlessChrome/120.0",
            "facebookexternalhit/1.1",
            "",
        ] {
            assert!(is_bot(Some(bot)), "{bot}");
        }
        assert!(is_bot(None));
        for browser in [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
            "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
        ] {
            assert!(!is_bot(Some(browser)), "{browser}");
        }
    }
}
