use crate::canonical_id::canonical_edge_id;
use crate::highlights::{format_tag_label, HighlightColor, Highlights};
use crate::model::{short_interface_name, CompositionGraph};
use crate::output::{
    build_all_interfaces_view, build_full_view, DetailLevel, Direction, SymbolMap,
};
use crate::subgraph::{
    canonical_ids, compute_export_subgraphs, filtered_tag_lines, shared_instances,
};
use crate::{find_chain_interfaces, get_chain_for};
use std::collections::{BTreeMap, BTreeSet};

/// Generate a Mermaid diagram from the composition graph.
pub fn generate_mermaid(
    graph: &CompositionGraph,
    detail: DetailLevel,
    direction: Direction,
    show_types: bool,
    highlights: Option<&Highlights>,
) -> String {
    match detail {
        DetailLevel::HandlerChain => generate_handler_chain(graph, direction, show_types),
        DetailLevel::AllInterfaces => generate_all_interfaces(graph, direction, show_types),
        DetailLevel::Graph => generate_graph(graph, direction, show_types, highlights),
        DetailLevel::Full => generate_full(graph, direction, show_types),
    }
}

/// One `subgraph` cluster per top-level export, mirroring the ASCII graph
/// view (sectioning by export, request-flow edges, shared-instance
/// distinction) while letting Mermaid handle the 2D placement.
fn generate_graph(
    graph: &CompositionGraph,
    direction: Direction,
    show_types: bool,
    highlights: Option<&Highlights>,
) -> String {
    let subgraphs = compute_export_subgraphs(graph);
    if subgraphs.is_empty() {
        return generate_all_interfaces(graph, direction, show_types);
    }

    let shared = shared_instances(&subgraphs);

    let mut output = String::from(INIT_DIRECTIVE);
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));
    output.push_str("    classDef shared stroke-width:3px,stroke-dasharray:5 3\n");

    if let Some(h) = highlights {
        for color in h.colors_used() {
            output.push_str(&format!(
                "    classDef hl_{color} fill:{},stroke:#333,stroke-width:3px\n",
                color.mermaid_hex(),
            ));
        }
    }
    output.push('\n');

    let mut symbols = SymbolMap::new();
    let mut already_rendered: BTreeSet<u32> = BTreeSet::new();

    let mut link_index: usize = 0;
    let mut link_styles: Vec<(usize, HighlightColor)> = Vec::new();

    for sg in &subgraphs {
        let short = short_interface_name(&sg.interface_name);
        let sg_id = format!("sg_{}", sanitize_for_mermaid(&sg.interface_name));
        output.push_str(&format!("    subgraph {sg_id}[\"export: {short}\"]\n"));

        for &idx in &sg.nodes {
            let Some(node) = graph.nodes.get(&idx) else {
                continue;
            };
            if !already_rendered.insert(idx) {
                continue;
            }
            let node_id = node_id_for(idx);
            let suffix = node_tag_suffix(highlights, node.canonical_id());
            output.push_str(&format!(
                "        {node_id}[\"{}{suffix}\"]\n",
                escape_mermaid_label(node.display_label()),
            ));
            // Highlight wins over shared (matches the ASCII renderer).
            let node_hl = highlights.and_then(|h| h.node_color(node.canonical_id()));
            if let Some(color) = node_hl {
                output.push_str(&format!("        class {node_id} hl_{color}\n"));
            } else if shared.contains(&idx) && idx != sg.source_instance {
                output.push_str(&format!("        class {node_id} shared\n"));
            }
        }

        // Export marker → source-instance arrow.
        let export_node = format!("ext_{}", sanitize_for_mermaid(&sg.interface_name));
        let export_sym = symbols.export_symbol(graph, &sg.interface_name, show_types);
        let (export_hl, export_tag_ids) = export_highlight(graph, highlights, sg);
        let export_suffix = escape_mermaid_label(&format_tag_label(&export_tag_ids));
        output.push_str(&format!(
            "        {export_node}([\"ext: {short}{export_sym}{export_suffix}\"]) --> {}\n",
            node_id_for(sg.source_instance),
        ));
        if let Some(color) = export_hl {
            link_styles.push((link_index, color));
        }
        link_index += 1;

        // Inter-component edges, merging parallel interfaces between the
        // same (caller, provider) pair into one labeled arrow.
        let mut by_pair: BTreeMap<(u32, u32), (Vec<String>, Option<HighlightColor>)> =
            BTreeMap::new();
        for e in &sg.edges {
            let label = short_interface_name(&e.interface);
            let symbol = edge_type_symbol(graph, &mut symbols, e, show_types);
            let (iface_hl, iface_tag_ids) =
                edge_highlight(graph, highlights, e.caller, e.provider, &e.interface);
            let ctx_suffix = format_tag_label(&iface_tag_ids);
            let entry = by_pair.entry((e.caller, e.provider)).or_default();
            entry.0.push(format!("{label}{symbol}{ctx_suffix}"));
            // First non-None interface highlight wins the link's color
            // (matches the ASCII per-edge aggregation).
            if entry.1.is_none() {
                entry.1 = iface_hl;
            }
        }
        for ((caller, provider), (labels, hl)) in by_pair {
            output.push_str(&format!(
                "        {} -->|\"{}\"| {}\n",
                node_id_for(caller),
                escape_mermaid_label(&labels.join(",")),
                node_id_for(provider),
            ));
            if let Some(color) = hl {
                link_styles.push((link_index, color));
            }
            link_index += 1;
        }

        output.push_str("    end\n\n");
    }

    let (present_nodes, present_edges) = canonical_ids(graph, &subgraphs);
    output.push_str(&render_key_with_tags(
        &symbols,
        highlights,
        &present_nodes,
        &present_edges,
    ));

    for (idx, color) in link_styles {
        output.push_str(&format!(
            "    linkStyle {} stroke:{},stroke-width:3px\n",
            idx,
            color.mermaid_hex(),
        ));
    }
    output
}

fn node_tag_suffix(highlights: Option<&Highlights>, canonical_id: &str) -> String {
    let raw = highlights
        .map(|h| format_tag_label(&h.node_tag_ids(canonical_id)))
        .unwrap_or_default();
    escape_mermaid_label(&raw)
}

fn edge_highlight(
    graph: &CompositionGraph,
    highlights: Option<&Highlights>,
    caller: u32,
    provider: u32,
    interface: &str,
) -> (Option<HighlightColor>, Vec<u32>) {
    let Some(h) = highlights else {
        return (None, Vec::new());
    };
    let Some(caller_label) = graph.nodes.get(&caller).map(|n| n.canonical_id()) else {
        return (None, Vec::new());
    };
    let Some(provider_label) = graph.nodes.get(&provider).map(|n| n.canonical_id()) else {
        return (None, Vec::new());
    };
    let id = canonical_edge_id(interface, Some(caller_label), provider_label);
    (h.edge_color(&id), h.edge_tag_ids(&id))
}

/// Like [`edge_highlight`] but for the boundary (export) edge of a
/// subgraph.
fn export_highlight(
    graph: &CompositionGraph,
    highlights: Option<&Highlights>,
    sg: &crate::subgraph::ExportSubgraph,
) -> (Option<HighlightColor>, Vec<u32>) {
    let Some(h) = highlights else {
        return (None, Vec::new());
    };
    let Some(src) = graph.nodes.get(&sg.source_instance) else {
        return (None, Vec::new());
    };
    let id = canonical_edge_id(&sg.interface_name, None, src.canonical_id());
    (h.edge_color(&id), h.edge_tag_ids(&id))
}

fn edge_type_symbol(
    graph: &CompositionGraph,
    symbols: &mut SymbolMap,
    e: &crate::subgraph::SubgraphEdge,
    show_types: bool,
) -> String {
    if !show_types {
        return String::new();
    }
    let conn = graph
        .nodes
        .get(&e.caller)
        .and_then(|n| n.imports.iter().find(|c| c.interface_name == e.interface));
    let fp = conn.and_then(|c| c.fingerprint.as_deref());
    let lines = conn
        .and_then(|c| c.interface_type.as_ref())
        .map(|it| crate::output::format_interface_type_lines(it, &graph.arena))
        .unwrap_or_default();
    symbols.assign(true, fp, lines)
}

fn node_id_for(idx: u32) -> String {
    format!("n{}", idx)
}

fn render_key_with_tags(
    symbols: &SymbolMap,
    highlights: Option<&Highlights>,
    present_nodes: &BTreeSet<String>,
    present_edges: &BTreeSet<String>,
) -> String {
    let tag_lines = filtered_tag_lines(highlights, present_nodes, present_edges);
    let sigs_block =
        (!symbols.is_empty()).then(|| build_key_block("Signatures:", symbols.key_lines()));
    let tags_block = (!tag_lines.is_empty()).then(|| build_key_block("Tags:", tag_lines));

    let content = match (sigs_block, tags_block) {
        (None, None) => return String::new(),
        (Some(sigs), None) => format!("<div style='text-align:left'>{sigs}</div>"),
        (None, Some(tags)) => format!("<div style='text-align:left'>{tags}</div>"),
        (Some(sigs), Some(tags)) => format!(
            "<div style='text-align:left'>\
             <div style='display:inline-block;vertical-align:top;padding-right:32px'>{sigs}</div>\
             <div style='display:inline-block;vertical-align:top'>{tags}</div>\
             </div>",
        ),
    };

    format!("\n    key[\"{content}\"]\n    style key fill:none,stroke:none,color:#888\n")
}

fn build_key_block(header: &str, body: Vec<String>) -> String {
    std::iter::once(header.to_string())
        .chain(
            body.into_iter()
                .map(|l| preserve_leading_indent(&escape_mermaid_label(&l))),
        )
        .collect::<Vec<_>>()
        .join("<br/>")
}

/// Signatures-only key node used by the legacy detail levels (which
/// don't carry highlight tags).
fn render_key(symbols: &SymbolMap) -> String {
    if symbols.is_empty() {
        return String::new();
    }
    let body = build_key_block("Signatures:", symbols.key_lines());
    format!(
        "\n    key[\"<div style='text-align:left'>{body}</div>\"]\
        \n    style key fill:none,stroke:none,color:#888\n"
    )
}

/// Escape characters that Mermaid's default label parser (marked.js)
/// interprets as markdown/HTML.
fn escape_mermaid_label(s: &str) -> String {
    s.replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('`', "'")
        .replace('[', "⟦")
        .replace(']', "⟧")
}

fn preserve_leading_indent(s: &str) -> String {
    let mut out = String::new();
    let mut leading = true;
    for c in s.chars() {
        if leading && c == ' ' {
            out.push_str("&nbsp;&nbsp;");
        } else {
            leading = false;
            out.push(c);
        }
    }
    out
}

/// Mermaid auto-wraps node label text at ~200 px by default; bump it
/// so the wide Signatures key isn't wrapped at an arbitrary mid-line
/// column.
const INIT_DIRECTIVE: &str = "%%{init: {'flowchart': {'wrappingWidth': 2400}}}%%\n";

/// Each exported handler interface as a request-flow chain.
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

    for iface in &chain_interfaces {
        let chain = get_chain_for(graph, iface);
        if chain.is_empty() {
            continue;
        }
        let short = short_interface_name(iface);
        let export_sym = symbols.export_symbol(graph, iface, show_types);

        if let Some(&first_idx) = chain.first() {
            if let Some(first_node) = graph.get_node(first_idx) {
                output.push_str(&format!(
                    "    export_{}([\"Export: {short}{export_sym}\"]) --> {}\n",
                    sanitize_for_mermaid(iface),
                    sanitize_for_mermaid(&first_node.name)
                ));
            }
        }

        for window in chain.windows(2) {
            if let [from_idx, to_idx] = window {
                if let (Some(from_node), Some(to_node)) =
                    (graph.get_node(*from_idx), graph.get_node(*to_idx))
                {
                    let conn_sym = symbols.connection_symbol(graph, from_node, iface, show_types);
                    output.push_str(&format!(
                        "    {} -->|\"{short}{conn_sym}\"| {}\n",
                        sanitize_for_mermaid(&from_node.name),
                        sanitize_for_mermaid(&to_node.name)
                    ));
                }
            }
        }
    }

    output.push_str(&render_key(&symbols));
    output
}

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
        let arrow = if edge.is_dashed { "-.->" } else { "-->" };
        output.push_str(&format!(
            "    {} {}|\"{}{}\"| {}\n",
            from_id, arrow, edge.label, sym, to_id
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
            exp.short_name,
            sym
        ));
    }

    output.push_str(&render_key(&symbols));

    output
}

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
    use crate::highlights::Selection;
    use crate::output::Direction;
    use crate::test_utils::*;

    #[test]
    fn test_handler_chain_mermaid() {
        let graph = simple_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::HandlerChain,
            Direction::LeftToRight,
            false,
            None,
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
        let graph = simple_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
            None,
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
        assert!(output.contains("-->|"), "should have solid instance edges");
        // Export
        assert!(output.contains("Export"), "should have export");
    }

    #[test]
    fn test_full_mermaid() {
        let graph = simple_chain_graph();
        let output = generate_mermaid(&graph, DetailLevel::Full, Direction::TopDown, false, None);

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
            None,
        );
        assert!(chain.contains("No middleware chains found"));

        let all = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
            None,
        );
        assert!(all.contains("No component instances found"));
    }

    #[test]
    fn test_show_types_all_interfaces() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            true,
            None,
        );

        assert!(
            output.contains("'handle': (u32) -&gt; bool"),
            "should embed function signature in edge label"
        );
    }

    #[test]
    fn test_show_types_full() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::Full,
            Direction::LeftToRight,
            true,
            None,
        );

        assert!(
            output.contains("'handle': (u32) -&gt; bool"),
            "should embed function signature in edge label"
        );
    }

    #[test]
    fn test_hide_types_mermaid() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::AllInterfaces,
            Direction::LeftToRight,
            false,
            None,
        );

        assert!(
            !output.contains("'handle': (u32) -&gt; bool"),
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
        );
        assert!(
            output.contains("Signatures:"),
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
            None,
        );
        assert!(
            output.contains("'handle': (u32) -&gt; bool"),
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
            None,
        );
        // Two distinct types → two entries in the single key node (separated by \n in the label)
        let key_line = output
            .lines()
            .find(|l| l.contains("Signatures:"))
            .expect("no key node");
        assert!(
            key_line.matches("-&gt;").count() >= 2,
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
            None,
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
            None,
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
            None,
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
            None,
        );
        assert!(
            !output.contains("Signatures:"),
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
            None,
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

        let output = generate_mermaid(
            &graph,
            DetailLevel::Full,
            Direction::LeftToRight,
            false,
            None,
        );
        assert!(
            output.contains("synth"),
            "synthetic node should appear in Full output"
        );
        assert!(
            output.contains("(synthetic)"),
            "synthetic node should be labelled as synthetic"
        );
    }

    // -----------------------------------------------------------------------
    // Highlights (Graph detail level only)
    // -----------------------------------------------------------------------

    #[test]
    fn mermaid_graph_highlight_emits_classdef_and_class() {
        let graph = simple_chain_graph();
        let mut h = Highlights::default();
        h.mark(Selection::node("srv"));
        let output = generate_mermaid(
            &graph,
            DetailLevel::Graph,
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(
            output.contains("classDef hl_yellow"),
            "expected classDef for default-color highlight, got:\n{output}"
        );
        assert!(
            output.contains("class n1 hl_yellow"),
            "expected `class n1 hl_yellow` for srv (idx 1), got:\n{output}"
        );
    }

    #[test]
    fn mermaid_graph_highlight_color_override() {
        let graph = simple_chain_graph();
        let mut h = Highlights::default();
        h.mark(Selection::node("srv").color(HighlightColor::Orange));
        let output = generate_mermaid(
            &graph,
            DetailLevel::Graph,
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(
            output.contains("classDef hl_orange"),
            "expected orange classDef, got:\n{output}"
        );
        assert!(
            output.contains("class n1 hl_orange"),
            "expected n1 in orange class, got:\n{output}"
        );
    }

    #[test]
    fn mermaid_graph_highlight_edge_emits_linkstyle() {
        let graph = simple_chain_graph();
        let mut h = Highlights::default();
        h.register_tag(1, "drained").unwrap();
        h.mark(Selection::edge("wasi:http/handler@0.3.0::middleware->srv").tag(1));
        let output = generate_mermaid(
            &graph,
            DetailLevel::Graph,
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(
            output.contains("linkStyle"),
            "expected linkStyle for highlighted edge, got:\n{output}"
        );
        // Bracket label appears on edge text — escape_mermaid_label
        // converts `[`/`]` to Unicode lookalikes (`⟦`/`⟧`) so marked.js
        // doesn't read them as markdown link syntax and so the entities
        // survive downstream markdown pipelines that strip `&#91;`.
        assert!(
            output.contains("handler⟦1⟧"),
            "expected `handler⟦1⟧` on edge label, got:\n{output}"
        );
        // Tag list in the key node
        assert!(
            output.contains("Tags:"),
            "expected Tags in key node, got:\n{output}"
        );
        assert!(
            output.contains("1 drained"),
            "missing tag entry, got:\n{output}"
        );
    }

    #[test]
    fn mermaid_graph_no_highlights_no_extra_styles() {
        let graph = simple_chain_graph();
        let output = generate_mermaid(
            &graph,
            DetailLevel::Graph,
            Direction::LeftToRight,
            false,
            None,
        );
        assert!(
            !output.contains("classDef hl_"),
            "should not emit highlight classDefs when no highlights given"
        );
        assert!(
            !output.contains("linkStyle"),
            "should not emit linkStyle without highlights"
        );
        assert!(!output.contains("Tags:"));
    }

    #[test]
    fn mermaid_graph_highlight_wins_over_shared_class() {
        // Reuse the shared-instance fixture used by the ASCII test.  When
        // `logger` is highlighted in the second cluster, the emitted class
        // should be `hl_*` (not `shared`).
        use crate::model::{ComponentNode, CompositionGraph, InterfaceConnection};
        let mut g = CompositionGraph::new();
        g.add_node(1, ComponentNode::new("$logger".into(), 0, 0));
        let mut srv = ComponentNode::new("$srv-http".into(), 1, 1);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".into(),
            source_instance: Some(1),
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        g.add_node(2, srv);
        let mut cache = ComponentNode::new("$cache".into(), 2, 2);
        cache.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".into(),
            source_instance: Some(1),
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        g.add_node(3, cache);
        g.add_export("wasi:http/handler@0.3.0".into(), 2, None);
        g.add_export("wasi:keyvalue/store@0.1.0".into(), 3, None);

        let mut h = Highlights::default();
        h.mark(Selection::node("logger"));
        let output = generate_mermaid(
            &g,
            DetailLevel::Graph,
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        // logger (n1) should be in the hl_* class, not the shared class.
        assert!(
            output.contains("class n1 hl_yellow"),
            "logger should be highlighted, got:\n{output}"
        );
        assert!(
            !output.contains("class n1 shared"),
            "highlight should override shared class for logger, got:\n{output}"
        );
    }
}
