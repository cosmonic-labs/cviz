use crate::model::{CompositionGraph, ExportInfo, InterfaceConnection};
use std::collections::HashSet;

pub mod model;
pub mod output;
pub mod parse;

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
    let inter_component: HashSet<&str> = graph.nodes.values()
        .flat_map(|n| n.imports.iter())
        .filter(|c| !c.is_host_import)
        .map(|c| c.interface_name.as_str())
        .collect();

    graph.component_exports.keys()
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
