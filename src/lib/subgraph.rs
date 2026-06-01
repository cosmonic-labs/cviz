//! Per-export reachability subgraphs.
//!
//! A composition typically exposes one or more interfaces to the outside
//! world via top-level exports.  For each export, the *reachable* set of
//! instances — everything the exporter transitively calls in request-flow
//! direction — forms a self-contained slice of the larger composition.
//!
//! This module computes those slices.  Each [`ExportSubgraph`] names one
//! export, identifies the instance providing it, and enumerates all
//! reachable instances and the inter-component edges among them.
//!
//! Identity is by **instance index** (the `u32` key in
//! [`CompositionGraph::nodes`]), not by display name.  Two component
//! instances can share a display label without being the same instance;
//! callers that need to detect "shared across subgraphs" should compare
//! `u32`s, not strings.

use crate::model::{CompositionGraph, SYNTHETIC_COMPONENT};
use std::collections::{BTreeSet, VecDeque};

/// One export's reachable closure within a [`CompositionGraph`].
#[derive(Debug, Clone)]
pub struct ExportSubgraph {
    /// Fully-qualified exported interface name (e.g. `wasi:http/handler@0.3.0`).
    pub interface_name: String,
    /// Instance index of the node providing the export (the subgraph root).
    pub source_instance: u32,
    /// Every reachable instance, **including** the source.  Synthetic
    /// component instances are excluded.
    pub nodes: BTreeSet<u32>,
    /// Inter-component edges among the reachable nodes, in
    /// `(caller, provider, interface)` order — i.e. request-flow direction.
    /// Host imports are excluded.
    pub edges: Vec<SubgraphEdge>,
}

/// A single (caller → provider) edge inside an [`ExportSubgraph`].
#[derive(Debug, Clone)]
pub struct SubgraphEdge {
    pub caller: u32,
    pub provider: u32,
    pub interface: String,
}

/// Compute one [`ExportSubgraph`] per top-level export.
///
/// Subgraphs are returned in the iteration order of
/// [`CompositionGraph::component_exports`] (sorted by interface name, since
/// the map is a `BTreeMap`).  An instance may appear in more than one
/// subgraph; consumers that care about sharing should diff the `nodes`
/// sets against one another.
pub fn compute_export_subgraphs(graph: &CompositionGraph) -> Vec<ExportSubgraph> {
    graph
        .component_exports
        .iter()
        .filter_map(|(name, info)| {
            // Skip exports whose source isn't a real node (defensive — the
            // parser should not produce these, but better safe).
            let src_node = graph.nodes.get(&info.source_instance)?;
            if src_node.component_index == SYNTHETIC_COMPONENT {
                return None;
            }
            let nodes = reachable_from(graph, info.source_instance);
            let edges = collect_edges(graph, &nodes);
            Some(ExportSubgraph {
                interface_name: name.clone(),
                source_instance: info.source_instance,
                nodes,
                edges,
            })
        })
        .collect()
}

/// Instance indices that appear in two or more of the given subgraphs.
///
/// Used by the renderer to decide which boxes deserve the "shared instance"
/// visual treatment (double-line border in the second-and-subsequent
/// occurrences).
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
            // Recorded for completeness in `visited`; we'll filter at the end.
            continue;
        }
        for import in &node.imports {
            if import.is_host_import {
                continue;
            }
            let Some(src) = import.source_instance else {
                continue;
            };
            if graph.nodes.contains_key(&src) {
                queue.push_back(src);
            }
        }
    }
    // Strip any synthetic nodes that snuck in (the start might be synthetic).
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

fn collect_edges(graph: &CompositionGraph, nodes: &BTreeSet<u32>) -> Vec<SubgraphEdge> {
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
}
