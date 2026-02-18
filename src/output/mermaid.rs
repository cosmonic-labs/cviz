use crate::get_chain_for;
use crate::model::{short_interface_name, CompositionGraph, SYNTHETIC_COMPONENT};
use crate::output::{DetailLevel, Direction};

/// Generate a Mermaid diagram from the composition graph
pub fn generate_mermaid(graph: &CompositionGraph, detail: DetailLevel, direction: Direction) -> String {
    match detail {
        DetailLevel::HandlerChain => generate_handler_chain(graph, direction),
        DetailLevel::AllInterfaces => generate_all_interfaces(graph, direction),
        DetailLevel::Full => generate_full(graph, direction),
    }
}

/// Generate a diagram showing only the HTTP handler chain (request flow direction)
fn generate_handler_chain(graph: &CompositionGraph, direction: Direction) -> String {
    let mut output = String::new();
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    let chain = get_chain_for(graph, "wasi:http/handler");

    if chain.is_empty() {
        output.push_str("    empty[\"No HTTP handler chain found\"]\n");
        return output;
    }

    // Add subgraph for the handler chain
    output.push_str("    subgraph composition[\"Handler Chain\"]\n");

    for &idx in &chain {
        if let Some(node) = graph.get_node(idx) {
            let id = sanitize_for_mermaid(&node.name);
            let label = node.display_label();
            output.push_str(&format!("        {}[\"{}\"]\n", id, label));
        }
    }

    output.push_str("    end\n\n");

    // Add export entry point
    if let Some(&first_idx) = chain.first() {
        if let Some(first_node) = graph.get_node(first_idx) {
            output.push_str(&format!(
                "    export([\"Export: handler\"]) --> {}\n",
                sanitize_for_mermaid(&first_node.name)
            ));
        }
    }

    // Add connections between chain elements in request flow order
    for window in chain.windows(2) {
        if let [from_idx, to_idx] = window {
            if let (Some(from_node), Some(to_node)) = (graph.get_node(*from_idx), graph.get_node(*to_idx)) {
                output.push_str(&format!(
                    "    {} -->|\"handler\"| {}\n",
                    sanitize_for_mermaid(&from_node.name),
                    sanitize_for_mermaid(&to_node.name)
                ));
            }
        }
    }

    output
}

/// Generate a diagram showing all interface connections
fn generate_all_interfaces(graph: &CompositionGraph, direction: Direction) -> String {
    let mut output = String::new();
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    let component_nodes = graph.real_nodes();

    if component_nodes.is_empty() {
        output.push_str("    empty[\"No component instances found\"]\n");
        return output;
    }

    let host_interfaces = graph.host_interfaces();

    // Add host imports subgraph
    if !host_interfaces.is_empty() {
        output.push_str("    subgraph host[\"Host Imports\"]\n");
        for interface in &host_interfaces {
            let id = sanitize_for_mermaid(interface);
            let label = short_interface_name(interface);
            output.push_str(&format!("        {}{{\"{}\"}}\n", id, label));
        }
        output.push_str("    end\n\n");
    }

    // Add component instances subgraph
    output.push_str("    subgraph composition[\"Component Instances\"]\n");
    for node in &component_nodes {
        let id = sanitize_for_mermaid(&node.name);
        let label = node.display_label();
        output.push_str(&format!("        {}[\"{}\"]\n", id, label));
    }
    output.push_str("    end\n\n");

    // Add connections
    for node in &component_nodes {
        for import in &node.imports {
            if import.is_host_import {
                let host_id = sanitize_for_mermaid(&import.interface_name);
                let label = import.short_label();
                output.push_str(&format!(
                    "    {} -.->|\"{}\"| {}\n",
                    host_id,
                    label,
                    sanitize_for_mermaid(&node.name)
                ));
            } else if let Some(source_idx) = import.source_instance {
                if let Some(source_node) = graph.get_node(source_idx) {
                    if source_node.component_index != SYNTHETIC_COMPONENT {
                        let label = import.short_label();
                        output.push_str(&format!(
                            "    {} -->|\"{}\"| {}\n",
                            sanitize_for_mermaid(&source_node.name),
                            label,
                            sanitize_for_mermaid(&node.name)
                        ));
                    }
                }
            }
        }
    }

    // Add exports
    output.push('\n');
    for (export_name, source_idx) in &graph.component_exports {
        if let Some(node) = graph.get_node(*source_idx) {
            if node.component_index != SYNTHETIC_COMPONENT {
                let label = short_interface_name(export_name);
                output.push_str(&format!(
                    "    {} --> export_{}([\"Export: {}\"])\n",
                    sanitize_for_mermaid(&node.name),
                    sanitize_for_mermaid(export_name),
                    label
                ));
            }
        }
    }

    output
}

/// Generate a full diagram with all details
fn generate_full(graph: &CompositionGraph, direction: Direction) -> String {
    let mut output = String::new();
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    // Show all nodes including synthetic ones
    output.push_str("    subgraph all[\"All Instances\"]\n");
    for node in graph.nodes.values() {
        let id = sanitize_for_mermaid(&node.name);
        let label = if node.component_index == SYNTHETIC_COMPONENT {
            format!("{} (synthetic)", node.display_label())
        } else {
            format!("{} [comp:{}]", node.display_label(), node.component_index)
        };
        output.push_str(&format!("        {}[\"{}\"]\n", id, label));
    }
    output.push_str("    end\n\n");

    // Show all connections with full interface names
    for node in graph.nodes.values() {
        for import in &node.imports {
            if let Some(source_idx) = import.source_instance {
                if let Some(source_node) = graph.get_node(source_idx) {
                    output.push_str(&format!(
                        "    {} -->|\"{}\"| {}\n",
                        sanitize_for_mermaid(&source_node.name),
                        import.interface_name,
                        sanitize_for_mermaid(&node.name)
                    ));
                }
            }
        }
    }

    // Show all exports
    output.push('\n');
    for (export_name, source_idx) in &graph.component_exports {
        if let Some(node) = graph.get_node(*source_idx) {
            output.push_str(&format!(
                "    {} --> export_{}([\"Export: {}\"])\n",
                sanitize_for_mermaid(&node.name),
                sanitize_for_mermaid(export_name),
                export_name
            ));
        }
    }

    output
}

/// Sanitize a string for use as a Mermaid node ID
fn sanitize_for_mermaid(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_start_matches('_')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComponentNode, InterfaceConnection};
    use crate::output::Direction;

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
    fn test_handler_chain_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::HandlerChain, Direction::LeftToRight);

        assert!(
            output.starts_with("graph LR\n"),
            "should start with graph direction"
        );
        assert!(output.contains("subgraph composition"), "should have subgraph");
        assert!(output.contains("Handler Chain"), "should have Handler Chain title");
        assert!(output.contains("srv"), "should show srv node");
        assert!(output.contains("middleware"), "should show middleware node");
        assert!(output.contains("-->|\"handler\"|"), "should have handler edge");
        // Export should point to the first (outermost) node
        assert!(
            output.contains("export([\"Export: handler\"]) --> middleware"),
            "export should point to outermost handler, got:\n{}",
            output
        );
    }

    #[test]
    fn test_all_interfaces_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::AllInterfaces, Direction::LeftToRight);

        assert!(output.starts_with("graph LR\n"));
        // Host imports subgraph
        assert!(output.contains("subgraph host"), "should have host subgraph");
        assert!(output.contains("handler"), "should show handler host import");
        assert!(output.contains("log"), "should show log host import");
        // Component instances subgraph
        assert!(output.contains("subgraph composition"), "should have composition subgraph");
        // Connections
        assert!(output.contains("-.->"), "should have dashed host import edges");
        assert!(output.contains("-->|"), "should have solid instance edges");
        // Export
        assert!(output.contains("Export"), "should have export");
    }

    #[test]
    fn test_full_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::TopDown);

        assert!(output.starts_with("graph TD\n"), "should use TD direction");
        assert!(output.contains("subgraph all"), "should have all-instances subgraph");
        // Full mode uses full interface names for connections where source exists
        assert!(output.contains("wasi:http/handler@0.3.0"), "should show full interface name");
    }

    #[test]
    fn test_empty_graph_mermaid() {
        let graph = CompositionGraph::new();

        let chain = generate_mermaid(&graph, DetailLevel::HandlerChain, Direction::LeftToRight);
        assert!(chain.contains("No HTTP handler chain found"));

        let all = generate_mermaid(&graph, DetailLevel::AllInterfaces, Direction::LeftToRight);
        assert!(all.contains("No component instances found"));
    }

    #[test]
    fn test_sanitize_for_mermaid() {
        assert_eq!(sanitize_for_mermaid("$srv"), "srv");
        assert_eq!(sanitize_for_mermaid("mdl-a"), "mdl_a");
        assert_eq!(sanitize_for_mermaid("instance_0"), "instance_0");
    }
}

