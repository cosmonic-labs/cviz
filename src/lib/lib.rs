use crate::model::{CompositionGraph, ExportInfo, InterfaceConnection};
use std::collections::HashSet;

pub mod model;
pub mod output;
pub mod parse;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
pub(crate) mod test_utils;

/// Check if this is the connection for a specific interface
pub fn is_connection_for(conn: &InterfaceConnection, interface_name: &str) -> bool {
    conn.interface_name.contains(interface_name)
}

/// Find all interfaces that form a middleware chain in the composition.
///
/// An interface forms a chain when it is both:
/// - exported by the final composed component, and
/// - imported by at least one real component instance from another real
///   component instance (i.e. not from the host).
///
/// This captures the middleware pattern generically, without assuming any
/// specific interface name.
pub fn find_chain_interfaces(graph: &CompositionGraph) -> Vec<String> {
    let inter_component: HashSet<&str> = graph
        .nodes
        .values()
        .flat_map(|n| n.imports.iter())
        .filter(|c| !c.is_host_import)
        .map(|c| c.interface_name.as_str())
        .collect();

    graph
        .component_exports
        .keys()
        .filter(|name| inter_component.contains(name.as_str()))
        .cloned()
        .collect()
}

/// Get the chain in request-flow order (outermost → innermost).
/// The first element is the exported interface (entry point for requests),
/// and the last element is the innermost interface (imports from host).
///
/// When the export source is a synthetic pass-through shim (no recorded imports
/// for the interface), the chain is built from the inter-component import graph
/// instead: starting from whichever node provides the interface to the terminal
/// consumer, walking down through the provider chain.
pub fn get_chain_for(graph: &CompositionGraph, interface_name: &str) -> Vec<u32> {
    // Find the export point for the interface
    let export_instance = graph
        .component_exports
        .iter()
        .find(|(name, _)| name.contains(interface_name))
        .map(
            |(
                _,
                ExportInfo {
                    source_instance: idx,
                    ..
                },
            )| *idx,
        );

    let Some(start) = export_instance else {
        return vec![];
    };

    // If the export source has a non-host import for this interface, walk from
    // it directly through the import chain (the normal middleware model).
    let start_has_relevant_import = graph.get_node(start).is_some_and(|n| {
        n.imports
            .iter()
            .any(|c| is_connection_for(c, interface_name) && !c.is_host_import)
    });

    if start_has_relevant_import {
        let mut chain = Vec::new();
        let mut current: Option<(u32, bool)> = Some((start, false));
        let mut visited = std::collections::HashSet::new();

        while let Some((idx, is_host)) = current {
            if is_host || visited.contains(&idx) {
                break;
            }
            visited.insert(idx);
            chain.push(idx);

            current = graph.nodes.get(&idx).and_then(|node| {
                node.imports
                    .iter()
                    .find(|conn| is_connection_for(conn, interface_name) && !conn.is_host_import)
                    .and_then(|conn| conn.source_instance.map(|src| (src, conn.is_host_import)))
            });
        }
        return chain;
    }

    // Fallback: the export source is a synthetic shim whose constructor args are
    // not tracked as instance imports (e.g. WAC-compiled compositions that wire
    // interfaces via individual function arguments).  Build the provider chain
    // from the inter-component import graph.
    build_provider_chain(graph, interface_name)
}

/// Build the provider chain for `interface_name` from the inter-component import
/// graph when the export source carries no usable import edges.
///
/// Returns the chain in request-flow order: the node that serves the terminal
/// consumer comes first; the base provider (no further upstream) comes last.
fn build_provider_chain(graph: &CompositionGraph, interface_name: &str) -> Vec<u32> {
    // Collect all inter-component (non-host) connections for this interface.
    let connections: Vec<(u32, u32)> = graph
        .nodes
        .iter()
        .flat_map(|(&consumer_id, node)| {
            node.imports
                .iter()
                .filter(|c| is_connection_for(c, interface_name) && !c.is_host_import)
                .filter_map(move |c| c.source_instance.map(|src| (consumer_id, src)))
        })
        .collect();

    if connections.is_empty() {
        return vec![];
    }

    // provider_of[consumer] = source
    let provider_of: std::collections::HashMap<u32, u32> = connections.iter().copied().collect();

    // Nodes that are themselves providers for this interface
    let providers: HashSet<u32> = connections.iter().map(|(_, src)| *src).collect();

    // The terminal consumer imports the interface but is not a source for it,
    // i.e. it is a pure consumer (e.g. service-comp in a fanin composition).
    let terminal_consumer = connections
        .iter()
        .map(|(consumer, _)| *consumer)
        .find(|c| !providers.contains(c));

    let Some(terminal) = terminal_consumer else {
        return vec![];
    };

    // Walk from the terminal consumer's provider down through the provider chain.
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut current = provider_of.get(&terminal).copied();

    while let Some(node_id) = current {
        if visited.contains(&node_id) {
            break;
        }
        visited.insert(node_id);
        chain.push(node_id);
        current = provider_of.get(&node_id).copied();
    }

    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_find_chain_interfaces_two_chains() {
        let graph = two_chain_graph();
        let mut chains = find_chain_interfaces(&graph);
        chains.sort();
        assert_eq!(chains.len(), 2, "should find exactly two chain interfaces");
        assert!(
            chains.iter().any(|c| c.contains("handler")),
            "should find http handler chain"
        );
        assert!(
            chains.iter().any(|c| c.contains("store")),
            "should find keyvalue store chain"
        );
    }

    #[test]
    fn test_find_chain_interfaces_utility_node_excluded() {
        // $logger has only host imports — its interface is not exported, so no chain
        let graph = chain_plus_utility_graph();
        let chains = find_chain_interfaces(&graph);
        assert_eq!(chains.len(), 1, "utility-only node should not form a chain");
        assert!(chains[0].contains("handler"));
    }

    #[test]
    fn test_get_chain_for_http_handler() {
        let graph = simple_chain_graph();
        let chain = get_chain_for(&graph, "wasi:http/handler@0.3.0");
        // Request-flow order: middleware(2) → srv(1)
        assert_eq!(
            chain,
            vec![2, 1],
            "http handler chain should walk middleware → srv"
        );
    }

    #[test]
    fn test_get_chain_for_long_chain() {
        // long_chain_graph uses wasi:messaging/consumer to verify generality beyond http
        let graph = long_chain_graph();
        let chain = get_chain_for(&graph, "wasi:messaging/consumer@0.2.0");
        // Request-flow order: gateway(3) → service(2) → backend(1)
        assert_eq!(
            chain,
            vec![3, 2, 1],
            "messaging chain should be in request-flow order (outermost first)"
        );
    }

    // Verify that chain walking works for a non-http interface (keyvalue/store).
    // This ensures get_chain_for is genuinely generic, not accidentally http-specific.
    #[test]
    fn test_get_chain_for_keyvalue_interface() {
        let graph = two_chain_graph();
        let chain = get_chain_for(&graph, "wasi:keyvalue/store@0.1.0");
        // cache(4) is the export source, db(3) is the inner node
        assert_eq!(chain, vec![4, 3], "keyvalue chain should walk cache → db");
    }

    #[test]
    fn test_find_chain_interfaces_exported_but_not_imported() {
        // An interface that is exported but never imported inter-component
        // (only imported from the host) should NOT be identified as a chain.
        let mut graph = CompositionGraph::new();
        use crate::model::{ComponentNode, InterfaceConnection};

        let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: None,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, srv);
        // Export it, but nobody imports it from another component
        graph.add_export("wasi:http/handler@0.3.0".to_string(), 1, None);

        let chains = find_chain_interfaces(&graph);
        assert!(
            chains.is_empty(),
            "export with no inter-component importers should not be a chain"
        );
    }

    #[test]
    fn test_get_chain_for_unknown_interface() {
        let graph = simple_chain_graph();
        let chain = get_chain_for(&graph, "does:not/exist@0.0.0");
        assert!(
            chain.is_empty(),
            "unknown interface should return empty chain"
        );
    }

    #[test]
    fn test_get_chain_for_no_cycle() {
        // Build a graph where two nodes import each other on the same interface.
        // get_chain_for must terminate without panicking.
        let mut graph = CompositionGraph::new();
        use crate::model::{ComponentNode, InterfaceConnection};

        let mut a = ComponentNode::new("$a".to_string(), 0, 0);
        a.add_import(InterfaceConnection {
            interface_name: "test:iface/foo@0.1.0".to_string(),
            source_instance: Some(2),
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, a);

        let mut b = ComponentNode::new("$b".to_string(), 1, 1);
        b.add_import(InterfaceConnection {
            interface_name: "test:iface/foo@0.1.0".to_string(),
            source_instance: Some(1),
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(2, b);

        graph.add_export("test:iface/foo@0.1.0".to_string(), 1, None);

        // Should not hang; chain length is bounded by node count
        let chain = get_chain_for(&graph, "test:iface/foo@0.1.0");
        assert!(chain.len() <= 2, "cycle detection should bound the chain");
    }

    // -----------------------------------------------------------------------
    // Shim-export / provider-chain fallback
    //
    // These tests cover the `build_provider_chain` fallback path triggered when
    // the export source has no recorded imports for the interface — the pattern
    // produced by WAC-compiled compositions that wire interfaces via individual
    // function arguments rather than instance arguments.
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_chain_for_shim_export_direct() {
        // Export source is a shim with no api imports; the only inter-component
        // connection is consumer → base.  Chain should be just [base].
        let graph = shim_export_direct_graph();
        let chain = get_chain_for(&graph, "test:svc/api@1.0.0");
        assert_eq!(chain, vec![1], "direct shim export: chain should be [base]");
    }

    #[test]
    fn test_get_chain_for_shim_export_one_middleware() {
        // consumer → middleware → base, exported via shim.
        // Chain should be [middleware, base] in request-flow order.
        let graph = shim_export_one_middleware_graph();
        let chain = get_chain_for(&graph, "test:svc/api@1.0.0");
        assert_eq!(
            chain,
            vec![2, 1],
            "one-middleware shim export: chain should be [middleware, base]"
        );
        // Sanity-check the node labels
        assert_eq!(
            graph.get_node(chain[0]).unwrap().display_label(),
            "middleware"
        );
        assert_eq!(graph.get_node(chain[1]).unwrap().display_label(), "base");
    }

    #[test]
    fn test_get_chain_for_shim_export_three_middlewares() {
        // consumer → mdl-a → mdl-b → mdl-c → base, exported via shim.
        // Chain should be [mdl-a, mdl-b, mdl-c, base].
        let graph = shim_export_three_middleware_graph();
        let chain = get_chain_for(&graph, "test:svc/api@1.0.0");
        assert_eq!(
            chain,
            vec![4, 3, 2, 1],
            "three-middleware shim export: chain should be [mdl-a, mdl-b, mdl-c, base]"
        );
        assert_eq!(graph.get_node(chain[0]).unwrap().display_label(), "mdl-a");
        assert_eq!(graph.get_node(chain[3]).unwrap().display_label(), "base");
    }

    #[test]
    fn test_find_chain_interfaces_shim_export() {
        // Even with a shim export source, the interface qualifies as a chain
        // because it is exported AND imported inter-component.
        let graph = shim_export_one_middleware_graph();
        let chains = find_chain_interfaces(&graph);
        assert!(
            chains.iter().any(|c| c.contains("test:svc/api")),
            "shim-exported interface should be identified as a chain interface"
        );
    }
}
