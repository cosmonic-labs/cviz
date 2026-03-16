use crate::{find_chain_interfaces, get_chain_for};
use crate::model::{short_interface_name, CompositionGraph};
use crate::output::{build_all_interfaces_view, build_full_view, SymbolMap, DetailLevel, Direction};

/// Generate a Mermaid diagram from the composition graph
pub fn generate_mermaid(
    graph: &CompositionGraph,
    detail: DetailLevel,
    direction: Direction,
    show_types: bool,
) -> String {
    match detail {
        DetailLevel::HandlerChain => generate_handler_chain(graph, direction, show_types),
        DetailLevel::AllInterfaces => generate_all_interfaces(graph, direction, show_types),
        DetailLevel::Full => generate_full(graph, direction, show_types),
    }
}

/// Build an edge label string, optionally appending WIT function signatures.
///
/// When `type_lines` is non-empty the label becomes:
/// `"base_label\nfn1: sig\nfn2: sig"` — Mermaid renders `\n` as a line break
/// in most environments.
fn edge_label(base: &str, type_lines: &[String]) -> String {
    if type_lines.is_empty() {
        base.to_string()
    } else {
        format!("{}\\n{}", base, type_lines.join("\\n"))
    }
}

/// Generate a diagram showing all middleware chains (request flow direction)
fn generate_handler_chain(graph: &CompositionGraph, direction: Direction, show_types: bool) -> String {
    let mut output = String::new();
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    let chain_interfaces = find_chain_interfaces(graph);

    if chain_interfaces.is_empty() {
        output.push_str("    empty[\"No middleware chains found\"]\n");
        return output;
    }

    let mut symbols = SymbolMap::new();

    // One subgraph per chain interface, all nodes collected into a single
    // "Middleware Chains" subgraph
    output.push_str("    subgraph composition[\"Middleware Chains\"]\n");
    for iface in &chain_interfaces {
        for &idx in &get_chain_for(graph, iface) {
            if let Some(node) = graph.get_node(idx) {
                let id = sanitize_for_mermaid(&node.name);
                output.push_str(&format!("        {}[\"{}\"]\n", id, node.display_label()));
            }
        }
    }
    output.push_str("    end\n\n");

    // Edges per chain
    for iface in &chain_interfaces {
        let chain = get_chain_for(graph, iface);
        if chain.is_empty() {
            continue;
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

        if let Some(&first_idx) = chain.first() {
            if let Some(first_node) = graph.get_node(first_idx) {
                output.push_str(&format!(
                    "    export_{}([\"Export: {}{}\"]) --> {}\n",
                    sanitize_for_mermaid(iface),
                    short, export_sym,
                    sanitize_for_mermaid(&first_node.name)
                ));
            }
        }

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
                    output.push_str(&format!(
                        "    {} -->|\"{}{}\"| {}\n",
                        sanitize_for_mermaid(&from_node.name),
                        short, conn_sym,
                        sanitize_for_mermaid(&to_node.name)
                    ));
                }
            }
        }
    }

    // Key subgraph — shared across all chains
    if !symbols.is_empty() {
        output.push('\n');
        output.push_str("    subgraph key[\"Key\"]\n");
        for (i, line) in symbols.key_lines().iter().enumerate() {
            output.push_str(&format!("        k{}[\"{}\"]\n", i, line));
        }
        output.push_str("    end\n");
    }

    output
}

/// Generate a diagram showing all interface connections
fn generate_all_interfaces(graph: &CompositionGraph, direction: Direction, show_types: bool) -> String {
    let view = build_all_interfaces_view(graph, show_types);
    let mut output = format!("graph {}\n", direction.to_mermaid());

    if view.nodes.is_empty() {
        output.push_str("    empty[\"No component instances found\"]\n");
        return output;
    }

    if !view.host_names.is_empty() {
        output.push_str("    subgraph host[\"Host Imports\"]\n");
        for name in &view.host_names {
            output.push_str(&format!(
                "        {}[\"{}\"]\n",
                sanitize_for_mermaid(name),
                short_interface_name(name)
            ));
        }
        output.push_str("    end\n\n");
    }

    output.push_str("    subgraph composition[\"Component Instances\"]\n");
    for node in &view.nodes {
        output.push_str(&format!("        {}[\"{}\"]\n", sanitize_for_mermaid(&node.name), node.display));
    }
    output.push_str("    end\n\n");

    for edge in &view.edges {
        let from_id = sanitize_for_mermaid(&edge.from_name);
        let to_id = sanitize_for_mermaid(&edge.to_name);
        let lbl = edge_label(&edge.label, &edge.type_lines);
        if edge.is_dashed {
            output.push_str(&format!("    {} -.->|\"{}\"| {}\n", from_id, lbl, to_id));
        } else {
            output.push_str(&format!("    {} -->|\"{}\"| {}\n", from_id, lbl, to_id));
        }
    }

    output.push('\n');
    for exp in &view.exports {
        let lbl = edge_label(&exp.short_name, &exp.type_lines);
        output.push_str(&format!(
            "    {} --> export_{}([\"Export: {}\"])\n",
            sanitize_for_mermaid(&exp.from_name),
            sanitize_for_mermaid(&exp.full_name),
            lbl
        ));
    }

    output
}

/// Generate a full diagram with all details
fn generate_full(graph: &CompositionGraph, direction: Direction, show_types: bool) -> String {
    let view = build_full_view(graph, show_types);
    let mut output = format!("graph {}\n", direction.to_mermaid());

    output.push_str("    subgraph all[\"All Instances\"]\n");
    for node in &view.nodes {
        let label = if node.is_synthetic {
            format!("{} (synthetic)", node.display)
        } else {
            format!("{} [comp:{}]", node.display, node.component_index)
        };
        output.push_str(&format!("        {}[\"{}\"]\n", sanitize_for_mermaid(&node.name), label));
    }
    output.push_str("    end\n\n");

    for edge in &view.edges {
        let lbl = edge_label(&edge.label, &edge.type_lines);
        output.push_str(&format!(
            "    {} -->|\"{}\"| {}\n",
            sanitize_for_mermaid(&edge.from_name),
            lbl,
            sanitize_for_mermaid(&edge.to_name)
        ));
    }

    output.push('\n');
    for exp in &view.exports {
        let lbl = edge_label(&exp.full_name, &exp.type_lines);
        output.push_str(&format!(
            "    {} --> export_{}([\"Export: {}\"])\n",
            sanitize_for_mermaid(&exp.from_name),
            sanitize_for_mermaid(&exp.full_name),
            lbl
        ));
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
    use crate::model::{ComponentNode, FuncSignature, InstanceInterface, InterfaceConnection, InterfaceType, ValueType};
    use crate::output::Direction;
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
    fn test_graph_with_types() -> CompositionGraph {
        let mut graph = CompositionGraph::new();

        let u32_id = graph.arena.intern_val(ValueType::U32);
        let bool_id = graph.arena.intern_val(ValueType::Bool);

        let handle_sig = FuncSignature { params: vec![u32_id], results: vec![bool_id] };
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
    fn test_handler_chain_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::HandlerChain, Direction::LeftToRight, false);

        assert!(
            output.starts_with("graph LR\n"),
            "should start with graph direction"
        );
        assert!(
            output.contains("subgraph composition"),
            "should have subgraph"
        );
        assert!(
            output.contains("Middleware Chains"),
            "should have Middleware Chains title"
        );
        assert!(output.contains("srv"), "should show srv node");
        assert!(output.contains("middleware"), "should show middleware node");
        assert!(
            output.contains("-->|\"handler\"|"),
            "should have handler edge"
        );
        // Export should point to the first (outermost) node
        assert!(
            output.contains("\"Export: handler\"]) --> middleware"),
            "export should point to outermost handler, got:\n{}",
            output
        );
    }

    #[test]
    fn test_all_interfaces_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::AllInterfaces, Direction::LeftToRight, false);

        assert!(output.starts_with("graph LR\n"));
        // Host imports subgraph
        assert!(
            output.contains("subgraph host"),
            "should have host subgraph"
        );
        assert!(
            output.contains("handler"),
            "should show handler host import"
        );
        assert!(output.contains("log"), "should show log host import");
        // Component instances subgraph
        assert!(
            output.contains("subgraph composition"),
            "should have composition subgraph"
        );
        // Connections
        assert!(
            output.contains("-.->"),
            "should have dashed host import edges"
        );
        assert!(output.contains("-->|"), "should have solid instance edges");
        // Export
        assert!(output.contains("Export"), "should have export");
    }

    #[test]
    fn test_full_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::TopDown, false);

        assert!(output.starts_with("graph TD\n"), "should use TD direction");
        assert!(
            output.contains("subgraph all"),
            "should have all-instances subgraph"
        );
        // Full mode uses full interface names for connections where source exists
        assert!(
            output.contains("wasi:http/handler@0.3.0"),
            "should show full interface name"
        );
    }

    #[test]
    fn test_empty_graph_mermaid() {
        let graph = CompositionGraph::new();

        let chain = generate_mermaid(&graph, DetailLevel::HandlerChain, Direction::LeftToRight, false);
        assert!(chain.contains("No middleware chains found"));

        let all = generate_mermaid(&graph, DetailLevel::AllInterfaces, Direction::LeftToRight, false);
        assert!(all.contains("No component instances found"));
    }

    #[test]
    fn test_show_types_all_interfaces() {
        let graph = test_graph_with_types();
        let output = generate_mermaid(&graph, DetailLevel::AllInterfaces, Direction::LeftToRight, true);

        assert!(output.contains("`handle`: (u32) -> bool"), "should embed function signature in edge label");
    }

    #[test]
    fn test_show_types_full() {
        let graph = test_graph_with_types();
        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::LeftToRight, true);

        assert!(output.contains("`handle`: (u32) -> bool"), "should embed function signature in edge label");
    }

    #[test]
    fn test_hide_types_mermaid() {
        let graph = test_graph_with_types();
        let output = generate_mermaid(&graph, DetailLevel::AllInterfaces, Direction::LeftToRight, false);

        assert!(!output.contains("`handle`: (u32) -> bool"), "should not show signatures when types disabled");
    }

    #[test]
    fn test_sanitize_for_mermaid() {
        assert_eq!(sanitize_for_mermaid("$srv"), "srv");
        assert_eq!(sanitize_for_mermaid("mdl-a"), "mdl_a");
        assert_eq!(sanitize_for_mermaid("instance_0"), "instance_0");
    }
}
