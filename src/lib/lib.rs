use crate::model::{CompositionGraph, ExportInfo, InterfaceConnection};
use std::collections::HashSet;

pub mod model;
pub mod output;
pub mod parse;
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
pub fn get_chain_for(graph: &CompositionGraph, interface_name: &str) -> Vec<u32> {
    let mut chain = Vec::new();

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

    if let Some(start) = export_instance {
        // Walk from export through the chain following handler imports
        let mut current = Some((start, false));

        let mut visited = std::collections::HashSet::new();

        while let Some((idx, is_host)) = current {
            if is_host || visited.contains(&idx) {
                break; // Avoid infinite loops
            }
            visited.insert(idx);
            chain.push(idx);

            // Find what this node imports for handler
            current = graph.nodes.get(&idx).and_then(|node| {
                node.imports
                    .iter()
                    .find(|conn| is_connection_for(conn, interface_name) && !conn.is_host_import)
                    .map(|conn| (conn.source_instance, conn.is_host_import))
            });
        }
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
            source_instance: 0,
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
            source_instance: 2,
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, a);

        let mut b = ComponentNode::new("$b".to_string(), 1, 1);
        b.add_import(InterfaceConnection {
            interface_name: "test:iface/foo@0.1.0".to_string(),
            source_instance: 1,
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
}
