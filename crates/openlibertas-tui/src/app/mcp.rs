use crate::app::App;
use openlibertas_core::mcp::McpTool;

impl App {
    pub fn mcp_server_names(&self) -> Vec<String> {
        self.mcp_server_names_cache.clone()
    }

    pub fn mcp_server_count(&self) -> usize {
        self.mcp_server_names().len()
    }

    pub fn mcp_selected_server_name(&self) -> Option<String> {
        let names = self.mcp_server_names();
        names.get(self.mcp_selected_server).cloned()
    }

    pub fn mcp_tools_for_server(&self, server_name: &str) -> Vec<McpTool> {
        let tool_map = self.engine.tools().tool_server_map();
        self.engine
            .tools()
            .available_tools()
            .iter()
            .filter(|t| {
                tool_map
                    .get(&t.name)
                    .map(|s| s == server_name)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    pub fn mcp_server_prev(&mut self) {
        self.mcp_selected_server = self.mcp_selected_server.saturating_sub(1);
        self.mcp_selected_tool = 0;
        self.mcp_scroll = 0;
    }

    pub fn mcp_server_next(&mut self) {
        let count = self.mcp_server_count();
        if count > 0 {
            self.mcp_selected_server = (self.mcp_selected_server + 1).min(count - 1);
            self.mcp_selected_tool = 0;
            self.mcp_scroll = 0;
        }
    }

    pub fn toggle_mcp_detail(&mut self) {
        self.mcp_show_detail = !self.mcp_show_detail;
    }

    pub fn selected_tool_name(&self) -> Option<String> {
        if let Some(server_name) = self.mcp_selected_server_name() {
            let tools = self.mcp_tools_for_server(&server_name);
            tools.get(self.mcp_selected_tool).map(|t| t.name.clone())
        } else {
            None
        }
    }
}
