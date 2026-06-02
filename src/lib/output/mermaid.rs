use crate::canonical_id::canonical_edge_id;
use crate::highlights::{format_tag_label, HighlightColor, Highlights};
use crate::model::{short_interface_name, CompositionGraph};
use crate::output::graph::{empty_render_message, filtered_export_subgraphs, GraphRenderOpts};
use crate::output::{Direction, SymbolMap};
use crate::subgraph::{canonical_ids, filtered_tag_lines, shared_instances};
use std::collections::{BTreeMap, BTreeSet};

/// Generate a Mermaid diagram from the composition graph.
///
/// One `subgraph` cluster per top-level export, mirroring the ASCII graph
/// view (sectioning by export, request-flow edges, shared-instance
/// distinction) while letting Mermaid handle the 2D placement.
pub fn generate_mermaid(
    graph: &CompositionGraph,
    opts: &GraphRenderOpts,
    direction: Direction,
    show_types: bool,
    highlights: Option<&Highlights>,
) -> String {
    let subgraphs = filtered_export_subgraphs(graph, opts);
    let mut output = String::from(INIT_DIRECTIVE);
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    if subgraphs.is_empty() {
        output.push_str(&format!(
            "    empty[\"{}\"]\n",
            escape_mermaid_label(&empty_render_message(opts)),
        ));
        return output;
    }

    let shared = shared_instances(&subgraphs);
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

    if opts.show_host_imports {
        render_host_imports_mermaid(&mut output, graph, &already_rendered, &mut link_index);
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

/// Render a "Host Imports" cluster with one node per host interface used by
/// any rendered node, plus a dashed edge from the host node to each
/// importing instance.
fn render_host_imports_mermaid(
    output: &mut String,
    graph: &CompositionGraph,
    rendered: &BTreeSet<u32>,
    link_index: &mut usize,
) {
    let mut by_iface: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for &idx in rendered {
        let Some(node) = graph.nodes.get(&idx) else {
            continue;
        };
        for imp in &node.imports {
            if !imp.is_host_import {
                continue;
            }
            by_iface
                .entry(imp.interface_name.clone())
                .or_default()
                .push(idx);
        }
    }
    if by_iface.is_empty() {
        return;
    }
    output.push_str("    subgraph host[\"Host Imports\"]\n");
    for iface in by_iface.keys() {
        output.push_str(&format!(
            "        host_{}[\"{}\"]\n",
            sanitize_for_mermaid(iface),
            escape_mermaid_label(&short_interface_name(iface)),
        ));
    }
    output.push_str("    end\n\n");
    for (iface, importers) in &by_iface {
        let label = short_interface_name(iface);
        for &importer in importers {
            output.push_str(&format!(
                "    host_{} -.->|\"{}\"| {}\n",
                sanitize_for_mermaid(iface),
                escape_mermaid_label(&label),
                node_id_for(importer),
            ));
            *link_index += 1;
        }
    }
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

    fn mermaid_default(graph: &CompositionGraph) -> String {
        generate_mermaid(
            graph,
            &GraphRenderOpts::default(),
            Direction::LeftToRight,
            false,
            None,
        )
    }

    #[test]
    fn default_renders_export_subgraphs() {
        let graph = simple_chain_graph();
        let output = mermaid_default(&graph);
        assert!(output.contains("graph LR\n"));
        assert!(
            output.contains("subgraph sg_"),
            "should have an export subgraph"
        );
        assert!(
            output.contains("ext: handler"),
            "should label the export entry"
        );
        assert!(output.contains("srv"));
        assert!(output.contains("middleware"));
    }

    #[test]
    fn empty_graph_renders_empty_marker() {
        let graph = CompositionGraph::new();
        let output = mermaid_default(&graph);
        assert!(output.contains("No component instances found"));
    }

    #[test]
    fn show_types_embeds_signature() {
        let graph = typed_chain_graph();
        let output = generate_mermaid(
            &graph,
            &GraphRenderOpts::default(),
            Direction::LeftToRight,
            true,
            None,
        );
        assert!(
            output.contains("'handle': (u32) -&gt; bool"),
            "key should embed function signature, got:\n{output}"
        );
    }

    #[test]
    fn hide_types_drops_signature() {
        let graph = typed_chain_graph();
        let output = mermaid_default(&graph);
        assert!(!output.contains("'handle': (u32) -&gt; bool"));
    }

    #[test]
    fn test_sanitize_for_mermaid() {
        assert_eq!(sanitize_for_mermaid("$srv"), "srv");
        assert_eq!(sanitize_for_mermaid("mdl-a"), "mdl_a");
        assert_eq!(sanitize_for_mermaid("instance_0"), "instance_0");
    }

    #[test]
    fn chain_only_filters_to_chain_interfaces() {
        let graph = two_chain_graph();
        let output = generate_mermaid(
            &graph,
            &GraphRenderOpts {
                chain_only: true,
                ..Default::default()
            },
            Direction::LeftToRight,
            false,
            None,
        );
        assert!(
            output.contains("ext: handler"),
            "expected http chain, got:\n{output}"
        );
        assert!(
            output.contains("ext: store"),
            "expected store chain, got:\n{output}"
        );
    }

    #[test]
    fn filter_substring_keeps_matching_subgraphs() {
        let graph = two_chain_graph();
        let output = generate_mermaid(
            &graph,
            &GraphRenderOpts {
                filter: Some("http".into()),
                ..Default::default()
            },
            Direction::LeftToRight,
            false,
            None,
        );
        assert!(output.contains("ext: handler"));
        assert!(!output.contains("ext: store"));
    }

    // -----------------------------------------------------------------------
    // Highlights
    // -----------------------------------------------------------------------

    #[test]
    fn highlight_emits_classdef_and_class() {
        let graph = simple_chain_graph();
        let mut h = Highlights::default();
        h.mark(Selection::node("srv"));
        let output = generate_mermaid(
            &graph,
            &GraphRenderOpts::default(),
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(output.contains("classDef hl_yellow"));
        assert!(output.contains("class n1 hl_yellow"));
    }

    #[test]
    fn highlight_color_override() {
        let graph = simple_chain_graph();
        let mut h = Highlights::default();
        h.mark(Selection::node("srv").color(HighlightColor::Orange));
        let output = generate_mermaid(
            &graph,
            &GraphRenderOpts::default(),
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(output.contains("classDef hl_orange"));
        assert!(output.contains("class n1 hl_orange"));
    }

    #[test]
    fn highlight_edge_emits_linkstyle() {
        let graph = simple_chain_graph();
        let mut h = Highlights::default();
        h.register_tag(1, "drained").unwrap();
        h.mark(Selection::edge("wasi:http/handler@0.3.0::middleware->srv").tag(1));
        let output = generate_mermaid(
            &graph,
            &GraphRenderOpts::default(),
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(output.contains("linkStyle"));
        assert!(output.contains("handler⟦1⟧"));
        assert!(output.contains("Tags:"));
        assert!(output.contains("1 drained"));
    }

    #[test]
    fn show_host_imports_renders_cluster() {
        let graph = simple_chain_graph();
        let opts = GraphRenderOpts {
            show_host_imports: true,
            ..Default::default()
        };
        let output = generate_mermaid(&graph, &opts, Direction::LeftToRight, false, None);
        assert!(
            output.contains("subgraph host[\"Host Imports\"]"),
            "should emit host imports cluster, got:\n{output}"
        );
        // One node per host interface: handler (srv's) and log (middleware's).
        assert!(output.contains("host_wasi_http_handler_0_3_0"));
        assert!(output.contains("host_wasi_logging_log_0_1_0"));
        // Dashed edges from host nodes to the importing instances.
        assert!(output.contains("host_wasi_http_handler_0_3_0 -.->|\"handler\"| n1"));
        assert!(output.contains("host_wasi_logging_log_0_1_0 -.->|\"log\"| n2"));
    }

    #[test]
    fn show_host_imports_off_by_default() {
        let graph = simple_chain_graph();
        let output = mermaid_default(&graph);
        assert!(
            !output.contains("Host Imports"),
            "no host imports cluster by default, got:\n{output}"
        );
    }

    #[test]
    fn no_highlights_no_extra_styles() {
        let graph = simple_chain_graph();
        let output = mermaid_default(&graph);
        assert!(!output.contains("classDef hl_"));
        assert!(!output.contains("linkStyle"));
        assert!(!output.contains("Tags:"));
    }

    #[test]
    fn highlight_wins_over_shared_class() {
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
            &GraphRenderOpts::default(),
            Direction::LeftToRight,
            false,
            Some(&h),
        );
        assert!(output.contains("class n1 hl_yellow"));
        assert!(!output.contains("class n1 shared"));
    }
}
