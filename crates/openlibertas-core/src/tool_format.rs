use serde::{Deserialize, Serialize};

/// Controls how tool calls are sent to and received from a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ToolFormat {
    /// OpenAI/Anthropic native `tool_calls` array in the API.
    #[default]
    Native,
    /// JSON objects or arrays embedded in `message.content` (Ollama Qwen style).
    ContentJson,
    /// `<tool_call>` XML tags in `message.content` (llama.cpp Qwen3.5 style).
    Xml,
    /// No tool support.
    None,
}

impl ToolFormat {
    /// Returns `true` if this format sends the native `tools` array in API requests.
    pub fn sends_native_tools(self) -> bool {
        self == ToolFormat::Native
    }

    /// Returns `true` if the backend expects tool calls in the native API format.
    pub fn expects_native_format(self) -> bool {
        self == ToolFormat::Native
    }

    /// Returns `true` if the sanitizer needs to parse tool calls from message content.
    pub fn needs_content_parsing(self) -> bool {
        matches!(self, ToolFormat::ContentJson | ToolFormat::Xml)
    }

    /// Returns format-specific instructions to append to the system prompt.
    pub fn tool_instructions_suffix(self) -> &'static str {
        match self {
            ToolFormat::Native => "",
            ToolFormat::ContentJson => {
                "\n\n\
                When invoking tools, output a JSON object or array in the message content \
                with this structure:\n\
                {\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n\
                Do not use the native tool_calls format."
            }
            ToolFormat::Xml => {
                "\n\n\
                When invoking tools, wrap each tool call in XML tags like this:\n\
                <tool_call>\n\
                <name>tool_name</name>\n\
                <arguments>{\"param\": \"value\"}</arguments>\n\
                </tool_call>\n\
                Do not use the native tool_calls format."
            }
            ToolFormat::None => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_native() {
        assert_eq!(ToolFormat::default(), ToolFormat::Native);
    }

    #[test]
    fn sends_native_tools_only_for_native() {
        assert!(ToolFormat::Native.sends_native_tools());
        assert!(!ToolFormat::ContentJson.sends_native_tools());
        assert!(!ToolFormat::Xml.sends_native_tools());
        assert!(!ToolFormat::None.sends_native_tools());
    }

    #[test]
    fn expects_native_format_only_for_native() {
        assert!(ToolFormat::Native.expects_native_format());
        assert!(!ToolFormat::ContentJson.expects_native_format());
        assert!(!ToolFormat::Xml.expects_native_format());
        assert!(!ToolFormat::None.expects_native_format());
    }

    #[test]
    fn needs_content_parsing_for_content_json_and_xml() {
        assert!(!ToolFormat::Native.needs_content_parsing());
        assert!(ToolFormat::ContentJson.needs_content_parsing());
        assert!(ToolFormat::Xml.needs_content_parsing());
        assert!(!ToolFormat::None.needs_content_parsing());
    }

    #[test]
    fn tool_instructions_suffix_is_empty_for_native_and_none() {
        assert_eq!(ToolFormat::Native.tool_instructions_suffix(), "");
        assert_eq!(ToolFormat::None.tool_instructions_suffix(), "");
    }

    #[test]
    fn content_json_suffix_contains_json() {
        let suffix = ToolFormat::ContentJson.tool_instructions_suffix();
        assert!(!suffix.is_empty());
        assert!(suffix.contains("JSON"));
        assert!(suffix.contains("name"));
        assert!(suffix.contains("arguments"));
    }

    #[test]
    fn xml_suffix_contains_xml_tags() {
        let suffix = ToolFormat::Xml.tool_instructions_suffix();
        assert!(!suffix.is_empty());
        assert!(suffix.contains("<tool_call>"));
        assert!(suffix.contains("<name>"));
        assert!(suffix.contains("<arguments>"));
    }

    #[test]
    fn all_variants_roundtrip_through_serde() {
        for variant in [
            ToolFormat::Native,
            ToolFormat::ContentJson,
            ToolFormat::Xml,
            ToolFormat::None,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ToolFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized, "roundtrip failed for {:?}", variant);
        }
    }

    #[test]
    fn serde_deserializes_from_string() {
        assert_eq!(
            serde_json::from_str::<ToolFormat>("\"Native\"").unwrap(),
            ToolFormat::Native
        );
        assert_eq!(
            serde_json::from_str::<ToolFormat>("\"ContentJson\"").unwrap(),
            ToolFormat::ContentJson
        );
        assert_eq!(
            serde_json::from_str::<ToolFormat>("\"Xml\"").unwrap(),
            ToolFormat::Xml
        );
        assert_eq!(
            serde_json::from_str::<ToolFormat>("\"None\"").unwrap(),
            ToolFormat::None
        );
    }
}
