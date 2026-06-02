//! Per-export reachability subgraphs.
//!
//! For each top-level export, the *reachable* set of instances —
//! everything the exporter transitively calls in request-flow direction —
//! forms a self-contained slice of the larger composition.
//!
//! Identity is by **instance index** (the `u32` key in
//! [`CompositionGraph::nodes`]), not by display name: two component
//! instances can share a display label without being the same instance.

use crate::highlights::Highlights;
use crate::model::{CompositionGraph, SYNTHETIC_COMPONENT};
use std::collections::{BTreeSet, VecDeque};

/// One export's reachable closure within a [`CompositionGraph`].
#[derive(Debug, Clone)]
pub struct ExportSubgraph {
    /// Fully-qualified exported interface name (e.g. `wasi:http/handler@0.3.0`).
    pub interface_name: String,
    /// The instance providing the export (subgraph root).
    pub source_instance: u32,
    /// Every reachable instance, including the source. Synthetics excluded.
    pub nodes: BTreeSet<u32>,
    /// Inter-component edges among the reachable nodes in request-flow
    /// direction. Host imports excluded.
    pub edges: Vec<SubgraphEdge>,
}

#[derive(Debug, Clone)]
pub struct SubgraphEdge {
    pub caller: u32,
    pub provider: u32,
    pub interface: String,
}

/// Compute one [`ExportSubgraph`] per top-level export, in interface-name
/// order (the `BTreeMap` iteration order of `component_exports`).
///
/// When the recorded export source carries no inter-component import for its
/// own exported interface (a WAC-compiled passthrough shim wires the interface
/// via individual function arguments instead), the subgraph is reconstructed
/// from the inter-component import graph via [`crate::build_provider_chain`]
/// and rooted at the chain's outermost provider.
pub fn compute_export_subgraphs(graph: &CompositionGraph) -> Vec<ExportSubgraph> {
    graph
        .component_exports
        .iter()
        .filter_map(|(name, info)| {
            let src_node = graph.nodes.get(&info.source_instance)?;
            if src_node.component_index == SYNTHETIC_COMPONENT {
                return None;
            }

            let source_has_chain_import = src_node
                .imports
                .iter()
                .any(|c| c.interface_name == *name && !c.is_host_import);

            let (source_instance, nodes) = if source_has_chain_import {
                (
                    info.source_instance,
                    reachable_from(graph, info.source_instance),
                )
            } else {
                let chain = crate::build_provider_chain(graph, name);
                match chain.first() {
                    Some(&chain_start) => (chain_start, reachable_from(graph, chain_start)),
                    None => (
                        info.source_instance,
                        reachable_from(graph, info.source_instance),
                    ),
                }
            };

            let edges = collect_edges(graph, &nodes);
            Some(ExportSubgraph {
                interface_name: name.clone(),
                source_instance,
                nodes,
                edges,
            })
        })
        .collect()
}

/// Instance indices that appear in two or more of the given subgraphs —
/// what the renderer uses to flag a shared instance with the double-line
/// border treatment.
pub fn shared_instances(subgraphs: &[ExportSubgraph]) -> BTreeSet<u32> {
    let mut seen = BTreeSet::new();
    let mut shared = BTreeSet::new();
    for sg in subgraphs {
        for n in &sg.nodes {
            if !seen.insert(*n) {
                shared.insert(*n);
            }
        }
    }
    shared
}

/// BFS through inter-component imports starting at `start`. Synthetic
/// nodes are not traversed through and are stripped from the result; if
/// `start` itself is synthetic the result is empty.
fn reachable_from(graph: &CompositionGraph, start: u32) -> BTreeSet<u32> {
    let mut visited: BTreeSet<u32> = BTreeSet::new();
    let mut queue: VecDeque<u32> = VecDeque::from([start]);
    while let Some(idx) = queue.pop_front() {
        if !visited.insert(idx) {
            continue;
        }
        let Some(node) = graph.nodes.get(&idx) else {
            continue;
        };
        if node.component_index == SYNTHETIC_COMPONENT {
            continue;
        }
        for import in &node.imports {
            if import.is_host_import {
                continue;
            }
            if let Some(src) = import.source_instance {
                if graph.nodes.contains_key(&src) {
                    queue.push_back(src);
                }
            }
        }
    }
    visited
        .into_iter()
        .filter(|idx| {
            graph
                .nodes
                .get(idx)
                .is_some_and(|n| n.component_index != SYNTHETIC_COMPONENT)
        })
        .collect()
}

/// Collect every canonical node ID and edge ID present in `graph` /
/// `subgraphs`.
pub(crate) fn canonical_ids(
    graph: &CompositionGraph,
    subgraphs: &[ExportSubgraph],
) -> (BTreeSet<String>, BTreeSet<String>) {
    use crate::canonical_id::canonical_edge_id;
    let nodes: BTreeSet<String> = graph
        .nodes
        .values()
        .map(|n| n.canonical_id().to_string())
        .collect();
    let mut edges: BTreeSet<String> = BTreeSet::new();
    for sg in subgraphs {
        for e in &sg.edges {
            let caller = graph.nodes.get(&e.caller).map(|n| n.canonical_id());
            let provider = graph.nodes.get(&e.provider).map(|n| n.canonical_id());
            if let (Some(c), Some(p)) = (caller, provider) {
                edges.insert(canonical_edge_id(&e.interface, Some(c), p));
            }
        }
        // Skip the anonymous fallback subgraph's empty interface name —
        // there's no boundary edge to register for it.
        if !sg.interface_name.is_empty() {
            if let Some(src) = graph.nodes.get(&sg.source_instance) {
                edges.insert(canonical_edge_id(
                    &sg.interface_name,
                    None,
                    src.canonical_id(),
                ));
            }
        }
    }
    (nodes, edges)
}

/// `Highlights::tag_lines_referenced_by` lifted to take the present-id
/// sets [`canonical_ids`] returns and an `Option<&Highlights>`. Empty
/// when there are no highlights.
pub(crate) fn filtered_tag_lines(
    highlights: Option<&Highlights>,
    present_nodes: &BTreeSet<String>,
    present_edges: &BTreeSet<String>,
) -> Vec<String> {
    highlights
        .map(|h| {
            h.tag_lines_referenced_by(
                present_nodes.iter().map(String::as_str),
                present_edges.iter().map(String::as_str),
            )
        })
        .unwrap_or_default()
}

/// Collect every inter-component edge whose caller AND provider are both
/// in `nodes`. Host imports and dangling source-instance refs are skipped.
pub(crate) fn collect_edges(graph: &CompositionGraph, nodes: &BTreeSet<u32>) -> Vec<SubgraphEdge> {
    let mut edges = Vec::new();
    for &caller_idx in nodes {
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
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn simple_chain_single_subgraph() {
        let g = simple_chain_graph();
        let sgs = compute_export_subgraphs(&g);
        assert_eq!(sgs.len(), 1, "simple chain has one export");
        let sg = &sgs[0];
        assert!(sg.interface_name.contains("wasi:http/handler"));
        // Reachable from middleware (export source): middleware (2) and srv (1).
        assert_eq!(sg.nodes, BTreeSet::from([1, 2]));
        // One inter-component edge: middleware → srv.
        assert_eq!(sg.edges.len(), 1);
        let e = &sg.edges[0];
        assert_eq!(e.caller, 2);
        assert_eq!(e.provider, 1);
        assert!(e.interface.contains("wasi:http/handler"));
    }

    #[test]
    fn two_chain_graph_yields_two_subgraphs() {
        let g = two_chain_graph();
        let sgs = compute_export_subgraphs(&g);
        assert_eq!(sgs.len(), 2);
        // Each subgraph contains exactly its chain's two nodes.
        let by_iface: std::collections::HashMap<&str, &ExportSubgraph> = sgs
            .iter()
            .map(|sg| (sg.interface_name.as_str(), sg))
            .collect();
        let http = by_iface
            .values()
            .find(|sg| sg.interface_name.contains("http"))
            .unwrap();
        let kv = by_iface
            .values()
            .find(|sg| sg.interface_name.contains("keyvalue"))
            .unwrap();
        assert_eq!(http.nodes, BTreeSet::from([1, 2]));
        assert_eq!(kv.nodes, BTreeSet::from([3, 4]));
    }

    #[test]
    fn shared_instances_detects_overlap() {
        // Hand-construct a graph where two exports both reach the same node.
        use crate::model::{ComponentNode, CompositionGraph, InterfaceConnection};
        let mut g = CompositionGraph::new();
        // 1: shared utility (logger)
        g.add_node(1, ComponentNode::new("$logger".into(), 0, 0));
        // 2: srv-http — exports http handler, imports log from logger
        let mut srv_http = ComponentNode::new("$srv-http".into(), 1, 1);
        srv_http.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".into(),
            source_instance: Some(1),
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        g.add_node(2, srv_http);
        // 3: cache — exports keyvalue store, also imports log from logger
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

        let sgs = compute_export_subgraphs(&g);
        let shared = shared_instances(&sgs);
        assert_eq!(shared, BTreeSet::from([1]), "logger should be shared");
    }

    #[test]
    fn synthetic_export_source_skipped() {
        use crate::model::{ComponentNode, CompositionGraph, SYNTHETIC_COMPONENT};
        let mut g = CompositionGraph::new();
        g.add_node(
            1,
            ComponentNode::new("$synth".into(), SYNTHETIC_COMPONENT, SYNTHETIC_COMPONENT),
        );
        g.add_export("test:x/y@1.0.0".into(), 1, None);
        let sgs = compute_export_subgraphs(&g);
        assert!(sgs.is_empty(), "synthetic export source should be filtered");
    }

    #[test]
    fn reachability_excludes_host_imports() {
        // Node imports from host AND from another node; only the latter is followed.
        let g = simple_chain_graph(); // srv imports handler from host; middleware imports from srv
        let sgs = compute_export_subgraphs(&g);
        assert_eq!(sgs[0].nodes.len(), 2);
        // No edge has a host-style caller; all internal.
        for e in &sgs[0].edges {
            assert!(g.nodes.contains_key(&e.caller));
            assert!(g.nodes.contains_key(&e.provider));
        }
    }

    #[test]
    fn shim_export_direct_uses_provider_chain() {
        let g = shim_export_direct_graph();
        let sgs = compute_export_subgraphs(&g);
        assert_eq!(sgs.len(), 1);
        let sg = &sgs[0];
        // Shim source (3) substituted with the chain's outermost provider (1 = $base).
        assert_eq!(sg.source_instance, 1);
        assert_eq!(sg.nodes, BTreeSet::from([1]));
        assert!(sg.edges.is_empty());
    }

    #[test]
    fn shim_export_one_middleware_uses_provider_chain() {
        let g = shim_export_one_middleware_graph();
        let sgs = compute_export_subgraphs(&g);
        assert_eq!(sgs.len(), 1);
        let sg = &sgs[0];
        // Shim source (4) substituted with $middleware (2); reachable chain is [2, 1].
        assert_eq!(sg.source_instance, 2);
        assert_eq!(sg.nodes, BTreeSet::from([1, 2]));
        assert_eq!(sg.edges.len(), 1);
        let e = &sg.edges[0];
        assert_eq!(e.caller, 2);
        assert_eq!(e.provider, 1);
    }

    #[test]
    fn shim_export_three_middleware_uses_provider_chain() {
        let g = shim_export_three_middleware_graph();
        let sgs = compute_export_subgraphs(&g);
        assert_eq!(sgs.len(), 1);
        let sg = &sgs[0];
        // Shim source (6) substituted with $mdl-a (4); reachable chain is [4, 3, 2, 1].
        assert_eq!(sg.source_instance, 4);
        assert_eq!(sg.nodes, BTreeSet::from([1, 2, 3, 4]));
        assert_eq!(sg.edges.len(), 3);
    }
}
