use std::collections::HashMap;

use rmcp::model::Tool;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolFilter {
    /// If set, only these tools are allowed (whitelist mode).
    /// If None, all tools are allowed unless in `deny`.
    #[serde(default, rename = "allow")]
    allow_list: Option<Vec<String>>,

    /// Tools to explicitly block (blacklist mode, only when allow is None).
    #[serde(default, rename = "deny")]
    deny_list: Vec<String>,

    /// Rename map: original_name -> exposed_name.
    #[serde(default, rename = "rename")]
    rename_map: HashMap<String, String>,

    /// Redescripte map: original_name -> new description.
    #[serde(default, rename = "redescripte")]
    redescripte_map: HashMap<String, String>,
}

impl ToolFilter {
    pub fn allow_all() -> Self {
        Self::default()
    }

    pub fn allowed(names: Vec<impl Into<String>>) -> Self {
        Self {
            allow_list: Some(names.into_iter().map(Into::into).collect()),
            deny_list: Vec::new(),
            rename_map: HashMap::new(),
            redescripte_map: HashMap::new(),
        }
    }

    pub fn denied(names: Vec<impl Into<String>>) -> Self {
        Self {
            allow_list: None,
            deny_list: names.into_iter().map(Into::into).collect(),
            rename_map: HashMap::new(),
            redescripte_map: HashMap::new(),
        }
    }

    pub fn with_rename(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.rename_map.insert(from.into(), to.into());
        self
    }

    /// Override the description of a tool.
    pub fn with_redescripte(mut self, tool: impl Into<String>, desc: impl Into<String>) -> Self {
        self.redescripte_map.insert(tool.into(), desc.into());
        self
    }

    pub(crate) fn apply(&self, tools: Vec<Tool>) -> Vec<Tool> {
        tools
            .into_iter()
            .filter(|t| self.is_tool_visible(t.name.as_ref()))
            .map(|mut t| {
                if let Some(exposed) = self.rename_map.get(t.name.as_ref()) {
                    t.name = exposed.clone().into();
                }
                if let Some(new_desc) = self.redescripte_map.get(t.name.as_ref()) {
                    t.description = Some(new_desc.clone().into());
                }
                t
            })
            .collect()
    }

    pub(crate) fn is_tool_visible(&self, original_name: &str) -> bool {
        match &self.allow_list {
            Some(allowed) => allowed.iter().any(|n| n == original_name),
            None => !self.deny_list.iter().any(|n| n == original_name),
        }
    }
}

impl Default for ToolFilter {
    fn default() -> Self {
        Self {
            allow_list: None,
            deny_list: Vec::new(),
            rename_map: HashMap::new(),
            redescripte_map: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;

    fn make_tool(name: &str) -> Tool {
        Tool::new(name.to_owned(), "", serde_json::Map::new())
    }

    #[test]
    fn test_allow_all() {
        let filter = ToolFilter::allow_all();
        let tools = vec![make_tool("a"), make_tool("b")];
        let result = filter.apply(tools);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_allowed() {
        let filter = ToolFilter::allowed(vec!["a", "c"]);
        let tools = vec![make_tool("a"), make_tool("b"), make_tool("c")];
        let result = filter.apply(tools);
        let names: Vec<_> = result.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, vec!["a", "c"]);
    }

    #[test]
    fn test_denied() {
        let filter = ToolFilter::denied(vec!["a"]);
        let tools = vec![make_tool("a"), make_tool("b"), make_tool("c")];
        let result = filter.apply(tools);
        let names: Vec<_> = result.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, vec!["a", "c"]);
    }

    #[test]
    fn test_rename() {
        let filter = ToolFilter::allow_all().with_rename("search", "web_search");
        let tools = vec![make_tool("search"), make_tool("read")];
        let result = filter.apply(tools);
        let names: Vec<_> = result.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, vec!["web_search", "read"]);
    }

    #[test]
    fn test_allowed_with_rename() {
        let filter =
            ToolFilter::allowed(vec!["search", "read"]).with_rename("search", "web_search");
        let tools = vec![make_tool("search"), make_tool("read"), make_tool("delete")];
        let result = filter.apply(tools);
        let names: Vec<_> = result.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, vec!["web_search", "read"]);
    }

    #[test]
    fn test_is_tool_visible() {
        let filter = ToolFilter::allowed(vec!["a", "b"]);
        assert!(filter.is_tool_visible("a"));
        assert!(filter.is_tool_visible("b"));
        assert!(!filter.is_tool_visible("c"));

        let filter = ToolFilter::denied(vec!["a"]);
        assert!(!filter.is_tool_visible("a"));
        assert!(filter.is_tool_visible("b"));
    }

    #[test]
    fn test_redescripte() {
        let filter = ToolFilter::allow_all().with_redescripte("a", "new description for a");
        let tools = vec![make_tool("a"), make_tool("b")];
        let result = filter.apply(tools);
        assert_eq!(
            result[0].description.as_ref().map(|d| d.as_ref()),
            Some("new description for a")
        );
        assert_eq!(result[1].description.as_ref().map(|d| d.as_ref()), Some(""));
    }
}
