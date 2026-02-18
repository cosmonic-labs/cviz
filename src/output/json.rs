use serde::{Deserialize, Serialize};
use crate::model::CompositionGraph;

/// Generate JSON from the composition graph
pub fn generate_json(graph: &CompositionGraph, pretty: bool) -> Result<String, serde_json::Error> {
    let model = generate_json_model(graph);
    if pretty {
        serde_json::to_string_pretty(&model)
    } else {
        serde_json::to_string(&model)
    }
}

fn generate_json_model(graph: &CompositionGraph) -> JsonCompositionGraph {
    let nodes = graph.nodes.iter().map(|(id, node)| {
        JsonNode {
            id: *id,
            name: node.display_label().to_string(),
            component_index: node.component_index,
            imports: node.imports.iter().map(|conn| {
                JsonInterfaceConnection {
                    interface: conn.interface_name.clone(),
                    short: conn.short_label(),
                    source_instance: conn.source_instance,
                    is_host_import: conn.is_host_import,
                }
            }).collect(),
        }
    }).collect();

    let exports = graph.component_exports.iter().map(|(iface, src)| {
        JsonExport {
            interface: iface.clone(),
            source_instance: *src,
        }
    }).collect();

    JsonCompositionGraph { version: 1, nodes, exports }
}

#[derive(Deserialize, Serialize)]
pub struct JsonCompositionGraph {
    pub version: u32,
    pub nodes: Vec<JsonNode>,
    pub exports: Vec<JsonExport>,
}

#[derive(Deserialize, Serialize)]
pub struct JsonNode {
    pub id: u32,
    pub name: String,
    pub component_index: u32,
    pub imports: Vec<JsonInterfaceConnection>,
}

#[derive(Deserialize, Serialize)]
pub struct JsonInterfaceConnection {
    pub interface: String,
    pub short: String,
    pub source_instance: Option<u32>,
    pub is_host_import: bool,
}

#[derive(Deserialize, Serialize)]
pub struct JsonExport {
    pub interface: String,
    pub source_instance: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComponentNode, InterfaceConnection};

    /// Build a graph: host → $srv → $middleware → export(handler)
    fn test_graph() -> CompositionGraph {
        let mut graph = CompositionGraph::new();

        let mut srv = ComponentNode::new("$srv".to_string(), 0);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: Some(0),
            is_host_import: true,
        });
        graph.add_node(1, srv);

        let mut mw = ComponentNode::new("$middleware".to_string(), 1);
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: Some(1),
            is_host_import: false,
        });
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".to_string(),
            source_instance: Some(0),
            is_host_import: true,
        });
        graph.add_node(2, mw);

        graph.add_export("wasi:http/handler@0.3.0".to_string(), 2);
        graph
    }

    #[test]
    fn test_full_json() {
        let graph = test_graph();
        let output = generate_json(&graph, true).unwrap();
        println!("{output}");

        assert!(output.contains("srv"), "should show srv");
        assert!(output.contains("middleware"), "should show middleware");
        // Full mode shows full interface names
        assert!(output.contains("wasi:http/handler@0.3.0"), "should show full interface name");
    }

    #[test]
    fn test_empty_graph_json() {
        let graph = CompositionGraph::new();

        let full = generate_json(&graph, true).unwrap();
        println!("{full}");
        assert!(full.contains("[]"));
    }
}
