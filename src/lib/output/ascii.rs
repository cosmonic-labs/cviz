use crate::{find_chain_interfaces, get_chain_for};
use crate::model::{short_interface_name, CompositionGraph};
use crate::output::{build_all_interfaces_view, build_full_view, SymbolMap, DetailLevel};

/// Generate an ASCII diagram from the composition graph
pub fn generate_ascii(graph: &CompositionGraph, detail: DetailLevel, show_types: bool) -> String {
    match detail {
        DetailLevel::HandlerChain => generate_handler_chain_ascii(graph, show_types),
        DetailLevel::AllInterfaces => generate_all_interfaces_ascii(graph, show_types),
        DetailLevel::Full => generate_full_ascii(graph, show_types),
    }
}

/// Generate ASCII diagram showing all middleware chains (request flow direction)
fn generate_handler_chain_ascii(graph: &CompositionGraph, show_types: bool) -> String {
    let chain_interfaces = find_chain_interfaces(graph);

    if chain_interfaces.is_empty() {
        return box_content("Middleware Chains", &["No middleware chains found"]);
    }

    let mut symbols = SymbolMap::new();
    let mut lines = Vec::new();

    for (i, iface) in chain_interfaces.iter().enumerate() {
        let chain = get_chain_for(graph, iface);
        if chain.is_empty() {
            continue;
        }

        // Separator between chains
        if i > 0 {
            lines.push(String::new());
        }

        let short = short_interface_name(iface);

        let export_sym: String = show_types
            .then(|| {
                graph.component_exports.get(iface.as_str())
                    .and_then(|info| symbols.symbol_for_export(info, &graph.arena))
                    .map(str::to_string)
            })
            .flatten()
            .unwrap_or_default();

        // Export entry point
        if let Some(&first_idx) = chain.first() {
            if let Some(first_node) = graph.get_node(first_idx) {
                lines.push(format!(
                    "[Export: {}{}] ──> {}",
                    short, export_sym,
                    first_node.display_label()
                ));
            }
        }

        // Chain connections
        for window in chain.windows(2) {
            if let [from_idx, to_idx] = window {
                if let (Some(from_node), Some(to_node)) =
                    (graph.get_node(*from_idx), graph.get_node(*to_idx))
                {
                    let conn_sym: String = show_types
                        .then(|| {
                            from_node.imports.iter()
                                .find(|c| &c.interface_name == iface)
                                .and_then(|c| symbols.symbol_for_conn(c, &graph.arena))
                                .map(str::to_string)
                        })
                        .flatten()
                        .unwrap_or_default();
                    lines.push(format!(
                        "{} ──{}{}──> {}",
                        from_node.display_label(),
                        short, conn_sym,
                        to_node.display_label()
                    ));
                }
            }
        }
    }

    // Key — shared across all chains
    if !symbols.is_empty() {
        lines.push(String::new());
        lines.extend(symbols.key_lines());
    }

    box_content("Middleware Chains", &lines)
}

/// Generate ASCII diagram showing all interface connections
fn generate_all_interfaces_ascii(graph: &CompositionGraph, show_types: bool) -> String {
    let view = build_all_interfaces_view(graph, show_types);

    if view.nodes.is_empty() {
        return box_content("Component Instances", &["No component instances found"]);
    }

    let mut output = String::new();

    if !view.host_names.is_empty() {
        let host_lines: Vec<String> = view.host_names
            .iter()
            .map(|i| format!("  {{{}}}", short_interface_name(i)))
            .collect();
        output.push_str(&box_content("Host Imports", &host_lines));
        output.push('\n');
    }

    let instance_lines: Vec<String> = view.nodes
        .iter()
        .map(|n| format!("  [{}]", n.display))
        .collect();
    output.push_str(&box_content("Component Instances", &instance_lines));
    output.push('\n');

    let mut connection_lines = Vec::new();
    for edge in &view.edges {
        if edge.is_dashed {
            connection_lines.push(format!(
                "  {{{}}} -.{}.- [{}]",
                edge.from_display, edge.label, edge.to_display
            ));
        } else {
            connection_lines.push(format!(
                "  [{}] ──{}──> [{}]",
                edge.from_display, edge.label, edge.to_display
            ));
        }
        for line in &edge.type_lines {
            connection_lines.push(format!("      {}", line));
        }
    }
    for exp in &view.exports {
        connection_lines.push(format!("  [{}] ──> (Export: {})", exp.from_display, exp.short_name));
        for line in &exp.type_lines {
            connection_lines.push(format!("      {}", line));
        }
    }

    if !connection_lines.is_empty() {
        output.push_str(&box_content("Connections", &connection_lines));
    }

    output
}

/// Generate a full ASCII diagram with all details
fn generate_full_ascii(graph: &CompositionGraph, show_types: bool) -> String {
    let view = build_full_view(graph, show_types);

    let mut instance_lines: Vec<String> = view.nodes
        .iter()
        .map(|n| {
            if n.is_synthetic {
                format!("  [{}] (synthetic)", n.display)
            } else {
                format!("  [{}] [comp:{}]", n.display, n.component_index)
            }
        })
        .collect();

    if instance_lines.is_empty() {
        instance_lines.push("  No instances found".to_string());
    }

    let mut output = String::new();
    output.push_str(&box_content("All Instances", &instance_lines));
    output.push('\n');

    let mut connection_lines = Vec::new();
    for edge in &view.edges {
        connection_lines.push(format!(
            "  [{}] ──{}──> [{}]",
            edge.from_display, edge.label, edge.to_display
        ));
        for line in &edge.type_lines {
            connection_lines.push(format!("      {}", line));
        }
    }
    for exp in &view.exports {
        connection_lines.push(format!("  [{}] ──> (Export: {})", exp.from_display, exp.full_name));
        for line in &exp.type_lines {
            connection_lines.push(format!("      {}", line));
        }
    }

    if !connection_lines.is_empty() {
        output.push_str(&box_content("Connections", &connection_lines));
    }

    output
}

/// Calculate the display width of a string (number of terminal columns).
/// Uses char count instead of byte length to handle multi-byte Unicode
/// characters like box-drawing characters (─) which are 3 bytes but 1 column.
fn display_width(s: &str) -> usize {
    s.chars().count()
}

/// Create a box around content with a title
fn box_content(title: &str, lines: &[impl AsRef<str>]) -> String {
    // Calculate the width needed (in display columns, not bytes)
    let title_width = display_width(title) + 2; // Add padding around title
    let max_line_width = lines
        .iter()
        .map(|l| display_width(l.as_ref()))
        .max()
        .unwrap_or(0);
    let width = std::cmp::max(title_width, max_line_width) + 4; // Add padding

    let mut output = String::new();

    // Top border
    output.push_str(&format!("┌{}┐\n", "─".repeat(width)));

    // Title line centered
    let title_display = display_width(title);
    let title_padding = (width - title_display) / 2;
    let title_padding_right = width - title_display - title_padding;
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
        let padding = width.saturating_sub(display_width(line_str));
        output.push_str(&format!("│{}{}│\n", line_str, " ".repeat(padding)));
    }

    // Bottom border
    output.push_str(&format!("└{}┘", "─".repeat(width)));

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComponentNode, FuncSignature, InstanceInterface, InterfaceConnection, InterfaceType, ValueType};
    use std::collections::BTreeMap;

    /// Build a graph: host → $srv → $middleware → export(handler)
    fn test_graph() -> CompositionGraph {
        let mut graph = CompositionGraph::new();

        let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: 0,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, srv);

        let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: 1,
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".to_string(),
            source_instance: 0,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(2, mw);

        graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, None);
        graph
    }

    /// Build a graph with real type information for type-display tests.
    ///
    /// Adds an instance interface with a single `handle(u32) -> bool` function
    /// to both the import and export connections.
    fn test_graph_with_types() -> CompositionGraph {
        let mut graph = CompositionGraph::new();

        // Intern the param/result types up front
        let u32_id = graph.arena.intern_val(ValueType::U32);
        let bool_id = graph.arena.intern_val(ValueType::Bool);

        let handle_sig = FuncSignature {
            params: vec![u32_id],
            results: vec![bool_id],
        };
        let mut functions = BTreeMap::new();
        functions.insert("handle".to_string(), handle_sig);
        let iface_type = InterfaceType::Instance(InstanceInterface { functions });

        let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: 0,
            is_host_import: true,
            interface_type: Some(iface_type.clone()),
            fingerprint: Some(iface_type.fingerprint(&graph.arena)),
        });
        graph.add_node(1, srv);

        let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: 1,
            is_host_import: false,
            interface_type: Some(iface_type.clone()),
            fingerprint: Some(iface_type.fingerprint(&graph.arena)),
        });
        graph.add_node(2, mw);

        graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, Some(iface_type));
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
        let output = generate_ascii(&graph, DetailLevel::HandlerChain, false);

        assert!(output.contains("Middleware Chains"), "should have title");
        assert!(output.contains("srv"), "should show srv node");
        assert!(output.contains("middleware"), "should show middleware node");
        assert!(output.contains("handler"), "should show handler label");
        assert!(output.contains("Export"), "should show export at start");
        // Request flow order: export → middleware → srv
        assert!(
            output.contains("[Export: handler] ──> middleware"),
            "should show export pointing to outermost handler"
        );
        assert!(
            output.contains("middleware ──handler──> srv"),
            "should show request flow from middleware to srv"
        );
    }

    #[test]
    fn test_all_interfaces_ascii() {
        let graph = test_graph();
        let output = generate_ascii(&graph, DetailLevel::AllInterfaces, false);

        assert!(output.contains("Host Imports"), "should have host imports section");
        assert!(output.contains("handler"), "should show handler interface");
        assert!(output.contains("log"), "should show log interface");
        assert!(output.contains("Component Instances"), "should list instances");
        assert!(output.contains("srv"), "should show srv");
        assert!(output.contains("middleware"), "should show middleware");
        assert!(output.contains("Connections"), "should have connections section");
        assert!(output.contains("Export"), "should show export");
    }

    #[test]
    fn test_full_ascii() {
        let graph = test_graph();
        let output = generate_ascii(&graph, DetailLevel::Full, false);

        assert!(output.contains("All Instances"), "should have instances section");
        assert!(output.contains("srv"), "should show srv");
        assert!(output.contains("middleware"), "should show middleware");
        assert!(output.contains("wasi:http/handler@0.3.0"), "should show full interface name");
        assert!(output.contains("Connections"), "should have connections");
    }

    #[test]
    fn test_empty_graph_ascii() {
        let graph = CompositionGraph::new();

        let chain = generate_ascii(&graph, DetailLevel::HandlerChain, false);
        assert!(chain.contains("No middleware chains found"), "{}", chain);

        let all = generate_ascii(&graph, DetailLevel::AllInterfaces, false);
        assert!(all.contains("No component instances found"));

        let full = generate_ascii(&graph, DetailLevel::Full, false);
        assert!(full.contains("No instances found"));
    }

    #[test]
    fn test_show_types_all_interfaces() {
        let graph = test_graph_with_types();
        let output = generate_ascii(&graph, DetailLevel::AllInterfaces, true);

        // Each connection should be followed by the function signature
        assert!(output.contains("`handle`: (u32) -> bool"), "should show function signature");
    }

    #[test]
    fn test_show_types_full() {
        let graph = test_graph_with_types();
        let output = generate_ascii(&graph, DetailLevel::Full, true);

        assert!(output.contains("`handle`: (u32) -> bool"), "should show function signature");
    }

    #[test]
    fn test_hide_types_all_interfaces() {
        let graph = test_graph_with_types();
        let output = generate_ascii(&graph, DetailLevel::AllInterfaces, false);

        // Type lines should not appear when show_types=false
        assert!(!output.contains("`handle`: (u32) -> bool"), "should not show signatures when types disabled");
    }
}
