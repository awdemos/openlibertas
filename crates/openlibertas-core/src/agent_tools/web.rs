use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
}

#[derive(Debug, Deserialize)]
struct FetchArgs {
    url: String,
}

pub fn web_search(args: Value) -> Result<String> {
    let args: SearchArgs = serde_json::from_value(args)?;
    Ok(format!(
        "Web search for '{}' would be performed here. Configure an MCP server or API key for real search.",
        args.query
    ))
}

pub fn fetch_url(args: Value) -> Result<String> {
    let args: FetchArgs = serde_json::from_value(args)?;

    // If already inside a tokio runtime (e.g. called from async main loop),
    // use block_in_place to avoid "Cannot start a runtime from within a runtime".
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fetch_url_async(args.url))),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new().map_err(|e| anyhow::anyhow!(e))?;
            rt.block_on(fetch_url_async(args.url))
        }
    }
}

async fn fetch_url_async(url: String) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow::anyhow!("HTTP {status} fetching {url}"));
    }
    let text = resp.text().await?;
    let truncated = if text.len() > 10000 {
        format!(
            "{}\n\n[truncated - {} total characters]",
            &text[..10000],
            text.len()
        )
    } else {
        text
    };
    Ok(truncated)
}

pub fn web_search_tool() -> crate::agent_tools::BuiltinTool {
    crate::define_tool!(
        "web_search",
        "Search the web for information on a given query.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            },
            "required": ["query"]
        }),
        web_search
    )
}

pub fn fetch_url_tool() -> crate::agent_tools::BuiltinTool {
    crate::define_tool!(
        "fetch_url",
        "Fetch the content of a URL and return the page text.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                }
            },
            "required": ["url"]
        }),
        fetch_url
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn web_search_returns_result() {
        let result = web_search(serde_json::json!({"query": "rust programming"})).unwrap();
        assert!(result.contains("rust programming"));
    }

    fn start_test_server() -> (std::thread::JoinHandle<()>, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Read the request headers so the client has finished sending.
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = "HTTP/1.0 200 OK\r\nContent-Length: 13\r\n\r\nHello, world!";
            std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
            // Keep the socket open briefly so the client can read the full response.
            std::thread::sleep(std::time::Duration::from_millis(100));
        });
        (handle, port)
    }

    #[test]
    fn fetch_url_stub_returns_message() {
        let (handle, port) = start_test_server();
        let result =
            fetch_url(serde_json::json!({"url": format!("http://127.0.0.1:{}/", port)})).unwrap();
        assert!(result.contains("Hello, world!"));
        handle.join().unwrap();
    }
}
