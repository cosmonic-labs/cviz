use crate::model::{short_interface_name, CompositionGraph};
use crate::output::{
    build_all_interfaces_view, build_full_view, DetailLevel, Direction, SymbolMap,
};
use crate::{find_chain_interfaces, get_chain_for};

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

/// Word-wrap a single key entry line with a hanging indent.
///
/// The first token (the symbol, e.g. `✦`) acts as a bullet; continuation
/// lines are indented by two spaces so they align past the `"✦ "` prefix.
/// `max_cols` is measured in Unicode scalar values (display columns for the
/// ASCII/symbol mix we emit).  Lines already short enough are returned as-is.
fn wrap_bullet(line: &str, max_cols: usize) -> String {
    if line.chars().count() <= max_cols {
        return line.to_string();
    }
    // Continuation lines indent past the bullet symbol and its trailing space.
    let indent = "  ";
    let mut result = String::new();
    let mut current = String::new();

    for word in line.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= max_cols {
            current.push(' ');
            current.push_str(word);
        } else {
            if !result.is_empty() {
                result.push_str("\\n");
            }
            result.push_str(&current);
            current = format!("{indent}{word}");
        }
    }
    if !current.is_empty() {
        if !result.is_empty() {
            result.push_str("\\n");
        }
        result.push_str(&current);
    }
    result
}

/// Render the type-symbol key as a plain-text annotation node.
///
/// Produces a single borderless Mermaid node with `Key` as a header and one
/// wrapped entry per symbol.  Returns an empty string when the SymbolMap is
/// empty.
fn render_key(symbols: &SymbolMap) -> String {
    if symbols.is_empty() {
        return String::new();
    }
    let content = std::iter::once("Key".to_string())
        .chain(symbols.key_lines().into_iter().map(|l| wrap_bullet(&l, 72)))
        .collect::<Vec<_>>()
        .join("\\n");
    let mut out = String::new();
    out.push_str(&format!("\n    key[\"{content}\"]\n"));
    out.push_str("    style key fill:none,stroke:none,text-align:left,color:#888\n");
    out
}

/// Mermaid init directive that widens the text-wrapping threshold.
///
/// Mermaid auto-wraps node label text at ~200 px by default.  Setting a
/// larger `wrappingWidth` prevents the renderer from breaking our carefully
/// formatted key lines at unexpected points.
const INIT_DIRECTIVE: &str = "%%{init: {'flowchart': {'wrappingWidth': 600}}}%%\n";

/// Generate a diagram showing all middleware chains (request flow direction)
fn generate_handler_chain(
    graph: &CompositionGraph,
    direction: Direction,
    show_types: bool,
) -> String {
    let mut output = String::from(INIT_DIRECTIVE);
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    let chain_interfaces = find_chain_interfaces(graph);

    if chain_interfaces.is_empty() {
        output.push_str("    empty[\"No middleware chains found\"]\n");
        return output;
    }

    let mut symbols = SymbolMap::new();

    // One subgraph per chain interface, all nodes collected into a single
    // "Middleware Chains" subgraph
    output.push_str("    subgraph composition[\"Service Chains\"]\n");
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
                graph
                    .component_exports
                    .get(iface.as_str())
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
                    short,
                    export_sym,
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
                            from_node
                                .imports
                                .iter()
                                .find(|c| &c.interface_name == iface)
                                .and_then(|c| symbols.symbol_for_conn(c, &graph.arena))
                                .map(str::to_string)
                        })
                        .flatten()
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "    {} -->|\"{}{}\"| {}\n",
                        sanitize_for_mermaid(&from_node.name),
                        short,
                        conn_sym,
                        sanitize_for_mermaid(&to_node.name)
                    ));
                }
            }
        }
    }

    // Key subgraph — shared across all chains
    output.push_str(&render_key(&symbols));

    output
}

/// Generate a diagram showing all interface connections
fn generate_all_interfaces(
    graph: &CompositionGraph,
    direction: Direction,
    show_types: bool,
) -> String {
    let view = build_all_interfaces_view(graph, show_types);
    let mut output = format!("{INIT_DIRECTIVE}graph {}\n", direction.to_mermaid());

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
        output.push_str(&format!(
            "        {}[\"{}\"]\n",
            sanitize_for_mermaid(&node.name),
            node.display
        ));
    }
    output.push_str("    end\n\n");

    let mut symbols = SymbolMap::new();

    for edge in &view.edges {
        let from_id = sanitize_for_mermaid(&edge.from_name);
        let to_id = sanitize_for_mermaid(&edge.to_name);
        let sym = symbols.assign(
            show_types,
            edge.fingerprint.as_deref(),
            edge.type_lines.clone(),
        );
        if edge.is_dashed {
            output.push_str(&format!(
                "    {} -->|\"{}{}\"| {}\n",
                from_id, edge.label, sym, to_id
            ));
        } else {
            output.push_str(&format!(
                "    {} ──>|\"{}{}\"| {}\n",
                from_id, edge.label, sym, to_id
            ));
        }
    }

    output.push('\n');
    for exp in &view.exports {
        let sym = symbols.assign(
            show_types,
            exp.fingerprint.as_deref(),
            exp.type_lines.clone(),
        );
        output.push_str(&format!(
            "    {} --> export_{}([\"Export: {}{}\"])\n",
            sanitize_for_mermaid(&exp.from_name),
            sanitize_for_mermaid(&exp.full_name),
            exp.short_name,
            sym
        ));
    }

    output.push_str(&render_key(&symbols));

    output
}

/// Generate a full diagram with all details
fn generate_full(graph: &CompositionGraph, direction: Direction, show_types: bool) -> String {
    let view = build_full_view(graph, show_types);
    let mut output = format!("{INIT_DIRECTIVE}graph {}\n", direction.to_mermaid());

    output.push_str("    subgraph all[\"All Instances\"]\n");
    for node in &view.nodes {
        let label = if node.is_synthetic {
            format!("{} (synthetic)", node.display)
        } else {
            format!("{} [comp:{}]", node.display, node.component_index)
        };
        output.push_str(&format!(
            "        {}[\"{}\"]\n",
            sanitize_for_mermaid(&node.name),
            label
        ));
    }
    output.push_str("    end\n\n");

    let mut symbols = SymbolMap::new();

    for edge in &view.edges {
        let sym = symbols.assign(
            show_types,
            edge.fingerprint.as_deref(),
            edge.type_lines.clone(),
        );
        output.push_str(&format!(
            "    {} -->|\"{}{}\"| {}\n",
            sanitize_for_mermaid(&edge.from_name),
            edge.label,
            sym,
            sanitize_for_mermaid(&edge.to_name)
        ));
    }

    output.push('\n');
    for exp in &view.exports {
        let sym = symbols.assign(
            show_types,
            exp.fingerprint.as_deref(),
            exp.type_lines.clone(),
        );
        output.push_str(&format!(
            "    {} --> export_{}([\"Export: {}{}\"])\n",
            sanitize_for_mermaid(&exp.from_name),
            sanitize_for_mermaid(&exp.full_name),
            exp.full_name,
            sym
        ));
    }

    output.push_str(&render_key(&symbols));

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
    use crate::model::{
        ComponentNode, FuncSignature, InstanceInterface, InterfaceConnection, InterfaceType,
        ValueType,
    };
    use crate::output::Direction;
    use crate::test_utils::*;
    use std::collections::BTreeMap;

    /// Build a graph: host → $srv → $middleware → export(handler)
    fn test_graph() -> CompositionGraph {
        let mut graph = CompositionGraph::new();

        let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: None,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, srv);

        let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: Some(1),
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".to_string(),
            source_instance: None,
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
            source_instance: None,
            is_host_import: true,
            interface_type: Some(iface_type.clone()),
            fingerprint: Some(iface_type.fingerprint(&graph.arena)),
        });
        graph.add_node(1, srv);

        let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: Some(1),
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
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );

        assert!(
            output.contains("graph LR\n"),
            "should contain graph direction"
        );
        assert!(
            output.contains("subgraph composition"),
            "should have subgraph"
        );
        assert!(
            output.contains("Service Chains"),
            "should have Service Chains title"
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
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );

        assert!(output.contains("graph LR\n"));
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
            output.contains("-->"),
            "should have dashed host import edges"
        );
        assert!(output.contains("──>|"), "should have solid instance edges");
        // Export
        assert!(output.contains("Export"), "should have export");
    }

    #[test]
    fn test_full_mermaid() {
        let graph = test_graph();
        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::TopDown, false);

        assert!(output.contains("graph TD\n"), "should use TD direction");
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

        let chain = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        assert!(chain.contains("No middleware chains found"));

        let all = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );
        assert!(all.contains("No component instances found"));
    }

    #[test]
    fn test_show_types_all_interfaces() {
        let graph = test_graph_with_types();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            true,
        );

        assert!(
            output.contains("`handle`: (u32) -> bool"),
            "should embed function signature in edge label"
        );
    }

    #[test]
    fn test_show_types_full() {
        let graph = test_graph_with_types();
        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::LeftToRight, true);

        assert!(
            output.contains("`handle`: (u32) -> bool"),
            "should embed function signature in edge label"
        );
    }

    #[test]
    fn test_hide_types_mermaid() {
        let graph = test_graph_with_types();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );

        assert!(
            !output.contains("`handle`: (u32) -> bool"),
            "should not show signatures when types disabled"
        );
    }

    #[test]
    fn test_sanitize_for_mermaid() {
        assert_eq!(sanitize_for_mermaid("$srv"), "srv");
        assert_eq!(sanitize_for_mermaid("mdl-a"), "mdl_a");
        assert_eq!(sanitize_for_mermaid("instance_0"), "instance_0");
    }

    // -----------------------------------------------------------------------
    // Multiple chains
    // -----------------------------------------------------------------------

    #[test]
    fn test_two_chains_mermaid() {
        let graph = two_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        assert!(
            output.contains("Export: handler"),
            "should show http handler export"
        );
        assert!(
            output.contains("Export: store"),
            "should show keyvalue store export"
        );
    }

    #[test]
    fn test_two_chains_subgraph_nodes() {
        let graph = two_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        // All four chain nodes should appear inside the composition subgraph
        assert!(output.contains("srv_http"), "should have srv-http node");
        assert!(output.contains("mw_http"), "should have mw-http node");
        assert!(output.contains("db"), "should have db node");
        assert!(output.contains("cache"), "should have cache node");
    }

    #[test]
    fn test_two_chains_edges_mermaid() {
        let graph = two_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        assert!(
            output.contains("-->|\"handler\"|"),
            "should have handler edge"
        );
        assert!(output.contains("-->|\"store\"|"), "should have store edge");
    }

    // -----------------------------------------------------------------------
    // Utility node isolation
    // -----------------------------------------------------------------------

    #[test]
    fn test_utility_node_absent_in_handler_chain_mermaid() {
        let graph = chain_plus_utility_graph();
        let chain_out = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        assert!(
            !chain_out.contains("logger"),
            "utility node should not appear in HandlerChain output"
        );

        let all_out = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );
        assert!(
            all_out.contains("logger"),
            "utility node should appear in AllInterfaces output"
        );
    }

    // -----------------------------------------------------------------------
    // Long (3-node) chain
    // -----------------------------------------------------------------------

    #[test]
    fn test_long_chain_mermaid() {
        let graph = long_chain_graph(); // messaging/consumer
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        assert!(output.contains("gateway"), "should show gateway node");
        assert!(output.contains("service"), "should show service node");
        assert!(output.contains("backend"), "should show backend node");
        // Two inter-node edges for consumer
        assert_eq!(
            output.matches("-->|\"consumer\"|").count(),
            2,
            "should have two consumer edges for 3-node chain"
        );
    }

    // -----------------------------------------------------------------------
    // HandlerChain type symbols / key subgraph
    // -----------------------------------------------------------------------

    #[test]
    fn test_handler_chain_types_key_subgraph_mermaid() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            true,
        );
        assert!(
            output.contains("key[\"Key"),
            "should have key node when show_types=true"
        );
    }

    #[test]
    fn test_handler_chain_types_key_content_mermaid() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            true,
        );
        assert!(
            output.contains("`handle`: (u32) -> bool"),
            "key node should contain function signature"
        );
    }

    #[test]
    fn test_two_typed_chains_distinct_symbols_mermaid() {
        let graph = two_typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            true,
        );
        // Two distinct types → two entries in the single key node (separated by \n in the label)
        let key_line = output
            .lines()
            .find(|l| l.contains("key[\"Key"))
            .expect("no key node");
        assert!(
            key_line.matches("->").count() >= 2,
            "key should contain two type entries, got: {key_line}"
        );
    }

    // -----------------------------------------------------------------------
    // AllInterfaces exact structure
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_interfaces_host_node_shape() {
        let graph = simple_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );
        // Host interfaces use [] shape (not diamond) since we switched in the refactor
        assert!(
            output.contains("subgraph host"),
            "should have host subgraph"
        );
        assert!(
            output.contains("handler"),
            "should show handler host interface"
        );
    }

    #[test]
    fn test_all_interfaces_dashed_edge_present() {
        let graph = simple_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );
        assert!(
            output.contains("-->\"|\"handler\"|") || output.contains("-->|\"handler\"|"),
            "should have dashed edge for host handler import, got:\n{}",
            output
        );
    }

    #[test]
    fn test_all_interfaces_export_node() {
        let graph = simple_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );
        // Export uses stadium shape ([" ... "])
        assert!(
            output.contains("([\"Export: handler\"])"),
            "should have export stadium node, got:\n{}",
            output
        );
    }

    #[test]
    fn test_handler_chain_no_key_subgraph_when_types_disabled() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
        );
        assert!(
            !output.contains("key[\"Key"),
            "no key node when show_types=false"
        );
    }

    #[test]
    fn test_all_interfaces_two_chains_mermaid() {
        let graph = two_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
        );
        // All 4 nodes in composition subgraph
        assert!(output.contains("srv_http"), "should have srv-http node");
        assert!(output.contains("mw_http"), "should have mw-http node");
        assert!(output.contains("db"), "should have db node");
        assert!(output.contains("cache"), "should have cache node");
        // Both exports
        assert!(
            output.contains("Export: handler"),
            "should have handler export"
        );
        assert!(output.contains("Export: store"), "should have store export");
        // Both dashed host edges
        assert_eq!(
            output.matches("-->\"|\"handler\"|").count()
                + output.matches("-->|\"handler\"|").count(),
            1,
            "should have one dashed handler edge"
        );
    }

    #[test]
    fn test_full_synthetic_node_visible_mermaid() {
        use crate::model::{ComponentNode, SYNTHETIC_COMPONENT};
        let mut graph = CompositionGraph::new();
        let real = ComponentNode::new("$real".to_string(), 0, 0);
        graph.add_node(1, real);
        let synthetic = ComponentNode::new(
            "$synth".to_string(),
            SYNTHETIC_COMPONENT,
            SYNTHETIC_COMPONENT,
        );
        graph.add_node(99, synthetic);

        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::LeftToRight, false);
        assert!(
            output.contains("synth"),
            "synthetic node should appear in Full output"
        );
        assert!(
            output.contains("(synthetic)"),
            "synthetic node should be labelled as synthetic"
        );
    }
}
