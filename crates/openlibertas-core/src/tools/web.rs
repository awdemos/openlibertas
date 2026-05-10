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
    let rt = tokio::runtime::Runtime::new().map_err(|e| anyhow::anyhow!(e))?;
    rt.block_on(fetch_url_async(args.url))
}

async fn fetch_url_async(url: String) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow::anyhow!("HTTP {} fetching {}", status, url));
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

pub fn web_search_tool() -> crate::tools::BuiltinTool {
    crate::tools::BuiltinTool {
        name: "web_search".to_string(),
        description: "Search the web for information on a given query.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            },
            "required": ["query"]
        }),
        handler: web_search,
    }
}

pub fn fetch_url_tool() -> crate::tools::BuiltinTool {
    crate::tools::BuiltinTool {
        name: "fetch_url".to_string(),
        description: "Fetch the content of a URL and return the page text.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                }
            },
            "required": ["url"]
        }),
        handler: fetch_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_returns_result() {
        let result = web_search(serde_json::json!({"query": "rust programming"})).unwrap();
        assert!(result.contains("rust programming"));
    }

    #[test]
    fn fetch_url_stub_returns_message() {
        let result = fetch_url(serde_json::json!({"url": "https://httpbin.org/get"})).unwrap();
        assert!(!result.is_empty());
    }
}
