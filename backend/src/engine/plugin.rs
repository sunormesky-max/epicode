use std::collections::HashMap;
use std::sync::Arc;

/// A plugin provides custom tools, skills, or memory enhancers.
pub trait Plugin: Send + Sync + std::any::Any {
    /// Unique plugin identifier.
    fn id(&self) -> &str;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Plugin version (semver).
    fn version(&self) -> &str;

    /// Tool definitions this plugin exposes.
    fn tool_definitions(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// Execute a tool by name with JSON arguments.
    fn execute_tool(&self, name: &str, args: &serde_json::Value) -> Result<String, String> {
        let _ = (name, args);
        Err("plugin does not implement tools".to_string())
    }

    /// Called when the plugin is loaded.
    fn on_load(&self) {}

    /// Called when the plugin is unloaded.
    fn on_unload(&self) {}
}

/// Metadata for a registered plugin.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loaded_at: i64,
    pub tool_count: usize,
}

/// Registry for dynamic plugins.
pub struct PluginRegistry {
    plugins: parking_lot::RwLock<HashMap<String, Arc<dyn Plugin>>>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Register a plugin. Fails if ID already exists.
    pub fn register(&self, plugin: Arc<dyn Plugin>) -> Result<(), String> {
        let id = plugin.id().to_string();
        let mut plugins = self.plugins.write();
        if plugins.contains_key(&id) {
            return Err(format!("plugin '{}' already registered", id));
        }
        plugin.on_load();
        plugins.insert(id, plugin);
        Ok(())
    }

    /// Unregister a plugin by ID.
    pub fn unregister(&self, id: &str) -> bool {
        let mut plugins = self.plugins.write();
        if let Some(p) = plugins.remove(id) {
            p.on_unload();
            true
        } else {
            false
        }
    }

    /// Get a plugin by ID.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.read().get(id).cloned()
    }

    /// List all registered plugin metadata.
    pub fn list_meta(&self) -> Vec<PluginMeta> {
        let plugins = self.plugins.read();
        plugins
            .values()
            .map(|p| PluginMeta {
                id: p.id().to_string(),
                name: p.name().to_string(),
                version: p.version().to_string(),
                loaded_at: chrono::Utc::now().timestamp(),
                tool_count: p.tool_definitions().len(),
            })
            .collect()
    }

    /// Aggregate tool definitions from all plugins.
    pub fn all_tool_definitions(&self) -> Vec<serde_json::Value> {
        let plugins = self.plugins.read();
        let mut defs = Vec::new();
        for p in plugins.values() {
            defs.extend(p.tool_definitions());
        }
        defs
    }

    /// Execute a tool across all plugins (first match wins).
    pub fn execute_tool(&self, name: &str, args: &serde_json::Value) -> Result<String, String> {
        let plugins = self.plugins.read();
        for p in plugins.values() {
            for def in p.tool_definitions() {
                if let Some(func) = def.get("function") {
                    if let Some(func_name) = func.get("name").and_then(|n| n.as_str()) {
                        if func_name == name {
                            return p.execute_tool(name, args);
                        }
                    }
                }
            }
        }
        Err(format!("tool '{}' not found in any plugin", name))
    }

    /// Check if a plugin is registered.
    pub fn has(&self, id: &str) -> bool {
        self.plugins.read().contains_key(id)
    }

    /// Number of registered plugins.
    pub fn count(&self) -> usize {
        self.plugins.read().len()
    }
}

/// A built-in plugin that wraps the existing hard-coded tools so they can
/// participate in the plugin registry.
pub struct BuiltinToolPlugin;

impl Plugin for BuiltinToolPlugin {
    fn id(&self) -> &str {
        "epicode.builtin.tools"
    }

    fn name(&self) -> &str {
        "Epicode Built-in Tools"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        id: String,
    }

    impl Plugin for TestPlugin {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            "Test Plugin"
        }

        fn version(&self) -> &str {
            "0.1.0"
        }

        fn tool_definitions(&self) -> Vec<serde_json::Value> {
            vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "test_echo",
                    "description": "Echo back the input",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "message": {"type": "string"}
                        },
                        "required": ["message"]
                    }
                }
            })]
        }

        fn execute_tool(&self, name: &str, args: &serde_json::Value) -> Result<String, String> {
            if name == "test_echo" {
                let msg = args["message"].as_str().unwrap_or("?");
                Ok(format!("echo: {}", msg))
            } else {
                Err("unknown".to_string())
            }
        }
    }

    #[test]
    fn register_and_list() {
        let reg = PluginRegistry::new();
        let p = Arc::new(TestPlugin { id: "test.a".to_string() });
        assert!(reg.register(p).is_ok());
        assert_eq!(reg.count(), 1);

        let meta = reg.list_meta();
        assert_eq!(meta.len(), 1);
        assert_eq!(meta[0].id, "test.a");
    }

    #[test]
    fn duplicate_registration_fails() {
        let reg = PluginRegistry::new();
        let p1 = Arc::new(TestPlugin { id: "dup".to_string() });
        let p2 = Arc::new(TestPlugin { id: "dup".to_string() });
        assert!(reg.register(p1).is_ok());
        assert!(reg.register(p2).is_err());
    }

    #[test]
    fn unregister_removes_plugin() {
        let reg = PluginRegistry::new();
        let p = Arc::new(TestPlugin { id: "remove.me".to_string() });
        reg.register(p).unwrap();
        assert!(reg.unregister("remove.me"));
        assert!(!reg.has("remove.me"));
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn execute_tool_across_plugins() {
        let reg = PluginRegistry::new();
        let p = Arc::new(TestPlugin { id: "test.exec".to_string() });
        reg.register(p).unwrap();

        let result = reg.execute_tool("test_echo", &serde_json::json!({"message": "hello"}));
        assert_eq!(result.unwrap(), "echo: hello");
    }

    #[test]
    fn tool_definitions_aggregated() {
        let reg = PluginRegistry::new();
        let p = Arc::new(TestPlugin { id: "test.defs".to_string() });
        reg.register(p).unwrap();

        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["function"]["name"], "test_echo");
    }

    #[test]
    fn unknown_tool_returns_error() {
        let reg = PluginRegistry::new();
        let result = reg.execute_tool("nonexistent", &serde_json::json!({}));
        assert!(result.is_err());
    }
}
