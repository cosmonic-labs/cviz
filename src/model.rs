use std::collections::BTreeMap;

/// Sentinel value for synthetic component instances (e.g., export wrappers)
pub const SYNTHETIC_COMPONENT: u32 = u32::MAX;

/// Represents a component instance in the composition
#[derive(Debug, Clone)]
pub struct ComponentNode {
    /// Instance name (e.g., "$srv", "$mdl-a")
    pub name: String,
    /// Which component is being instantiated
    pub component_index: u32,
    /// List of interface connections (what it receives)
    pub imports: Vec<InterfaceConnection>,
}

impl ComponentNode {
    pub fn new(name: String, component_index: u32) -> Self {
        Self {
            name,
            component_index,
            imports: Vec::new(),
        }
    }

    pub fn add_import(&mut self, connection: InterfaceConnection) {
        self.imports.push(connection);
    }

    /// Get a display label for the node
    pub fn display_label(&self) -> &str {
        self.name.trim_start_matches('$')
    }
}

/// Represents wiring between instances
#[derive(Debug, Clone)]
pub struct InterfaceConnection {
    /// e.g., "wasi:http/handler@0.3.0-rc-2026-01-06"
    pub interface_name: String,
    /// Which instance provides this (None if host import)
    pub source_instance: Option<u32>,
    /// Whether this comes from the host
    pub is_host_import: bool,
}

impl InterfaceConnection {
    pub fn from_instance(interface_name: String, source_instance: u32) -> Self {
        Self {
            interface_name,
            source_instance: Some(source_instance),
            is_host_import: false,
        }
    }

    /// Get a short label for the interface (just the interface name without package/version)
    pub fn short_label(&self) -> String {
        short_interface_name(&self.interface_name)
    }
}

/// The complete parsed composition structure
#[derive(Debug, Default)]
pub struct CompositionGraph {
    /// All component instances, keyed by instance index
    pub nodes: BTreeMap<u32, ComponentNode>,
    /// What the composed component exports (interface name -> source instance)
    pub component_exports: BTreeMap<String, u32>,
}

impl CompositionGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, instance_index: u32, node: ComponentNode) {
        self.nodes.insert(instance_index, node);
    }

    pub fn get_node(&self, instance_index: u32) -> Option<&ComponentNode> {
        self.nodes.get(&instance_index)
    }

    pub fn add_export(&mut self, interface_name: String, source_instance: u32) {
        self.component_exports.insert(interface_name, source_instance);
    }

    /// Get all real (non-synthetic) component nodes
    pub fn real_nodes(&self) -> Vec<&ComponentNode> {
        self.nodes
            .values()
            .filter(|n| n.component_index != SYNTHETIC_COMPONENT)
            .collect()
    }

    /// Get sorted list of unique host interface names across all real nodes
    pub fn host_interfaces(&self) -> Vec<String> {
        let mut interfaces: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for node in self.real_nodes() {
            for import in &node.imports {
                if import.is_host_import {
                    interfaces.insert(import.interface_name.clone());
                }
            }
        }
        interfaces.into_iter().collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        for (iface, src) in &self.component_exports {
            if !self.nodes.contains_key(src) {
                return Err(format!(
                    "Export '{}' references unknown instance {}",
                    iface, src
                ));
            }
        }

        for (id, node) in &self.nodes {
            for conn in &node.imports {
                if let Some(src) = conn.source_instance {
                    if !self.nodes.contains_key(&src) {
                        return Err(format!(
                            "Instance {} imports from unknown instance {}",
                            id, src
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

/// Extract a short interface name from a full interface path
/// e.g., "wasi:http/handler@0.3.0-rc-2026-01-06" -> "handler"
pub fn short_interface_name(full_name: &str) -> String {
    if let Some(slash_pos) = full_name.rfind('/') {
        let after_slash = &full_name[slash_pos + 1..];
        if let Some(at_pos) = after_slash.find('@') {
            return after_slash[..at_pos].to_string();
        }
        return after_slash.to_string();
    }
    full_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_short_label() {
        let conn =
            InterfaceConnection::from_instance("wasi:http/handler@0.3.0-rc-2026-01-06".to_string(), 0);
        assert_eq!(conn.short_label(), "handler");

        let conn2 = InterfaceConnection::from_instance("wasi:io/streams@0.2.0".to_string(), 1);
        assert_eq!(conn2.short_label(), "streams");
    }

    #[test]
    fn test_short_interface_name() {
        assert_eq!(short_interface_name("wasi:http/handler@0.3.0"), "handler");
        assert_eq!(short_interface_name("wasi:io/streams@0.2.0"), "streams");
        assert_eq!(short_interface_name("simple"), "simple");
    }
}
