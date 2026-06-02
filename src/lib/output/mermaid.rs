use crate::canonical_id::canonical_edge_id;
use crate::highlights::{format_tag_label, HighlightColor, Highlights};
use crate::model::{short_interface_name, CompositionGraph};
use crate::output::{
    build_all_interfaces_view, build_full_view, DetailLevel, Direction, SymbolMap,
};
use crate::subgraph::{compute_export_subgraphs, shared_instances};
use crate::{find_chain_interfaces, get_chain_for};
use std::collections::{BTreeMap, BTreeSet};

/// Generate a Mermaid diagram from the composition graph.
///
/// `highlights` is only honored by the [`DetailLevel::Graph`] path — the
/// other detail levels ignore it.  Pass `None` when no emphasis is wanted
/// (or when calling with a non-graph detail level).
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

/// Graph-shaped Mermaid: one `subgraph` cluster per top-level export, with
/// instances shared between clusters drawn once and styled with a thicker
/// border so the reader can see that two clusters reach the same instance.
///
/// This mirrors the structure of the ASCII graph view — sectioning by
/// export, request-flow-direction edges (caller → provider), and a "shared
/// instance" visual distinction — but lets Mermaid's layout engine do the
/// 2D placement.
fn generate_graph(
    graph: &CompositionGraph,
    direction: Direction,
    show_types: bool,
    highlights: Option<&Highlights>,
) -> String {
    let subgraphs = compute_export_subgraphs(graph);
    if subgraphs.is_empty() {
        // No exports — fall back to the flat AllInterfaces view.
        return generate_all_interfaces(graph, direction, show_types);
    }

    let shared = shared_instances(&subgraphs);

    let mut output = String::from(INIT_DIRECTIVE);
    output.push_str(&format!("graph {}\n", direction.to_mermaid()));

    // A class with a thicker stroke + dashed pattern marks instances that
    // appear in more than one subgraph.  Applied to the second-and-later
    // occurrences only (the first occurrence anchors the instance in its
    // owning cluster).
    output.push_str("    classDef shared stroke-width:3px,stroke-dasharray:5 3\n");
    // One classDef per highlight color in use.  Stays empty (no extra
    // bytes) when there are no highlights.
    if let Some(h) = highlights {
        for color in h.colors_used() {
            output.push_str(&format!(
                "    classDef hl_{} stroke:{},stroke-width:3px,color:{}\n",
                color.slug(),
                color.mermaid_hex(),
                color.mermaid_hex(),
            ));
        }
    }
    output.push('\n');

    let mut symbols = SymbolMap::new();
    let mut already_rendered: BTreeSet<u32> = BTreeSet::new();
    // Track (link_index, color) so we can emit `linkStyle` lines at the
    // end.  Mermaid identifies each `-->` by its zero-based insertion
    // order across the entire diagram.
    let mut link_index: usize = 0;
    let mut link_styles: Vec<(usize, HighlightColor)> = Vec::new();

    for sg in &subgraphs {
        let short = short_interface_name(&sg.interface_name);
        let sg_id = format!("sg_{}", sanitize_for_mermaid(&sg.interface_name));
        output.push_str(&format!("    subgraph {sg_id}[\"export: {short}\"]\n"));

        // Emit each node from this subgraph.  Nodes that already appeared in
        // an earlier subgraph aren't re-declared (Mermaid would treat that as
        // a redefinition) — we just refer to them by id and let cross-cluster
        // edges thread out to wherever they were first placed.
        for &idx in &sg.nodes {
            let Some(node) = graph.nodes.get(&idx) else {
                continue;
            };
            if already_rendered.contains(&idx) {
                continue;
            }
            let node_id = node_id_for(idx);
            let node_ctx_suffix = highlights
                .map(|h| format_tag_label(&h.node_tag_ids(node.canonical_id())))
                .unwrap_or_default();
            output.push_str(&format!(
                "        {}[\"{}{}\"]\n",
                node_id,
                escape_mermaid_label(node.display_label()),
                escape_mermaid_label(&node_ctx_suffix),
            ));
            // Highlight wins over shared (matches the ASCII renderer).
            let node_hl = highlights.and_then(|h| h.node_color(node.canonical_id()));
            if let Some(color) = node_hl {
                output.push_str(&format!("        class {} hl_{}\n", node_id, color.slug()));
            } else if shared.contains(&idx) && idx != sg.source_instance {
                output.push_str(&format!("        class {} shared\n", node_id));
            }
        }

        // Export entry: stadium-shaped marker pointing into the source node.
        let export_node = format!("ext_{}", sanitize_for_mermaid(&sg.interface_name));
        let sym = if show_types {
            graph
                .component_exports
                .get(sg.interface_name.as_str())
                .and_then(|info| symbols.symbol_for_export(info, &graph.arena))
                .map(str::to_string)
                .unwrap_or_default()
        } else {
            String::new()
        };
        let (export_hl, export_tag_ids) = highlights
            .and_then(|h| {
                graph.nodes.get(&sg.source_instance).map(|src| {
                    let id = canonical_edge_id(&sg.interface_name, None, src.canonical_id());
                    (h.edge_color(&id), h.edge_tag_ids(&id))
                })
            })
            .unwrap_or((None, Vec::new()));
        let export_ctx_suffix = format_tag_label(&export_tag_ids);
        output.push_str(&format!(
            "        {}([\"ext: {}{}{}\"]) --> {}\n",
            export_node,
            short,
            sym,
            escape_mermaid_label(&export_ctx_suffix),
            node_id_for(sg.source_instance),
        ));
        if let Some(color) = export_hl {
            link_styles.push((link_index, color));
        }
        link_index += 1;

        // Edges within this subgraph, merging parallel interfaces between the
        // same (caller, provider) pair into one labeled arrow — matches what
        // the ASCII view does.
        let mut by_pair: BTreeMap<(u32, u32), (Vec<String>, Option<HighlightColor>)> =
            BTreeMap::new();
        for e in &sg.edges {
            let label = short_interface_name(&e.interface);
            let symbol = if show_types {
                let fp = graph.nodes.get(&e.caller).and_then(|n| {
                    n.imports
                        .iter()
                        .find(|c| c.interface_name == e.interface)
                        .and_then(|c| c.fingerprint.as_deref())
                });
                let lines = graph
                    .nodes
                    .get(&e.caller)
                    .and_then(|n| {
                        n.imports
                            .iter()
                            .find(|c| c.interface_name == e.interface)
                            .and_then(|c| c.interface_type.as_ref())
                    })
                    .map(|it| crate::output::format_interface_type_lines(it, &graph.arena))
                    .unwrap_or_default();
                symbols.assign(true, fp, lines)
            } else {
                String::new()
            };
            // Per-interface highlight + context, computed against the
            // canonical edge ID assembled from caller/provider canonical
            // labels.
            let (iface_hl, iface_tag_ids) = highlights
                .and_then(|h| {
                    let caller = graph.nodes.get(&e.caller).map(|n| n.canonical_id());
                    let provider = graph.nodes.get(&e.provider).map(|n| n.canonical_id());
                    caller.zip(provider).map(|(c, p)| {
                        let id = canonical_edge_id(&e.interface, Some(c), p);
                        (h.edge_color(&id), h.edge_tag_ids(&id))
                    })
                })
                .unwrap_or((None, Vec::new()));
            let ctx_suffix = format_tag_label(&iface_tag_ids);
            let entry = by_pair.entry((e.caller, e.provider)).or_default();
            entry.0.push(format!("{label}{symbol}{ctx_suffix}"));
            // First non-None interface highlight wins the link's color (matches
            // the ASCII per-edge aggregation).
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
        already_rendered.extend(sg.nodes.iter().copied());
    }

    // Filter the tag list to contexts referenced by at least one matched
    // node or edge.  We rebuild present-id sets here from the subgraph
    // data we already walked — Mermaid renders Tag IDs alongside the
    // matching `[N]` brackets so dropping unmatched entries keeps the
    // list and the brackets consistent.
    let present_nodes: BTreeSet<String> = graph
        .nodes
        .values()
        .map(|n| n.canonical_id().to_string())
        .collect();
    let mut present_edges: BTreeSet<String> = BTreeSet::new();
    for sg in &subgraphs {
        for e in &sg.edges {
            let caller = graph.nodes.get(&e.caller).map(|n| n.canonical_id());
            let provider = graph.nodes.get(&e.provider).map(|n| n.canonical_id());
            if let (Some(c), Some(p)) = (caller, provider) {
                present_edges.insert(canonical_edge_id(&e.interface, Some(c), p));
            }
        }
        if let Some(src) = graph.nodes.get(&sg.source_instance) {
            present_edges.insert(canonical_edge_id(
                &sg.interface_name,
                None,
                src.canonical_id(),
            ));
        }
    }
    output.push_str(&render_key_with_tags(
        &symbols,
        highlights,
        &present_nodes,
        &present_edges,
    ));

    // Per-link styles at the very end.  Mermaid resolves these against
    // the edge order we just emitted; we tracked that as `link_index`.
    //
    // We only set `stroke` (line color) and `stroke-width`.  The
    // arrowhead is rendered by a single global `<marker>` SVG element
    // shared by every link, so it's not possible to recolor only one
    // arrow's head through `linkStyle` — `fill:` here would (mis)apply
    // to the path's fill area, not the marker.  The thicker stroke is
    // what carries the per-edge emphasis; arrowhead color stays default.
    // (If a user wants every arrowhead recolored globally, they can
    // pass an `init` directive with the `lineColor` theme variable.)
    for (idx, color) in link_styles {
        output.push_str(&format!(
            "    linkStyle {} stroke:{},stroke-width:3px\n",
            idx,
            color.mermaid_hex(),
        ));
    }
    output
}

fn node_id_for(idx: u32) -> String {
    format!("n{}", idx)
}

/// Like [`render_key`] but also appends a Tags section listing the
/// caller-supplied highlight contexts in insertion order.  Returns an
/// empty string when both the SymbolMap and the tag list are empty.
///
/// `present_nodes` / `present_edges` filter the tag list to contexts
/// referenced by at least one matched id — tags from typo'd or stale
/// `--highlight` ids are dropped so the rendered list and the in-diagram
/// `[N]` brackets stay consistent.
fn render_key_with_tags(
    symbols: &SymbolMap,
    highlights: Option<&Highlights>,
    present_nodes: &BTreeSet<String>,
    present_edges: &BTreeSet<String>,
) -> String {
    let tag_lines = highlights
        .map(|h| {
            h.tag_lines_referenced_by(
                present_nodes.iter().map(String::as_str),
                present_edges.iter().map(String::as_str),
            )
        })
        .unwrap_or_default();
    if symbols.is_empty() && tag_lines.is_empty() {
        return String::new();
    }
    let mut body_lines: Vec<String> = Vec::new();
    if !symbols.is_empty() {
        body_lines.push("Signatures:".to_string());
        for l in symbols.key_lines() {
            body_lines.push(preserve_leading_indent(&escape_mermaid_label(&l)));
        }
    }
    if !tag_lines.is_empty() {
        if !body_lines.is_empty() {
            body_lines.push(String::new());
        }
        body_lines.push("Tags:".to_string());
        for l in tag_lines {
            body_lines.push(preserve_leading_indent(&escape_mermaid_label(&l)));
        }
    }
    let body = body_lines.join("<br/>");
    let content = format!("<div style='text-align:left'>{body}</div>");
    let mut out = String::new();
    out.push_str(&format!("\n    key[\"{content}\"]\n"));
    out.push_str("    style key fill:none,stroke:none,color:#888\n");
    out
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
    // Newlines in Mermaid labels: use `<br/>` rather than `\n`.  Modern
    // Mermaid renders `<br/>` as a line break inside a node label; `\n`
    // either renders literally or breaks the marked.js parser depending on
    // version.  Escape the lines first, then join with raw `<br/>` so the
    // tag survives the escaper.
    let body = std::iter::once("Signatures:".to_string())
        .chain(
            symbols
                .key_lines()
                .into_iter()
                .map(|l| preserve_leading_indent(&escape_mermaid_label(&l))),
        )
        .collect::<Vec<_>>()
        .join("<br/>");
    // Wrap in a left-aligned `<div>` so the signatures column reads top-to-
    // bottom rather than each line being centred inside the node bounding
    // box (the default for Mermaid flowchart node labels).
    let content = format!("<div style='text-align:left'>{body}</div>");
    let mut out = String::new();
    out.push_str(&format!("\n    key[\"{content}\"]\n"));
    out.push_str("    style key fill:none,stroke:none,color:#888\n");
    out
}

/// Escape characters that Mermaid's default label parser (marked.js)
/// interprets as markdown/HTML and chokes on:
///   `<`/`>`  — read as HTML tags (`<resource>`, `<list<s32>>`)
///   `` ` ``  — inline code spans (`` `handle` ``)
///   `[`/`]`  — markdown link syntax (`[constructor]counter` → invalid link)
///
/// `<`/`>` and `` ` `` use HTML entity / apostrophe substitution.  Brackets
/// are swapped to Unicode lookalikes (`⟦` U+27E6, `⟧` U+27E7) rather than
/// `&#91;` / `&#93;`, because some markdown-renders-then-Mermaid pipelines
/// (GitHub, certain MkDocs setups) strip the numeric entities before
/// Mermaid sees them and the label ends up displaying as `&[N&]`.  The
/// lookalikes survive every pipeline because they're just plain text.
fn escape_mermaid_label(s: &str) -> String {
    s.replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('`', "'")
        .replace('[', "⟦")
        .replace(']', "⟧")
}

/// HTML collapses runs of whitespace, so a leading "  " indent on a
/// continuation line in the Signatures key disappears once Mermaid renders
/// it.  Substitute leading spaces with `&nbsp;` so the indent survives.
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

/// Mermaid init directive that widens the text-wrapping threshold.
///
/// Mermaid auto-wraps node label text at ~200 px by default.  We bump it
/// substantially so the Signatures key — the widest node in any diagram —
/// gets to use the full horizontal real estate of the rendered graph
/// rather than wrapping at an arbitrary mid-line column.
const INIT_DIRECTIVE: &str = "%%{init: {'flowchart': {'wrappingWidth': 2400}}}%%\n";

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
    use crate::highlights::Selection;
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
            is_async: false,
            param_names: vec![],
            params: vec![u32_id],
            results: vec![bool_id],
        };
        let mut functions = BTreeMap::new();
        functions.insert("handle".to_string(), handle_sig);
        let iface_type = InterfaceType::Instance(InstanceInterface {
            functions,
            type_exports: BTreeMap::new(),
        });

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
        let graph = test_graph();
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
        let graph = test_graph();
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
        let graph = test_graph_with_types();
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
        let graph = test_graph_with_types();
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
        let graph = test_graph_with_types();
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
        let mut h = Highlights::new();
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
        let mut h = Highlights::new();
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
        let mut h = Highlights::new();
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

        let mut h = Highlights::new();
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
