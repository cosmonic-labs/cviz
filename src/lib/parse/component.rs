use anyhow::{anyhow, Result};
use std::collections::HashMap;
use wasmparser::{
    ComponentExport, ComponentExternalKind, ComponentInstance, KnownCustom, Parser, Payload,
};

use crate::model::{ComponentNode, CompositionGraph, InterfaceConnection};

/// Parse a WebAssembly component file and extract its composition graph
pub fn parse_component(bytes: &[u8]) -> Result<CompositionGraph> {
    let parser = Parser::new(0);
    let mut graph = CompositionGraph::new();

    // Collect component instances and exports from the TOP-LEVEL component only
    let mut component_instances: Vec<ComponentInstance> = Vec::new();
    let mut component_exports: Vec<ComponentExport> = Vec::new();

    // Track the next instance index (imports, instantiations, and aliases all create instances)
    let mut next_instance_index: u32 = 0;

    // Map from vector position to actual instance index
    let mut instance_index_map: Vec<u32> = Vec::new();

    // Map from alias instance index to the source instance it aliases from
    let mut alias_to_source: HashMap<u32, u32> = HashMap::new();

    // Instance names from the component-name custom section (parsed before instances)
    let mut instance_names: HashMap<u32, String> = HashMap::new();

    // Track nesting depth to only process top-level sections
    let mut depth = 0;

    for payload in parser.parse_all(bytes) {
        let payload = payload.map_err(|e| anyhow!("Failed to parse wasm: {}", e))?;

        match &payload {
            Payload::ComponentSection { .. } | Payload::ModuleSection { .. } => {
                depth += 1;
            }
            Payload::End(_) => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {}
        }

        // Only process top-level payloads
        if depth > 0 {
            continue;
        }

        match payload {
            Payload::CustomSection(reader) => {
                if let KnownCustom::ComponentName(name_reader) = reader.as_known() {
                    for subsection in name_reader {
                        if let Ok(wasmparser::ComponentName::Instances(names)) = subsection {
                            for naming in names.into_iter().flatten() {
                                instance_names.insert(naming.index, naming.name.to_string());
                            }
                        }
                    }
                }
            }
            Payload::ComponentImportSection(reader) => {
                // Each import of an instance type creates a new instance index
                for import in reader {
                    let import = import.map_err(|e| anyhow!("Failed to parse import: {}", e))?;
                    if matches!(import.ty, wasmparser::ComponentTypeRef::Instance(_)) {
                        next_instance_index += 1;
                    }
                }
            }
            Payload::ComponentInstanceSection(reader) => {
                for instance in reader {
                    let instance =
                        instance.map_err(|e| anyhow!("Failed to parse instance: {}", e))?;
                    instance_index_map.push(next_instance_index);
                    next_instance_index += 1;
                    component_instances.push(instance);
                }
            }
            Payload::ComponentAliasSection(reader) => {
                // Aliases can also create new instances
                for alias in reader {
                    let alias = alias.map_err(|e| anyhow!("Failed to parse alias: {}", e))?;
                    // Only InstanceExport aliases create new instances
                    if let wasmparser::ComponentAlias::InstanceExport {
                        kind,
                        instance_index,
                        ..
                    } = alias
                    {
                        if kind == ComponentExternalKind::Instance {
                            // This creates a new instance that aliases from instance_index
                            alias_to_source.insert(next_instance_index, instance_index);
                            next_instance_index += 1;
                        }
                    }
                }
            }
            Payload::ComponentExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(|e| anyhow!("Failed to parse export: {}", e))?;
                    component_exports.push(export);
                }
            }
            _ => {}
        }
    }

    // Process component instances - this is where the composition wiring lives
    for (vec_idx, instance) in component_instances.iter().enumerate() {
        let instance_idx = instance_index_map[vec_idx];

        match instance {
            ComponentInstance::Instantiate {
                component_index,
                args,
            } => {
                let name = instance_names
                    .get(&instance_idx)
                    .cloned()
                    .unwrap_or_else(|| format!("instance_{}", instance_idx));

                let mut node = ComponentNode::new(name, *component_index);

                // Process the "with" arguments - these are the interface connections
                for arg in args.iter() {
                    let interface_name = arg.name.to_string();

                    // The arg.index is the instance providing this interface
                    // It might be an alias, so resolve it to the actual source instance
                    let source_idx = resolve_alias(arg.index, &alias_to_source);

                    let connection = InterfaceConnection::from_instance(interface_name, source_idx);
                    node.add_import(connection);
                }

                graph.add_node(instance_idx, node);
            }
            ComponentInstance::FromExports(_exports) => {
                // This is a synthetic instance created from exports
                // These often wrap host imports - we don't track them as nodes
                // since they're just interface bundles, not actual components
            }
        }
    }

    // Process component exports to find what the composition exposes
    for export in component_exports.iter() {
        let export_name = export.name.0.to_string();
        // Only track instance exports
        if export.kind == ComponentExternalKind::Instance {
            // Resolve alias to find the actual component instance
            let source_idx = resolve_alias(export.index, &alias_to_source);
            graph.add_export(export_name, source_idx);
        }
    }

    // Mark host imports on the connections
    // Instances 0 to (first component instance - 1) are imports from the host
    let first_component_instance = instance_index_map.first().copied().unwrap_or(0);

    for node in graph.nodes.values_mut() {
        for import in &mut node.imports {
            if import.source_instance < first_component_instance {
                import.is_host_import = true;
            }
        }
    }

    Ok(graph)
}

/// Resolve an alias chain to find the ultimate source instance
fn resolve_alias(idx: u32, alias_to_source: &HashMap<u32, u32>) -> u32 {
    let mut current = idx;
    let limit = 100;
    // Follow the alias chain (with loop protection)
    for i in 0..limit {
        if let Some(&source) = alias_to_source.get(&current) {
            current = source;
            if i == limit - 1 {
                eprintln!(
                    "Warning: alias chain for instance {} exceeded {} steps, resolution may be incomplete",
                    idx, limit
                );
            }
        } else {
            break;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{get_chain_for, is_connection_for};

    /// WAT for a composed component with two middleware instances chained via wasi:http/handler.
    ///
    /// Structure:
    ///   host(handler) → middleware-a → middleware-b → export(handler)
    fn two_middleware_chain_wat() -> &'static str {
        r#"(component
            (import "wasi:http/handler@0.3.0" (instance $host
                (export "handle" (func))
            ))

            (component $middleware-a
                (import "wasi:http/handler@0.3.0" (instance $imp
                    (export "handle" (func))
                ))
                (alias export $imp "handle" (func $f))
                (instance $out (export "handle" (func $f)))
                (export "wasi:http/handler@0.3.0" (instance $out))
            )

            (instance $a (instantiate $middleware-a
                (with "wasi:http/handler@0.3.0" (instance $host))
            ))
            (alias export $a "wasi:http/handler@0.3.0" (instance $a-out))

            (component $middleware-b
                (import "wasi:http/handler@0.3.0" (instance $imp
                    (export "handle" (func))
                ))
                (alias export $imp "handle" (func $f))
                (instance $out (export "handle" (func $f)))
                (export "wasi:http/handler@0.3.0" (instance $out))
            )

            (instance $b (instantiate $middleware-b
                (with "wasi:http/handler@0.3.0" (instance $a-out))
            ))
            (alias export $b "wasi:http/handler@0.3.0" (instance $b-out))

            (export "wasi:http/handler@0.3.0" (instance $b-out))
        )"#
    }

    #[test]
    fn test_parse_composed_component() {
        let bytes = wat::parse_str(two_middleware_chain_wat()).expect("failed to parse WAT");
        let graph = parse_component(&bytes).expect("failed to parse component");

        // Should have exactly 2 real component nodes (the two middleware instances)
        let real_nodes = graph.real_nodes();
        assert_eq!(real_nodes.len(), 2, "expected 2 real component nodes");

        // Each node should have a handler import
        let http_interface = "wasi:http/handler";
        for node in &real_nodes {
            assert!(
                node.imports
                    .iter()
                    .any(|i| is_connection_for(i, http_interface)),
                "node '{}' should have a handler import",
                node.name
            );
        }

        // Should have an export for the handler
        assert!(
            graph
                .component_exports
                .keys()
                .any(|k| k.contains(http_interface)),
            "expected handler export"
        );
    }

    #[test]
    fn test_handler_chain_detection() {
        let bytes = wat::parse_str(two_middleware_chain_wat()).expect("failed to parse WAT");
        let graph = parse_component(&bytes).expect("failed to parse component");

        let http_interface = "wasi:http/handler";
        let chain = get_chain_for(&graph, http_interface);
        assert_eq!(chain.len(), 2, "expected 2 nodes in handler chain");

        // Chain is in request-flow order: outermost (export) first, innermost last
        // First node is the export point (outermost handler)
        let first = graph.get_node(chain[0]).expect("first chain node");
        assert!(
            first
                .imports
                .iter()
                .any(|i| !i.is_host_import && is_connection_for(i, http_interface)),
            "first chain node (outermost) should import handler from another component"
        );

        // Last node imports from host (innermost handler)
        let last = graph.get_node(chain[1]).expect("last chain node");
        assert!(
            last.imports
                .iter()
                .any(|i| i.is_host_import && is_connection_for(i, http_interface)),
            "last chain node (innermost) should import handler from host"
        );

        // First node's handler source should be the last node
        let first_handler = first
            .imports
            .iter()
            .find(|i| is_connection_for(i, http_interface))
            .unwrap();
        assert_eq!(
            first_handler.source_instance, chain[1],
            "first node's handler source should be the last chain node"
        );
    }

    #[test]
    fn test_host_import_detection() {
        let bytes = wat::parse_str(two_middleware_chain_wat()).expect("failed to parse WAT");
        let graph = parse_component(&bytes).expect("failed to parse component");

        let host_interfaces = graph.host_interfaces();
        assert!(
            host_interfaces
                .iter()
                .any(|i| i.contains("wasi:http/handler")),
            "expected host handler interface, got: {:?}",
            host_interfaces
        );
    }

    #[test]
    fn test_resolve_alias() {
        let mut aliases = HashMap::new();
        // Chain: 5 -> 3 -> 1
        aliases.insert(5, 3);
        aliases.insert(3, 1);

        assert_eq!(resolve_alias(5, &aliases), 1);
        assert_eq!(resolve_alias(3, &aliases), 1);
        assert_eq!(resolve_alias(1, &aliases), 1); // no alias, returns self
        assert_eq!(resolve_alias(99, &aliases), 99); // unknown, returns self
    }
}
