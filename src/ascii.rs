use crate::model::{short_interface_name, CompositionGraph, SYNTHETIC_COMPONENT};
use crate::output::DetailLevel;

/// Generate an ASCII diagram from the composition graph
pub fn generate_ascii(graph: &CompositionGraph, detail: DetailLevel) -> String {
    match detail {
        DetailLevel::HandlerChain => generate_handler_chain_ascii(graph),
        DetailLevel::AllInterfaces => generate_all_interfaces_ascii(graph),
        DetailLevel::Full => generate_full_ascii(graph),
    }
}

/// Generate ASCII diagram showing only the HTTP handler chain
fn generate_handler_chain_ascii(graph: &CompositionGraph) -> String {
    let chain = graph.get_handler_chain();

    if chain.is_empty() {
        return box_content("Middleware Chain", &["No HTTP handler chain found"]);
    }

    let mut lines = Vec::new();

    // Build the chain lines
    for (i, &idx) in chain.iter().enumerate() {
        if let Some(node) = graph.get_node(idx) {
            let label = node.display_label();

            if i < chain.len() - 1 {
                // Not the last node - show connection to next
                if let Some(next_node) = graph.get_node(chain[i + 1]) {
                    lines.push(format!("{} ──handler──> {}", label, next_node.display_label()));
                }
            } else {
                // Last node - show export
                lines.push(format!("{} ──> [Export: handler]", label));
            }
        }
    }

    box_content("Middleware Chain", &lines)
}

/// Generate ASCII diagram showing all interface connections
fn generate_all_interfaces_ascii(graph: &CompositionGraph) -> String {
    let mut output = String::new();

    let component_nodes = graph.real_nodes();

    if component_nodes.is_empty() {
        return box_content("Component Instances", &["No component instances found"]);
    }

    let host_interfaces = graph.host_interfaces();

    // Host imports section
    if !host_interfaces.is_empty() {
        let host_lines: Vec<String> = host_interfaces
            .iter()
            .map(|i| format!("  {{{}}}", short_interface_name(i)))
            .collect();
        output.push_str(&box_content("Host Imports", &host_lines));
        output.push('\n');
    }

    // Component instances section
    let instance_lines: Vec<String> = component_nodes
        .iter()
        .map(|n| format!("  [{}]", n.display_label()))
        .collect();
    output.push_str(&box_content("Component Instances", &instance_lines));
    output.push('\n');

    // Connections section
    let mut connection_lines = Vec::new();

    // Host imports connections
    for node in &component_nodes {
        for import in &node.imports {
            if import.is_host_import {
                let label = short_interface_name(&import.interface_name);
                connection_lines.push(format!(
                    "  {{{}}} -.{}.- [{}]",
                    label,
                    import.short_label(),
                    node.display_label()
                ));
            } else if let Some(source_idx) = import.source_instance {
                if let Some(source_node) = graph.get_node(source_idx) {
                    if source_node.component_index != SYNTHETIC_COMPONENT {
                        connection_lines.push(format!(
                            "  [{}] ──{}──> [{}]",
                            source_node.display_label(),
                            import.short_label(),
                            node.display_label()
                        ));
                    }
                }
            }
        }
    }

    // Exports
    for (export_name, source_idx) in &graph.component_exports {
        if let Some(node) = graph.get_node(*source_idx) {
            if node.component_index != SYNTHETIC_COMPONENT {
                let label = short_interface_name(export_name);
                connection_lines.push(format!(
                    "  [{}] ──> (Export: {})",
                    node.display_label(),
                    label
                ));
            }
        }
    }

    if !connection_lines.is_empty() {
        output.push_str(&box_content("Connections", &connection_lines));
    }

    output
}

/// Generate a full ASCII diagram with all details
fn generate_full_ascii(graph: &CompositionGraph) -> String {
    let mut output = String::new();

    // All instances section
    let mut instance_lines = Vec::new();
    for (_idx, node) in &graph.nodes {
        let label = if node.component_index == SYNTHETIC_COMPONENT {
            format!("  [{}] (synthetic)", node.display_label())
        } else {
            format!("  [{}] [comp:{}]", node.display_label(), node.component_index)
        };
        instance_lines.push(label);
    }

    if instance_lines.is_empty() {
        instance_lines.push("  No instances found".to_string());
    }

    output.push_str(&box_content("All Instances", &instance_lines));
    output.push('\n');

    // All connections with full interface names
    let mut connection_lines = Vec::new();
    for node in graph.nodes.values() {
        for import in &node.imports {
            if let Some(source_idx) = import.source_instance {
                if let Some(source_node) = graph.get_node(source_idx) {
                    connection_lines.push(format!(
                        "  [{}] ──{}──> [{}]",
                        source_node.display_label(),
                        &import.interface_name,
                        node.display_label()
                    ));
                }
            }
        }
    }

    // All exports with full names
    for (export_name, source_idx) in &graph.component_exports {
        if let Some(node) = graph.get_node(*source_idx) {
            connection_lines.push(format!(
                "  [{}] ──> (Export: {})",
                node.display_label(),
                export_name
            ));
        }
    }

    if !connection_lines.is_empty() {
        output.push_str(&box_content("Connections", &connection_lines));
    }

    output
}

/// Create a box around content with a title
fn box_content(title: &str, lines: &[impl AsRef<str>]) -> String {
    // Calculate the width needed
    let title_width = title.len() + 2; // Add padding around title
    let max_line_width = lines
        .iter()
        .map(|l| l.as_ref().len())
        .max()
        .unwrap_or(0);
    let width = std::cmp::max(title_width, max_line_width) + 4; // Add padding

    let mut output = String::new();

    // Top border
    output.push_str(&format!("┌{}┐\n", "─".repeat(width)));

    // Title line centered
    let title_padding = (width - title.len()) / 2;
    let title_padding_right = width - title.len() - title_padding;
    output.push_str(&format!(
        "│{}{}{}│\n",
        " ".repeat(title_padding),
        title,
        " ".repeat(title_padding_right)
    ));

    // Separator
    output.push_str(&format!("├{}┤\n", "─".repeat(width)));

    // Content lines
    for line in lines {
        let line_str = line.as_ref();
        let padding = width.saturating_sub(line_str.len());
        output.push_str(&format!("│{}{}│\n", line_str, " ".repeat(padding)));
    }

    // Bottom border
    output.push_str(&format!("└{}┘", "─".repeat(width)));

    output
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
    fn test_box_content() {
        let result = box_content("Test", &["line 1", "line 2"]);
        assert!(result.contains("Test"));
        assert!(result.contains("line 1"));
        assert!(result.contains("line 2"));
        assert!(result.contains("┌"));
        assert!(result.contains("└"));
    }

    #[test]
    fn test_handler_chain_ascii() {
        let graph = test_graph();
        let output = generate_ascii(&graph, DetailLevel::HandlerChain);

        assert!(output.contains("Middleware Chain"), "should have title");
        assert!(output.contains("srv"), "should show srv node");
        assert!(output.contains("middleware"), "should show middleware node");
        assert!(output.contains("handler"), "should show handler label");
        assert!(output.contains("Export"), "should show export");
    }

    #[test]
    fn test_all_interfaces_ascii() {
        let graph = test_graph();
        let output = generate_ascii(&graph, DetailLevel::AllInterfaces);

        // Host imports section
        assert!(output.contains("Host Imports"), "should have host imports section");
        assert!(output.contains("handler"), "should show handler interface");
        assert!(output.contains("log"), "should show log interface");

        // Component instances section
        assert!(output.contains("Component Instances"), "should list instances");
        assert!(output.contains("srv"), "should show srv");
        assert!(output.contains("middleware"), "should show middleware");

        // Connections section
        assert!(output.contains("Connections"), "should have connections section");
        assert!(output.contains("Export"), "should show export");
    }

    #[test]
    fn test_full_ascii() {
        let graph = test_graph();
        let output = generate_ascii(&graph, DetailLevel::Full);

        assert!(output.contains("All Instances"), "should have instances section");
        assert!(output.contains("srv"), "should show srv");
        assert!(output.contains("middleware"), "should show middleware");
        // Full mode shows full interface names
        assert!(output.contains("wasi:http/handler@0.3.0"), "should show full interface name");
        assert!(output.contains("Connections"), "should have connections");
    }

    #[test]
    fn test_empty_graph_ascii() {
        let graph = CompositionGraph::new();

        let chain = generate_ascii(&graph, DetailLevel::HandlerChain);
        assert!(chain.contains("No HTTP handler chain found"));

        let all = generate_ascii(&graph, DetailLevel::AllInterfaces);
        assert!(all.contains("No component instances found"));

        let full = generate_ascii(&graph, DetailLevel::Full);
        assert!(full.contains("No instances found"));
    }
}
