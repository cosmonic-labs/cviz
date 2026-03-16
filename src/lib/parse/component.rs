use crate::model::{ComponentNode, CompositionGraph, FuncSignature, InstanceInterface, InterfaceConnection, InterfaceType, TypeArena, ValueType, ValueTypeId};
use anyhow::Result;
use std::collections::HashMap;
use wirm::ir::component::refs::{GetCompRefs, GetItemRef};
use wirm::ir::component::visitor::{
    walk_structural, ComponentVisitor, ItemKind, ResolvedItem, VisitCtx,
};
use wirm::wasmparser::{
    ComponentAlias, ComponentExport, ComponentInstance, ComponentTypeRef, PrimitiveValType,
};
use wirm::{ConcreteFuncType, ConcreteType, ConcreteValType, Component};

/// Parse a WebAssembly component file and extract its composition graph
pub fn parse_component(buff: &[u8]) -> Result<CompositionGraph> {
    let component = Component::parse(buff, false, false).expect("Unable to parse");
    let mut visitor = Visitor::new();

    walk_structural(&component, &mut visitor);
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
impl ComponentVisitor<'_> for Visitor {
    fn enter_root_component(&mut self, _cx: &VisitCtx<'_>, _component: &Component<'_>) {
        self.comp_id_to_num.push(HashMap::new());
    }
    fn exit_root_component(&mut self, _cx: &VisitCtx<'_>, _component: &Component<'_>) {
        self.comp_id_to_num.pop();
    }
    fn enter_component(&mut self, _cx: &VisitCtx, id: u32, _component: &Component) {
        if let Some(outer) = self.comp_id_to_num.last_mut() {
            outer.insert(id, self.curr_comp_num);
        }
        self.curr_comp_num += 1;
        self.comp_id_to_num.push(HashMap::new());
    }

    fn exit_component(&mut self, _: &VisitCtx, _: u32, _component: &Component) {
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
                let instantiated_comp = if let ResolvedItem::Component(_, comp) =
                    cx.resolve(&instance.get_comp_refs().first().unwrap().ref_)
                {
                    Some(comp)
                } else {
                    None
                };

                let comp_num = self.comp_id_to_num.last().unwrap()[component_index];
                let mut node = ComponentNode::new(name, *component_index, comp_num);

                // Process the "with" arguments - these are the interface connections
                for arg in args.iter() {
                    let interface_name = arg.name.to_string();
                    let interface_type =
                        pull_type_info(&interface_name, &instantiated_comp, &mut self.graph);

                    // The arg.index is the instance providing this interface
                    // It might be an alias, so resolve it to the actual source instance
                    let item = cx.resolve(&arg.get_item_ref().ref_);
                    match item {
                        ResolvedItem::CompInst(inst_id, _) => {
                            let connection = InterfaceConnection::from_instance(
                                interface_name,
                                inst_id,
                                interface_type,
                                &self.graph.arena,
                            );
                            node.add_import(connection);
                        }
                        ResolvedItem::Import(id, imp) => {
                            if let ComponentTypeRef::Instance(_) = imp.ty {
                                let connection = InterfaceConnection::from_instance(
                                    interface_name,
                                    id,
                                    interface_type,
                                    &self.graph.arena,
                                );
                                node.add_import(connection);
                            }
                        }
                        ResolvedItem::Alias(_, alias) => {
                            resolve_inst_alias(
                                cx,
                                alias,
                                &interface_name,
                                interface_type,
                                &mut node,
                                &self.graph.arena,
                            );
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
            ResolvedItem::CompInst(inst_id, inst) => {
                let iface_type =
                    pull_export_type_from_instance(&export_name, inst, &mut self.graph, cx);
                self.graph.add_export(export_name, inst_id, iface_type);
            }
            ResolvedItem::Alias(_, alias) => {
                resolve_imp_alias(cx, alias, &export_name, &mut self.graph);
            }
            _ => {}
        }
    }
}

fn pull_export_type_from_instance(
    export_name: &str,
    inst: &ComponentInstance,
    graph: &mut CompositionGraph,
    cx: &VisitCtx,
) -> Option<InterfaceType> {
    let comp_ref = inst.get_comp_refs().into_iter().next()?;
    let comp = match cx.resolve(&comp_ref.ref_) {
        ResolvedItem::Component(_, c) => c,
        _ => return None,
    };
    concrete_to_interface_type(comp.concretize_export(export_name)?, &mut graph.arena)
}

fn pull_type_info(
    interface_name: &str,
    instantiated_comp: &Option<&Component>,
    graph: &mut CompositionGraph,
) -> Option<InterfaceType> {
    let comp = (*instantiated_comp)?;
    concrete_to_interface_type(comp.concretize_import(interface_name)?, &mut graph.arena)
}

fn concrete_to_interface_type(ty: ConcreteType, arena: &mut TypeArena) -> Option<InterfaceType> {
    match ty {
        ConcreteType::Instance(funcs) => {
            let functions = funcs
                .into_iter()
                .map(|(name, ft)| (name.to_string(), concrete_to_func_sig(ft, arena)))
                .collect();
            Some(InterfaceType::Instance(InstanceInterface { functions }))
        }
        ConcreteType::Func(ft) => Some(InterfaceType::Func(concrete_to_func_sig(ft, arena))),
        ConcreteType::Resource => None,
    }
}

fn intern(ty: ConcreteValType, arena: &mut TypeArena) -> ValueTypeId {
    let vt = concrete_to_val_type(ty, arena);
    arena.intern_val(vt)
}

fn concrete_to_func_sig(ft: ConcreteFuncType, arena: &mut TypeArena) -> FuncSignature {
    let params = ft
        .params
        .into_iter()
        .map(|(_, ty)| intern(ty, arena))
        .collect();
    let results = ft
        .result
        .map(|ty| intern(ty, arena))
        .into_iter()
        .collect();
    FuncSignature { params, results }
}

fn concrete_to_val_type(ty: ConcreteValType, arena: &mut TypeArena) -> ValueType {
    match ty {
        ConcreteValType::Primitive(p) => prim_to_val_type(p),
        ConcreteValType::Record(fields) => ValueType::Record(
            fields
                .into_iter()
                .map(|(name, ty)| (name.to_string(), intern(*ty, arena)))
                .collect(),
        ),
        ConcreteValType::Variant(cases) => ValueType::Variant(
            cases
                .into_iter()
                .map(|(name, ty)| {
                    (
                        name.to_string(),
                        ty.map(|t| intern(*t, arena)),
                    )
                })
                .collect(),
        ),
        ConcreteValType::List(ty) => ValueType::List(intern(*ty, arena)),
        ConcreteValType::FixedSizeList(ty, size) => {
            ValueType::FixedSizeList(intern(*ty, arena), size)
        }
        ConcreteValType::Tuple(types) => ValueType::Tuple(
            types
                .into_iter()
                .map(|ty| intern(ty, arena))
                .collect(),
        ),
        ConcreteValType::Option(ty) => {
            ValueType::Option(intern(*ty, arena))
        }
        ConcreteValType::Result { ok, err } => ValueType::Result {
            ok: ok.map(|t| intern(*t, arena)),
            err: err.map(|t| intern(*t, arena)),
        },
        ConcreteValType::Flags(names) => ValueType::Flags(names.iter().map(|s| s.to_string()).collect()),
        ConcreteValType::Enum(names) => ValueType::Enum(names.iter().map(|s| s.to_string()).collect()),
        ConcreteValType::Map(key, val) => ValueType::Map(
            intern(*key, arena),
            intern(*val, arena),
        ),
        ConcreteValType::Resource => ValueType::Resource,
        ConcreteValType::AsyncHandle => ValueType::AsyncHandle,
    }
}

fn prim_to_val_type(p: PrimitiveValType) -> ValueType {
    match p {
        PrimitiveValType::Bool => ValueType::Bool,
        PrimitiveValType::S8 => ValueType::S8,
        PrimitiveValType::U8 => ValueType::U8,
        PrimitiveValType::S16 => ValueType::S16,
        PrimitiveValType::U16 => ValueType::U16,
        PrimitiveValType::S32 => ValueType::S32,
        PrimitiveValType::U32 => ValueType::U32,
        PrimitiveValType::S64 => ValueType::S64,
        PrimitiveValType::U64 => ValueType::U64,
        PrimitiveValType::F32 => ValueType::F32,
        PrimitiveValType::F64 => ValueType::F64,
        PrimitiveValType::Char => ValueType::Char,
        PrimitiveValType::String => ValueType::String,
        PrimitiveValType::ErrorContext => ValueType::ErrorContext,
    }
}

fn resolve_inst_alias(
    cx: &VisitCtx,
    alias: &ComponentAlias,
    interface_name: &str,
    interface_type: Option<InterfaceType>,
    node: &mut ComponentNode,
    arena: &TypeArena,
) {
    let inst_ref = alias.get_item_ref();

    match cx.resolve(&inst_ref.ref_) {
        ResolvedItem::CompInst(inst_id, _) => {
            let connection = InterfaceConnection::from_instance(
                interface_name.to_string(),
                inst_id,
                interface_type,
                arena,
            );
            node.add_import(connection);
        }
        ResolvedItem::Alias(_, nested_alias) => resolve_inst_alias(
            cx,
            nested_alias,
            interface_name,
            interface_type,
            node,
            arena,
        ),
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
        ResolvedItem::CompInst(inst_id, inst) => {
            let iface_type = pull_export_type_from_instance(export_name, inst, graph, cx);
            graph.add_export(export_name.to_string(), inst_id, iface_type);
        }
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
    fn test_parse_composed_multiple() {
        let bytes = include_bytes!("../../../tests/fixtures/composed-multiple.wasm");
        let graph = parse_component(bytes).expect("failed to parse composed-multiple.wasm");
        assert!(!graph.nodes.is_empty(), "expected at least one component node");
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
