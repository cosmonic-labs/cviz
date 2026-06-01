//! Graph-shaped ASCII rendering, per export-reachable subgraph.
//!
//! The composition is split into one [`ExportSubgraph`](crate::ExportSubgraph)
//! per top-level export.  Each subgraph is rendered as its own block with a
//! section header naming the export; instances that appear in more than one
//! subgraph get a double-line border in the second-and-subsequent occurrence
//! so the reader can see "this is the same instance, just reached two
//! different ways."  Sharing is decided by instance index (`u32`), not
//! display name.

use crate::model::CompositionGraph;
use crate::output::SymbolMap;
use crate::subgraph::{compute_export_subgraphs, shared_instances, ExportSubgraph, SubgraphEdge};
use std::collections::{BTreeMap, BTreeSet};

/// Result of [`generate_graph_ascii`].
///
/// `ascii` is the rendered diagram.  `condensed` is `true` when the renderer
/// had to compromise legibility to fit the requested width — either a node
/// name was truncated or the rendered diagram would have exceeded the caller's
/// `max_width`.  Callers (the CLI) can use the flag to surface a hint, e.g.
/// "this diagram may render better with `-f mermaid`."
#[derive(Debug, Clone, Default)]
pub struct GraphAsciiOutput {
    pub ascii: String,
    pub condensed: bool,
}

/// Maximum width for a node label.  Adapter names from WAC compositions can
/// exceed 60 characters; everything wider than this is middle-truncated with
/// an ellipsis.
pub const MAX_NODE_LABEL: usize = 28;

/// Generate a graph-shaped ASCII diagram from the composition graph.
///
/// One section is rendered per top-level export, in interface-name order.
/// Instances shared between multiple sections render with a double-line
/// border in the second-and-subsequent occurrence so the reader can see
/// "this is the same instance, just reached two different ways."
///
/// If the composition exports nothing, all real nodes connected by
/// inter-component edges are rendered as a single unnamed block (mostly
/// useful for test fixtures).
pub fn generate_graph_ascii(
    graph: &CompositionGraph,
    show_types: bool,
    max_width: Option<usize>,
) -> GraphAsciiOutput {
    let mut subgraphs = compute_export_subgraphs(graph);
    if subgraphs.is_empty() {
        if let Some(fallback) = fallback_subgraph(graph) {
            subgraphs.push(fallback);
        }
    }
    if subgraphs.is_empty() {
        return GraphAsciiOutput {
            ascii: "No component instances found".to_string(),
            condensed: false,
        };
    }

    // Pre-compute the set of instances appearing in two or more subgraphs;
    // those become candidates for the "shared" double-line treatment.  The
    // root of each subgraph never gets the treatment (it's that block's
    // visual anchor), so the rendering loop excludes the current root.
    let shared = shared_instances(&subgraphs);

    // Shared SymbolMap so type signatures are collected from every subgraph
    // and listed once at the bottom.
    let mut symbols = SymbolMap::new();
    let mut already_rendered: BTreeSet<u32> = BTreeSet::new();
    let mut sections: Vec<String> = Vec::new();
    let mut any_truncated = false;
    let mut any_exceeded = false;

    for sg in &subgraphs {
        // Only mark a node as "shared" (double-line) if it has appeared in
        // an earlier subgraph AND is not this subgraph's root.
        let mut shared_here: BTreeSet<u32> = sg
            .nodes
            .iter()
            .copied()
            .filter(|n| shared.contains(n) && already_rendered.contains(n))
            .collect();
        shared_here.remove(&sg.source_instance);

        let RenderedBlock {
            ascii,
            truncated,
            exceeded,
        } = render_subgraph(graph, sg, &shared_here, &mut symbols, show_types, max_width);
        any_truncated |= truncated;
        any_exceeded |= exceeded;

        let header = section_header(&sg.interface_name);
        if let Some(h) = header {
            sections.push(format!("{h}\n\n{ascii}"));
        } else {
            sections.push(ascii);
        }

        already_rendered.extend(sg.nodes.iter().copied());
    }

    let mut out = sections.join("\n\n\n");
    if !symbols.is_empty() {
        out.push_str("\n\n");
        out.push_str("Signatures:\n");
        for line in symbols.key_lines() {
            out.push_str("  ");
            out.push_str(&line);
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
    }

    GraphAsciiOutput {
        ascii: out,
        condensed: any_truncated || any_exceeded,
    }
}

/// Middle-truncate a label to fit within `max` display columns.
fn truncate_name(s: &str, max: usize) -> (String, bool) {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return (s.to_string(), false);
    }
    let keep = max.saturating_sub(1);
    let prefix_len = keep / 2;
    let suffix_len = keep - prefix_len;
    let mut out: String = chars[..prefix_len].iter().collect();
    out.push('…');
    let tail_start = chars.len() - suffix_len;
    for c in &chars[tail_start..] {
        out.push(*c);
    }
    (out, true)
}

fn section_header(interface_name: &str) -> Option<String> {
    if interface_name.is_empty() {
        return None;
    }
    let short = crate::model::short_interface_name(interface_name);
    Some(format!("╞══ {short} {}", "═".repeat(40)))
}

/// When the composition has no exports, treat all real nodes connected by
/// any inter-component edge as one anonymous block.
fn fallback_subgraph(graph: &CompositionGraph) -> Option<ExportSubgraph> {
    use crate::model::SYNTHETIC_COMPONENT;
    let mut nodes: BTreeSet<u32> = BTreeSet::new();
    for (caller_idx, caller) in &graph.nodes {
        if caller.component_index == SYNTHETIC_COMPONENT {
            continue;
        }
        for import in &caller.imports {
            if import.is_host_import {
                continue;
            }
            let Some(provider_idx) = import.source_instance else {
                continue;
            };
            let Some(provider) = graph.nodes.get(&provider_idx) else {
                continue;
            };
            if provider.component_index == SYNTHETIC_COMPONENT {
                continue;
            }
            nodes.insert(*caller_idx);
            nodes.insert(provider_idx);
        }
    }
    if nodes.is_empty() {
        return None;
    }
    let mut edges = Vec::new();
    for &caller_idx in &nodes {
        let Some(caller) = graph.nodes.get(&caller_idx) else {
            continue;
        };
        for import in &caller.imports {
            if import.is_host_import {
                continue;
            }
            let Some(provider_idx) = import.source_instance else {
                continue;
            };
            if !nodes.contains(&provider_idx) {
                continue;
            }
            edges.push(SubgraphEdge {
                caller: caller_idx,
                provider: provider_idx,
                interface: import.interface_name.clone(),
            });
        }
    }
    let source_instance = *nodes.iter().next().unwrap();
    Some(ExportSubgraph {
        interface_name: String::new(),
        source_instance,
        nodes,
        edges,
    })
}

// ---------------------------------------------------------------------------
// Per-subgraph rendering
// ---------------------------------------------------------------------------

struct RenderedBlock {
    ascii: String,
    truncated: bool,
    exceeded: bool,
}

fn render_subgraph(
    graph: &CompositionGraph,
    sg: &ExportSubgraph,
    shared_here: &BTreeSet<u32>,
    symbols: &mut SymbolMap,
    show_types: bool,
    max_width: Option<usize>,
) -> RenderedBlock {
    let layout = layout_subgraph(graph, sg, show_types);
    let r = render(&layout, shared_here, symbols, max_width);
    let exceeded = max_width.is_some_and(|w| r.natural_width > w);
    RenderedBlock {
        ascii: r.ascii,
        truncated: layout.any_truncated,
        exceeded,
    }
}

// ---------------------------------------------------------------------------
// Layout model — keyed by instance index (u32)
// ---------------------------------------------------------------------------

struct Layout {
    /// Outer index = rank (column, left → right).  Inner = node instance
    /// indices in that rank (top → bottom).
    ranks: Vec<Vec<u32>>,
    /// All inter-component edges in (caller, provider) form (already in
    /// request-flow direction).  Parallel interfaces between the same pair
    /// are merged into a single edge whose `interfaces` vec carries them.
    edges: Vec<Edge>,
    /// One [`Export`] for this subgraph's externally-callable interface.
    /// May be empty (anonymous fallback block).
    exports: Vec<Export>,
    /// Per-instance rendering info (display label, possibly truncated).
    nodes: BTreeMap<u32, NodeInfo>,
    /// True iff any node label was shortened to fit [`MAX_NODE_LABEL`].
    any_truncated: bool,
}

#[derive(Clone)]
struct NodeInfo {
    /// The label to draw inside the box (after truncation).
    label: String,
}

struct Edge {
    from: u32,
    to: u32,
    interfaces: Vec<Interface>,
    rendered_label: String,
}

#[derive(Clone)]
struct Interface {
    label: String,
    fingerprint: Option<String>,
    type_lines: Vec<String>,
}

struct Export {
    provider: u32,
    label: String,
    fingerprint: Option<String>,
    type_lines: Vec<String>,
    rendered_label: String,
}

fn layout_subgraph(graph: &CompositionGraph, sg: &ExportSubgraph, show_types: bool) -> Layout {
    // Merge parallel edges between the same (caller, provider) pair.
    let mut by_pair: BTreeMap<(u32, u32), Edge> = BTreeMap::new();
    for e in &sg.edges {
        let iface_type_lines = if show_types {
            interface_type_lines(graph, e.caller, &e.interface)
        } else {
            Vec::new()
        };
        let fingerprint = if show_types {
            interface_fingerprint(graph, e.caller, &e.interface)
        } else {
            None
        };
        let iface = Interface {
            label: crate::model::short_interface_name(&e.interface),
            fingerprint,
            type_lines: iface_type_lines,
        };
        by_pair
            .entry((e.caller, e.provider))
            .and_modify(|existing| existing.interfaces.push(iface.clone()))
            .or_insert_with(|| Edge {
                from: e.caller,
                to: e.provider,
                interfaces: vec![iface],
                rendered_label: String::new(),
            });
    }
    let edges: Vec<Edge> = by_pair.into_values().collect();

    // Build node info.  Every node in the subgraph is included even if it has
    // no edges — the subgraph's source might be a lone box for a tiny chain.
    let mut nodes: BTreeMap<u32, NodeInfo> = BTreeMap::new();
    let mut any_truncated = false;
    for &idx in &sg.nodes {
        let Some(node) = graph.nodes.get(&idx) else {
            continue;
        };
        let (label, truncated) = truncate_name(node.display_label(), MAX_NODE_LABEL);
        if truncated {
            any_truncated = true;
        }
        nodes.insert(idx, NodeInfo { label });
    }
    let node_ids: BTreeSet<u32> = nodes.keys().copied().collect();

    let ranks = compute_ranks(&node_ids, &edges, sg.source_instance);

    // Export entry: only when the subgraph names an interface.
    let exports = if sg.interface_name.is_empty() || !nodes.contains_key(&sg.source_instance) {
        Vec::new()
    } else {
        let (type_lines, fingerprint) = if show_types {
            export_type_info(graph, &sg.interface_name)
        } else {
            (Vec::new(), None)
        };
        vec![Export {
            provider: sg.source_instance,
            label: crate::model::short_interface_name(&sg.interface_name),
            fingerprint,
            type_lines,
            rendered_label: String::new(),
        }]
    };

    Layout {
        ranks,
        edges,
        exports,
        nodes,
        any_truncated,
    }
}

fn interface_type_lines(
    graph: &CompositionGraph,
    caller: u32,
    interface_name: &str,
) -> Vec<String> {
    use crate::output::format_interface_type_lines;
    graph
        .nodes
        .get(&caller)
        .and_then(|n| {
            n.imports
                .iter()
                .find(|c| c.interface_name == interface_name)
                .and_then(|c| c.interface_type.as_ref())
                .map(|t| format_interface_type_lines(t, &graph.arena))
        })
        .unwrap_or_default()
}

fn interface_fingerprint(
    graph: &CompositionGraph,
    caller: u32,
    interface_name: &str,
) -> Option<String> {
    graph.nodes.get(&caller).and_then(|n| {
        n.imports
            .iter()
            .find(|c| c.interface_name == interface_name)
            .and_then(|c| c.fingerprint.clone())
    })
}

fn export_type_info(
    graph: &CompositionGraph,
    interface_name: &str,
) -> (Vec<String>, Option<String>) {
    use crate::output::format_interface_type_lines;
    let Some(info) = graph.component_exports.get(interface_name) else {
        return (Vec::new(), None);
    };
    let type_lines = match info.ty {
        Some(crate::model::InternedId::Interface(id)) => {
            format_interface_type_lines(graph.arena.lookup_interface(id), &graph.arena)
        }
        _ => Vec::new(),
    };
    (type_lines, info.fingerprint.clone())
}

/// Assign each node a rank via longest-path-from-source.
///
/// The subgraph root is anchored at rank 0 so the export arrow lands on the
/// leftmost column.  Other nodes get their rank from the longest predecessor
/// path; cycles are bounded by iterating at most `nodes.len()` rounds.
fn compute_ranks(nodes: &BTreeSet<u32>, edges: &[Edge], root: u32) -> Vec<Vec<u32>> {
    let mut rank: BTreeMap<u32, usize> = nodes.iter().map(|n| (*n, 0)).collect();
    rank.insert(root, 0);
    for _ in 0..nodes.len() {
        let mut changed = false;
        for e in edges {
            let next = rank.get(&e.from).copied().unwrap_or(0) + 1;
            let cur = rank.get(&e.to).copied().unwrap_or(0);
            if next > cur {
                rank.insert(e.to, next);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let max_rank = rank.values().copied().max().unwrap_or(0);
    let mut ranks: Vec<Vec<u32>> = vec![Vec::new(); max_rank + 1];
    for (id, r) in &rank {
        ranks[*r].push(*id);
    }
    for r in &mut ranks {
        r.sort();
    }
    ranks
}

// ---------------------------------------------------------------------------
// Rendering geometry
// ---------------------------------------------------------------------------

struct Geom {
    col_x: Vec<usize>,
    col_w: Vec<usize>,
    node_top: BTreeMap<u32, usize>,
    node_mid: BTreeMap<u32, usize>,
    width: usize,
    height: usize,
    bend_x_left: Vec<usize>,
}

const NODE_VPAD: usize = 2;
const BOX_HEIGHT: usize = 3;
const GUTTER_MIN: usize = 8;
/// Blank rows inserted between two sibling subtrees in the tree-aware Y
/// placement.  Keeps adjacent branches visually separated so the reader can
/// tell where one subtree ends and the next begins.
const INTER_SUBTREE_GAP: usize = 2;

/// Tree-aware DFS placement of one subtree.  `cursor` is the y at which this
/// subtree starts (top of the first child's first row).  Returns the y just
/// past this subtree's bottom so callers can place the next sibling there.
///
/// The placed node's y is set to centre on its children's mid rows; leaves
/// land at `cursor`.  For DAGs, the first encounter wins — secondary parents
/// of a shared node draw their edge into the already-placed position.
fn place_subtree(
    node: u32,
    cursor: usize,
    children_of: &BTreeMap<u32, Vec<u32>>,
    placed: &mut BTreeSet<u32>,
    node_top: &mut BTreeMap<u32, usize>,
    node_mid: &mut BTreeMap<u32, usize>,
) -> usize {
    if placed.contains(&node) {
        return cursor;
    }
    placed.insert(node);

    let children: Vec<u32> = children_of
        .get(&node)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| !placed.contains(c))
        .collect();

    if children.is_empty() {
        node_top.insert(node, cursor);
        node_mid.insert(node, cursor + 1);
        return cursor + BOX_HEIGHT + NODE_VPAD;
    }

    let mut child_mids: Vec<usize> = Vec::new();
    let mut current = cursor;
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            current += INTER_SUBTREE_GAP;
        }
        let after = place_subtree(*child, current, children_of, placed, node_top, node_mid);
        if let Some(&cm) = node_mid.get(child) {
            child_mids.push(cm);
        }
        current = after;
    }

    // Parent aligns with the FIRST child's row so the first outgoing edge
    // reads as a straight line and subsequent ones as bent-down branches.
    // This produces the canonical trunk-and-branch shape — `┬` at the
    // parent's row, `├` on each intermediate branch, `└` on the bottom
    // branch — without any extra drawing logic; the direction merger
    // composes those glyphs automatically when straight + bent edges
    // overlap in the same gutter cell.
    let my_mid = child_mids.first().copied().unwrap_or(cursor + 1);
    let my_top = my_mid.saturating_sub(1);
    node_top.insert(node, my_top);
    node_mid.insert(node, my_mid);
    current
}

fn leading_pad_width(layout: &Layout) -> usize {
    let max_label_width = layout
        .exports
        .iter()
        .map(|e| 4 + e.rendered_label.chars().count() + 5)
        .max()
        .unwrap_or(0);
    std::cmp::max(4, max_label_width)
}

fn geom(layout: &Layout, leading_pad: usize) -> Geom {
    let col_w: Vec<usize> = layout
        .ranks
        .iter()
        .map(|rank| {
            rank.iter()
                .map(|id| layout.nodes[id].label.chars().count() + 4)
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut gutter_w: Vec<usize> = vec![GUTTER_MIN; layout.ranks.len().saturating_sub(1)];
    let mut caller_count: BTreeMap<u32, usize> = BTreeMap::new();
    let mut targets_per_source: BTreeMap<u32, usize> = BTreeMap::new();
    for e in &layout.edges {
        *caller_count.entry(e.to).or_insert(0) += 1;
        *targets_per_source.entry(e.from).or_insert(0) += 1;
    }
    for e in &layout.edges {
        let (rf, rt) = ranks_of(layout, e.from, e.to);
        if rt == rf + 1 {
            let label_w = e.rendered_label.chars().count();
            let is_fan_in = caller_count.get(&e.to).copied().unwrap_or(0) > 1;
            let is_fan_out = targets_per_source.get(&e.from).copied().unwrap_or(0) > 1;
            let needed = if is_fan_in {
                2 * (label_w + 4)
            } else if is_fan_out {
                label_w + 8
            } else {
                label_w + 6
            };
            if needed > gutter_w[rf] {
                gutter_w[rf] = needed;
            }
        }
    }

    let mut col_x = Vec::with_capacity(layout.ranks.len());
    let mut x = leading_pad;
    for (i, w) in col_w.iter().enumerate() {
        col_x.push(x);
        x += w;
        if i < gutter_w.len() {
            x += gutter_w[i];
        }
    }
    let width = x;

    let bend_x_left: Vec<usize> = (0..layout.ranks.len())
        .map(|r| {
            if r == 0 {
                leading_pad / 2
            } else {
                let prev_right = col_x[r - 1] + col_w[r - 1];
                prev_right + gutter_w[r - 1] / 2
            }
        })
        .collect();

    // Tree-aware Y placement.  Each subtree (a node and everything that's
    // unique to its downstream) occupies a contiguous vertical band.  Sibling
    // subtrees stack with a gap between them so adjacent branches' downstream
    // chains never interleave columns.
    //
    // The X column is still determined by rank (the longest-path-from-source
    // value); only Y assignment changes here.
    let mut children_of: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for e in &layout.edges {
        children_of.entry(e.from).or_default().push(e.to);
    }
    let mut has_parent: BTreeSet<u32> = BTreeSet::new();
    for e in &layout.edges {
        has_parent.insert(e.to);
    }
    let mut roots: Vec<u32> = layout
        .nodes
        .keys()
        .copied()
        .filter(|n| !has_parent.contains(n))
        .collect();
    roots.sort();

    let mut node_top: BTreeMap<u32, usize> = BTreeMap::new();
    let mut node_mid: BTreeMap<u32, usize> = BTreeMap::new();
    let mut placed: BTreeSet<u32> = BTreeSet::new();
    let mut cursor: usize = 0;

    for (i, root) in roots.iter().copied().enumerate() {
        if i > 0 {
            cursor += INTER_SUBTREE_GAP * 2;
        }
        cursor = place_subtree(
            root,
            cursor,
            &children_of,
            &mut placed,
            &mut node_top,
            &mut node_mid,
        );
    }

    // Fallback for any node that wasn't reached by tree walk (shouldn't
    // happen for well-formed ExportSubgraphs but guards against orphans).
    let mut next_y_per_rank: Vec<usize> = vec![0; layout.ranks.len()];
    for (r, rank) in layout.ranks.iter().enumerate() {
        for &id in rank {
            if let Some(&top) = node_top.get(&id) {
                let bottom = top + BOX_HEIGHT;
                if bottom > next_y_per_rank[r] {
                    next_y_per_rank[r] = bottom + NODE_VPAD;
                }
            }
        }
    }
    for (r, rank) in layout.ranks.iter().enumerate() {
        for &id in rank {
            if placed.contains(&id) {
                continue;
            }
            let top = next_y_per_rank[r];
            node_top.insert(id, top);
            node_mid.insert(id, top + 1);
            next_y_per_rank[r] = top + BOX_HEIGHT + NODE_VPAD;
            placed.insert(id);
        }
    }

    let height = node_top
        .values()
        .map(|&y| y + BOX_HEIGHT)
        .max()
        .unwrap_or(0);

    Geom {
        col_x,
        col_w,
        node_top,
        node_mid,
        width,
        height,
        bend_x_left,
    }
}

fn ranks_of(layout: &Layout, from: u32, to: u32) -> (usize, usize) {
    let mut rf = 0;
    let mut rt = 0;
    for (r, rank) in layout.ranks.iter().enumerate() {
        for &id in rank {
            if id == from {
                rf = r;
            }
            if id == to {
                rt = r;
            }
        }
    }
    (rf, rt)
}

fn rank_of(layout: &Layout, id: u32) -> usize {
    for (r, rank) in layout.ranks.iter().enumerate() {
        if rank.contains(&id) {
            return r;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

struct RenderedSubgraph {
    ascii: String,
    /// Width the diagram *would* have had without wrapping.  Callers compare
    /// this against their max_width to decide whether to flag the output as
    /// "condensed" regardless of whether wrap actually fired.
    natural_width: usize,
}

fn render(
    layout: &Layout,
    shared_here: &BTreeSet<u32>,
    symbols: &mut SymbolMap,
    wrap_max_width: Option<usize>,
) -> RenderedSubgraph {
    // Assign labels (with type symbols when present) up front so the gutter
    // sizing computed by `geom` accommodates the final label width.  The
    // SymbolMap is shared across subgraphs by the caller, so identical
    // fingerprints get identical symbols even across blocks.
    //
    // `assign(show_types=true, ...)` returns the empty string when there's
    // no fingerprint (which is what `layout_subgraph` produces when types
    // are disabled), so this loop is correct in both modes.
    let mut sized = layout.clone_shallow();
    for exp in sized.exports.iter_mut() {
        let sym = symbols.assign(true, exp.fingerprint.as_deref(), exp.type_lines.clone());
        exp.rendered_label = if sym.is_empty() {
            exp.label.clone()
        } else {
            format!("{}{}", exp.label, sym)
        };
    }
    for edge in sized.edges.iter_mut() {
        let parts: Vec<String> = edge
            .interfaces
            .iter()
            .map(|iface| {
                let sym =
                    symbols.assign(true, iface.fingerprint.as_deref(), iface.type_lines.clone());
                if sym.is_empty() {
                    iface.label.clone()
                } else {
                    format!("{}{}", iface.label, sym)
                }
            })
            .collect();
        edge.rendered_label = parts.join(",");
    }

    let leading_pad = leading_pad_width(&sized);
    let g = geom(&sized, leading_pad);
    let natural_width = g.width;

    let mut grid: Vec<Vec<char>> = vec![vec![' '; g.width]; g.height];
    for rank in sized.ranks.iter() {
        for &id in rank {
            let y0 = g.node_top[&id];
            let r = rank_of(&sized, id);
            let x0 = g.col_x[r];
            let w = g.col_w[r];
            let is_shared = shared_here.contains(&id);
            draw_box(&mut grid, x0, y0, w, &sized.nodes[&id].label, is_shared);
        }
    }

    let mut by_target: BTreeMap<u32, Vec<&Edge>> = BTreeMap::new();
    for e in &sized.edges {
        by_target.entry(e.to).or_default().push(e);
    }
    for (to, group) in by_target {
        draw_edge_group(&mut grid, &sized, &g, to, &group);
    }
    for exp in &sized.exports {
        draw_export(&mut grid, &sized, &g, exp);
    }

    let needs_wrap = wrap_max_width.is_some_and(|w| g.width > w);
    let ascii = if needs_wrap {
        wrap_grid_into_bands(&grid, &g, wrap_max_width.unwrap())
    } else {
        grid_to_string(&grid)
    };

    RenderedSubgraph {
        ascii,
        natural_width,
    }
}

const WRAP_INDENT_NORMAL: &str = "    ";
const WRAP_INDENT_INCOMING: &str = "↪   ";

fn wrap_grid_into_bands(grid: &[Vec<char>], g: &Geom, max_width: usize) -> String {
    let band_ranges = compute_band_ranges(g, max_width);
    if band_ranges.len() <= 1 {
        return grid_to_string(grid);
    }
    let mut lines: Vec<String> = Vec::new();
    for (band_idx, (start, end)) in band_ranges.iter().copied().enumerate() {
        let is_first = band_idx == 0;
        let is_last = band_idx + 1 == band_ranges.len();
        let mut band_rows: Vec<String> = Vec::new();
        for row in grid {
            let outgoing = !is_last && end > 0 && matches!(row[end - 1], '─' | '▶');
            let incoming = !is_first && start > 0 && matches!(row[start - 1], '─' | '▶');
            let mut chars: Vec<char> = row[start..end].to_vec();
            if outgoing && !chars.is_empty() {
                chars.push(' ');
                chars.push('↩');
            }
            let prefix = if is_first {
                ""
            } else if incoming {
                WRAP_INDENT_INCOMING
            } else {
                WRAP_INDENT_NORMAL
            };
            let line = format!("{}{}", prefix, chars.iter().collect::<String>());
            band_rows.push(line.trim_end().to_string());
        }
        while band_rows.last().is_some_and(|l| l.is_empty()) {
            band_rows.pop();
        }
        while band_rows.first().is_some_and(|l| l.is_empty()) {
            band_rows.remove(0);
        }
        if band_rows.is_empty() {
            continue;
        }
        if !is_first {
            lines.push(String::new());
        }
        lines.extend(band_rows);
    }
    lines.join("\n")
}

fn compute_band_ranges(g: &Geom, max_width: usize) -> Vec<(usize, usize)> {
    let n = g.col_x.len();
    if n == 0 {
        return vec![(0, g.width)];
    }
    let mut bands: Vec<(usize, usize)> = Vec::new();
    let mut band_left: usize = 0;
    let mut current_rank: usize = 0;
    while current_rank < n {
        let mut last_fit = current_rank;
        for j in (current_rank + 1)..n {
            let candidate_right = g.col_x[j] + g.col_w[j];
            if candidate_right.saturating_sub(band_left) <= max_width {
                last_fit = j;
            } else {
                break;
            }
        }
        let end_col = if last_fit + 1 < n {
            g.col_x[last_fit + 1]
        } else {
            g.width
        };
        bands.push((band_left, end_col));
        band_left = end_col;
        current_rank = last_fit + 1;
    }
    bands
}

impl Layout {
    fn clone_shallow(&self) -> Layout {
        Layout {
            ranks: self.ranks.clone(),
            edges: self
                .edges
                .iter()
                .map(|e| Edge {
                    from: e.from,
                    to: e.to,
                    interfaces: e.interfaces.clone(),
                    rendered_label: e.rendered_label.clone(),
                })
                .collect(),
            exports: self
                .exports
                .iter()
                .map(|x| Export {
                    provider: x.provider,
                    label: x.label.clone(),
                    fingerprint: x.fingerprint.clone(),
                    type_lines: x.type_lines.clone(),
                    rendered_label: x.rendered_label.clone(),
                })
                .collect(),
            nodes: self.nodes.clone(),
            any_truncated: self.any_truncated,
        }
    }
}

/// Draw a single-line or double-line box.  `shared = true` switches to the
/// double-line border so the reader can see "this instance is rendered
/// elsewhere too."
fn draw_box(grid: &mut [Vec<char>], x: usize, y: usize, w: usize, label: &str, shared: bool) {
    let (tl, tr, bl, br, h, v) = if shared {
        ('╔', '╗', '╚', '╝', '═', '║')
    } else {
        ('┌', '┐', '└', '┘', '─', '│')
    };
    grid[y][x] = tl;
    for i in 1..w - 1 {
        grid[y][x + i] = h;
    }
    grid[y][x + w - 1] = tr;

    grid[y + 1][x] = v;
    let label_chars: Vec<char> = label.chars().collect();
    let inner_w = w - 2;
    let pad_left = (inner_w - label_chars.len()) / 2;
    for (i, c) in label_chars.iter().enumerate() {
        grid[y + 1][x + 1 + pad_left + i] = *c;
    }
    grid[y + 1][x + w - 1] = v;

    grid[y + 2][x] = bl;
    for i in 1..w - 1 {
        grid[y + 2][x + i] = h;
    }
    grid[y + 2][x + w - 1] = br;
}

fn draw_edge_group(grid: &mut [Vec<char>], layout: &Layout, g: &Geom, to: u32, group: &[&Edge]) {
    let target_rank = rank_of(layout, to);
    let target_left = g.col_x[target_rank];
    let target_mid = g.node_mid[&to];
    let bend_x = g.bend_x_left[target_rank];

    if group.len() == 1 {
        let e = group[0];
        let from_rank = rank_of(layout, e.from);
        let from_right = g.col_x[from_rank] + g.col_w[from_rank] - 1;
        let from_mid = g.node_mid[&e.from];
        let single_bend_x = if from_mid == target_mid {
            bend_x
        } else {
            // 3 cells past the source's right border keeps the bend bar
            // clearly off the previous rank's box (rather than visually
            // touching it) while still leaving the rest of the gutter for
            // a labeled horizontal edge.
            std::cmp::min(from_right + 3, bend_x)
        };
        draw_single_edge(
            grid,
            from_right + 1,
            from_mid,
            target_left - 1,
            target_mid,
            single_bend_x,
            &e.rendered_label,
        );
        return;
    }

    let mut arm_mids: Vec<usize> = group.iter().map(|e| g.node_mid[&e.from]).collect();
    arm_mids.sort();
    let top_arm = *arm_mids.first().unwrap();
    let bottom_arm = *arm_mids.last().unwrap();
    for row in grid.iter_mut().take(bottom_arm + 1).skip(top_arm) {
        if row[bend_x] == ' ' {
            row[bend_x] = '│';
        }
    }
    for e in group {
        let from_rank = rank_of(layout, e.from);
        let from_right = g.col_x[from_rank] + g.col_w[from_rank] - 1;
        let from_mid = g.node_mid[&e.from];
        draw_horizontal(
            grid,
            from_right + 1,
            from_mid,
            bend_x - 1,
            &e.rendered_label,
        );
        if from_mid == target_mid {
            grid[from_mid][bend_x] = '┼';
        } else if from_mid == top_arm {
            grid[from_mid][bend_x] = '┐';
        } else if from_mid == bottom_arm {
            grid[from_mid][bend_x] = '┘';
        } else {
            grid[from_mid][bend_x] = '┤';
        }
    }
    for cell in grid[target_mid]
        .iter_mut()
        .take(target_left)
        .skip(bend_x + 1)
    {
        if *cell == ' ' {
            *cell = '─';
        }
    }
    if target_left > 0 {
        grid[target_mid][target_left - 1] = '▶';
    }
}

fn draw_single_edge(
    grid: &mut [Vec<char>],
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    bend_x: usize,
    label: &str,
) {
    if y0 == y1 {
        draw_horizontal(grid, x0, y0, x1, label);
        grid[y0][x1] = '▶';
        return;
    }
    draw_horizontal(grid, x0, y0, bend_x - 1, "");

    // Source-row corner — line came from the LEFT (the source-row dashes)
    // and turns DOWN (if y1 > y0) or UP (if y1 < y0).  When a sibling bent
    // edge has already drawn a corner here for the opposite direction, we
    // need a T-junction (`┤`).
    grid[y0][bend_x] = merge_dirs(
        grid[y0][bend_x],
        DIR_LEFT | if y1 > y0 { DIR_DOWN } else { DIR_UP },
    );

    // Vertical bar through the gap.  Strictly between the source-row and
    // target-row corners — writing UP|DOWN into the corner cell itself would
    // upgrade `└` to `├` (vertical-and-right), making the last branch look
    // like the trunk continues past it when it doesn't.  Sibling bent edges
    // whose targets are deeper will write their own bar through this row and
    // correctly upgrade the corner via their own merge.
    if y0 < y1 && y0 + 2 <= y1 {
        for row in grid.iter_mut().take(y1).skip(y0 + 1) {
            row[bend_x] = merge_dirs(row[bend_x], DIR_UP | DIR_DOWN);
        }
    } else if y0 > y1 && y1 + 2 <= y0 {
        for row in grid.iter_mut().take(y0).skip(y1 + 1) {
            row[bend_x] = merge_dirs(row[bend_x], DIR_UP | DIR_DOWN);
        }
    }

    // Target-row corner — line continues from the vertical bar and turns
    // RIGHT toward the target box.  If another sibling's bar also passes
    // through this cell (because they share the bend column and this edge's
    // target is intermediate, not the deepest), the previous bar's `│` here
    // gets upgraded to `├` so the line reads as continuing past.
    let arrives_from = if y1 > y0 { DIR_UP } else { DIR_DOWN };
    grid[y1][bend_x] = merge_dirs(grid[y1][bend_x], arrives_from | DIR_RIGHT);

    if bend_x + 1 < x1 {
        draw_horizontal(grid, bend_x + 1, y1, x1 - 1, label);
    }
    grid[y1][x1] = '▶';
}

// ---------------------------------------------------------------------------
// Box-drawing direction merger.
//
// When two edges share a column or row, the existing glyph in a cell may
// already carry some directions; the new edge wants to add more.  We
// decompose to direction bits, OR them, and pick the glyph that matches.
// ---------------------------------------------------------------------------

const DIR_UP: u8 = 1 << 0;
const DIR_DOWN: u8 = 1 << 1;
const DIR_LEFT: u8 = 1 << 2;
const DIR_RIGHT: u8 = 1 << 3;

fn dirs_of(c: char) -> u8 {
    match c {
        '│' => DIR_UP | DIR_DOWN,
        '─' => DIR_LEFT | DIR_RIGHT,
        '┌' => DIR_DOWN | DIR_RIGHT,
        '┐' => DIR_DOWN | DIR_LEFT,
        '└' => DIR_UP | DIR_RIGHT,
        '┘' => DIR_UP | DIR_LEFT,
        '├' => DIR_UP | DIR_DOWN | DIR_RIGHT,
        '┤' => DIR_UP | DIR_DOWN | DIR_LEFT,
        '┬' => DIR_DOWN | DIR_LEFT | DIR_RIGHT,
        '┴' => DIR_UP | DIR_LEFT | DIR_RIGHT,
        '┼' => DIR_UP | DIR_DOWN | DIR_LEFT | DIR_RIGHT,
        _ => 0,
    }
}

fn char_of(d: u8) -> char {
    match d {
        0 => ' ',
        d if d == DIR_UP | DIR_DOWN => '│',
        d if d == DIR_LEFT | DIR_RIGHT => '─',
        d if d == DIR_DOWN | DIR_RIGHT => '┌',
        d if d == DIR_DOWN | DIR_LEFT => '┐',
        d if d == DIR_UP | DIR_RIGHT => '└',
        d if d == DIR_UP | DIR_LEFT => '┘',
        d if d == DIR_UP | DIR_DOWN | DIR_RIGHT => '├',
        d if d == DIR_UP | DIR_DOWN | DIR_LEFT => '┤',
        d if d == DIR_DOWN | DIR_LEFT | DIR_RIGHT => '┬',
        d if d == DIR_UP | DIR_LEFT | DIR_RIGHT => '┴',
        d if d == DIR_UP | DIR_DOWN | DIR_LEFT | DIR_RIGHT => '┼',
        // Single-direction stubs collapse to space (unlikely in our drawing).
        _ => '│',
    }
}

/// OR the new directions into whatever's at the cell and produce the right
/// glyph.  Non-box characters (label letters, arrow heads, etc.) are NOT
/// overwritten by this — pass through unchanged.
fn merge_dirs(existing: char, new_dirs: u8) -> char {
    let existing_dirs = dirs_of(existing);
    if existing_dirs == 0 && existing != ' ' {
        // Don't trample non-box content (labels, arrow heads, etc.).
        return existing;
    }
    char_of(existing_dirs | new_dirs)
}

fn draw_horizontal(grid: &mut [Vec<char>], x0: usize, y: usize, x1: usize, label: &str) {
    if x0 > x1 {
        return;
    }
    for cell in grid[y].iter_mut().take(x1 + 1).skip(x0) {
        if *cell == ' ' {
            *cell = '─';
        }
    }
    let span = x1 - x0 + 1;
    let label_chars: Vec<char> = label.chars().collect();
    if label_chars.is_empty() || label_chars.len() + 2 > span {
        return;
    }
    let start = x0 + (span - label_chars.len()) / 2;
    for (i, c) in label_chars.iter().enumerate() {
        grid[y][start + i] = *c;
    }
}

fn draw_export(grid: &mut [Vec<char>], layout: &Layout, g: &Geom, exp: &Export) {
    let r = rank_of(layout, exp.provider);
    if r != 0 {
        return;
    }
    let target_left = g.col_x[r];
    let mid = g.node_mid[&exp.provider];
    let text = format!("ext:{} ──▶", exp.rendered_label);
    let text_chars: Vec<char> = text.chars().collect();
    let end = target_left.saturating_sub(1);
    if text_chars.len() > end + 1 {
        return;
    }
    let start = end + 1 - text_chars.len();
    for (i, c) in text_chars.iter().enumerate() {
        if start + i < g.width && grid[mid][start + i] == ' ' {
            grid[mid][start + i] = *c;
        }
    }
}

fn grid_to_string(grid: &[Vec<char>]) -> String {
    let mut out = String::new();
    for (i, row) in grid.iter().enumerate() {
        let mut end = row.len();
        while end > 0 && row[end - 1] == ' ' {
            end -= 1;
        }
        for c in &row[..end] {
            out.push(*c);
        }
        if i + 1 < grid.len() {
            out.push('\n');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn simple_chain_renders_a_box_per_node() {
        let g = simple_chain_graph();
        let out = generate_graph_ascii(&g, false, None).ascii;
        assert!(out.contains("middleware"), "middleware not in:\n{out}");
        assert!(out.contains("srv"), "srv not in:\n{out}");
        assert!(
            out.contains('┌') && out.contains('└'),
            "no box chars in:\n{out}"
        );
    }

    #[test]
    fn simple_chain_has_handler_edge_label() {
        let g = simple_chain_graph();
        let out = generate_graph_ascii(&g, false, None).ascii;
        assert!(out.contains("handler"), "no handler label in:\n{out}");
        assert!(out.contains('▶'), "no arrow head in:\n{out}");
    }

    #[test]
    fn long_chain_has_three_boxes() {
        let g = long_chain_graph();
        let out = generate_graph_ascii(&g, false, None).ascii;
        for name in ["gateway", "service", "backend"] {
            assert!(out.contains(name), "{name} missing from:\n{out}");
        }
        assert!(
            out.matches('▶').count() >= 2,
            "expected at least 2 arrow heads, got:\n{out}"
        );
    }

    #[test]
    fn empty_graph_message() {
        let g = crate::model::CompositionGraph::new();
        let out = generate_graph_ascii(&g, false, None).ascii;
        assert!(out.contains("No component instances"), "got:\n{out}");
    }

    #[test]
    fn two_chains_render_as_separate_sections() {
        let g = two_chain_graph();
        let out = generate_graph_ascii(&g, false, None).ascii;
        for name in ["srv-http", "mw-http", "db", "cache"] {
            assert!(out.contains(name), "{name} missing from:\n{out}");
        }
        // Each export has its own section header.
        assert!(out.contains("handler "), "no handler header in:\n{out}");
        assert!(out.contains("store "), "no store header in:\n{out}");
    }

    #[test]
    fn types_on_emits_symbol_and_signatures_section() {
        let g = typed_chain_graph();
        let out = generate_graph_ascii(&g, true, None).ascii;
        assert!(out.contains('✦'), "expected symbol in:\n{out}");
        assert!(
            out.contains("Signatures:"),
            "expected signatures section in:\n{out}"
        );
        assert!(
            out.contains("`handle`: (u32) -> bool"),
            "expected function sig in signatures, got:\n{out}"
        );
    }

    #[test]
    fn types_off_no_symbol_no_signatures_section() {
        let g = typed_chain_graph();
        let out = generate_graph_ascii(&g, false, None).ascii;
        assert!(
            !out.contains('✦'),
            "should not emit symbol when types off:\n{out}"
        );
        assert!(
            !out.contains("Signatures:"),
            "should not emit signatures section when types off:\n{out}"
        );
    }

    #[test]
    fn export_marker_inline_with_arrow() {
        let g = simple_chain_graph();
        let out = generate_graph_ascii(&g, false, None).ascii;
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines.iter().any(|l| l.contains("ext:handler")
                && l.contains("──▶")
                && l.contains("middleware")),
            "expected `ext:handler ──▶ │ middleware │` on a single line, got:\n{out}"
        );
    }

    #[test]
    fn fallback_no_exports_still_renders_nodes() {
        use crate::model::{ComponentNode, CompositionGraph, InterfaceConnection};
        let mut g = CompositionGraph::new();
        g.add_node(1, ComponentNode::new("$adapter".into(), 0, 0));
        g.add_node(2, ComponentNode::new("$mdl-a".into(), 1, 1));
        if let Some(n) = g.nodes.get_mut(&1) {
            n.add_import(InterfaceConnection {
                interface_name: "wasi:http/handler@0.3.0".into(),
                source_instance: Some(2),
                is_host_import: false,
                interface_type: None,
                fingerprint: None,
            });
        }
        let out = generate_graph_ascii(&g, false, None).ascii;
        assert!(out.contains("adapter"));
        assert!(out.contains("mdl-a"));
    }

    #[test]
    fn shared_instance_uses_double_line_border() {
        // Two exports both reach the same logger instance.  In the second
        // section, logger should render with ╔═╗ borders.
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
        let out = generate_graph_ascii(&g, false, None).ascii;
        assert!(
            out.contains('╔') && out.contains('╗'),
            "expected double-line box for shared logger in:\n{out}"
        );
    }
}
