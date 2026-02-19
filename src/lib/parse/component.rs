use anyhow::Result;
use std::collections::HashMap;
use wirm::ir::component::refs::GetItemRef;
use wirm::ir::component::visitor::{
    traverse_component, ComponentVisitor, ItemKind, ResolvedItem, VisitCtx,
};
use wirm::wasmparser::{ComponentAlias, ComponentExport, ComponentInstance, ComponentTypeRef};
use wirm::Component;

use crate::model::{ComponentNode, CompositionGraph, InterfaceConnection};

/// Parse a WebAssembly component file and extract its composition graph
pub fn parse_component(buff: &[u8]) -> Result<CompositionGraph> {
    let component = Component::parse(buff, false, false).expect("Unable to parse");
    let mut visitor = Visitor::new();

    traverse_component(&component, &mut visitor);
    visitor.postprocess();
    Ok(visitor.graph)
}
struct Visitor {
    curr_comp_num: u32,
    comp_id_to_num: Vec<HashMap<u32, u32>>,
    graph: CompositionGraph,
}
impl Visitor {
    pub fn new() -> Self {
        Self {
            curr_comp_num: 0,
            comp_id_to_num: Vec::new(),
            graph: CompositionGraph::new(),
        }
    }
    pub fn postprocess(&mut self) {
        // Mark host imports on the connections
        // Imports that aren't from a node inside the component graph are actually imported from the host.
        let all_node_inst_ids = self.graph.nodes.keys().copied().collect::<Vec<_>>();
        for node in self.graph.nodes.values_mut() {
            for import in &mut node.imports {
                if !all_node_inst_ids.contains(&import.source_instance) {
                    import.is_host_import = true;
                }
            }
        }
    }
}
impl ComponentVisitor for Visitor {
    fn enter_component(&mut self, _cx: &VisitCtx, id: Option<u32>, _component: &Component) {
        // only handle the internal components!
        if let Some(id) = id {
            if let Some(outer) = self.comp_id_to_num.last_mut() {
                outer.insert(id, self.curr_comp_num);
            }
            self.curr_comp_num += 1;
        }
        self.comp_id_to_num.push(HashMap::new());
    }

    fn exit_component(&mut self, _: &VisitCtx, _: Option<u32>, _component: &Component) {
        self.comp_id_to_num.pop();
    }

    // Process component instances - ** this is where the composition wiring lives **
    fn visit_comp_instance(&mut self, cx: &VisitCtx, id: u32, instance: &ComponentInstance) {
        let name = cx
            .lookup_comp_inst_name(id)
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("instance_{}", id));
        match instance {
            ComponentInstance::Instantiate {
                component_index,
                args,
            } => {
                let comp_num = self.comp_id_to_num.last().unwrap()[component_index];
                let mut node = ComponentNode::new(name, *component_index, comp_num);

                // Process the "with" arguments - these are the interface connections
                for arg in args.iter() {
                    let interface_name = arg.name.to_string();

                    // The arg.index is the instance providing this interface
                    // It might be an alias, so resolve it to the actual source instance
                    let item = cx.resolve(&arg.get_item_ref().ref_);
                    match item {
                        ResolvedItem::CompInst(inst_id, _) => {
                            let connection =
                                InterfaceConnection::from_instance(interface_name, inst_id);
                            node.add_import(connection);
                        }
                        ResolvedItem::Import(id, imp) => {
                            if let ComponentTypeRef::Instance(_) = imp.ty {
                                let connection =
                                    InterfaceConnection::from_instance(interface_name, id);
                                node.add_import(connection);
                            }
                        }
                        ResolvedItem::Alias(_, alias) => {
                            resolve_inst_alias(cx, alias, &interface_name, &mut node);
                        }
                        _ => {}
                    }
                }

                self.graph.add_node(id, node);
            }
            ComponentInstance::FromExports(_) => {
                // This is a synthetic instance created from exports
                // These often wrap host imports - we don't track them as nodes
                // since they're just interface bundles, not actual components
            }
        }
    }
    fn visit_comp_export(&mut self, cx: &VisitCtx, _: ItemKind, _: u32, export: &ComponentExport) {
        let export_name = export.name.0.to_string();
        let item = cx.resolve(&export.get_item_ref().ref_);

        // Only track instance exports
        match item {
            ResolvedItem::CompInst(inst_id, _) => {
                self.graph.add_export(export_name, inst_id);
            }
            ResolvedItem::Alias(_, alias) => {
                resolve_imp_alias(cx, alias, &export_name, &mut self.graph);
            }
            _ => {}
        }
    }
}

fn resolve_inst_alias(
    cx: &VisitCtx,
    alias: &ComponentAlias,
    interface_name: &str,
    node: &mut ComponentNode,
) {
    let inst_ref = alias.get_item_ref();

    match cx.resolve(&inst_ref.ref_) {
        ResolvedItem::CompInst(inst_id, _) => {
            let connection =
                InterfaceConnection::from_instance(interface_name.to_string(), inst_id);
            node.add_import(connection);
        }
        ResolvedItem::Alias(_, nested_alias) => {
            resolve_inst_alias(cx, nested_alias, interface_name, node)
        }
        _ => {}
    }
}
fn resolve_imp_alias(
    cx: &VisitCtx,
    alias: &ComponentAlias,
    export_name: &str,
    graph: &mut CompositionGraph,
) {
    let inst_ref = alias.get_item_ref();

    match cx.resolve(&inst_ref.ref_) {
        ResolvedItem::CompInst(inst_id, _) => graph.add_export(export_name.to_string(), inst_id),
        ResolvedItem::Alias(_, nested_alias) => {
            resolve_imp_alias(cx, nested_alias, export_name, graph)
        }
        _ => {}
    }
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
                .any(|k| k.contains("wasi:http/handler")),
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
}
