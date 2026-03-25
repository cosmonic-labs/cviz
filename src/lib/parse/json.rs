use crate::model::{
    ComponentNode, CompositionGraph, ExportInfo, FuncSignature, InstanceInterface,
    InterfaceConnection, InterfaceType, InternedId, TypeArena, ValueType, ValueTypeId,
};
use crate::output::json::{
    FuncSignatureJson, InterfaceTypeJson, JsonCompositionGraph, JsonExport, ValueTypeJson,
};
use serde::de::Error as _;
use std::collections::BTreeMap;
use std::fs::File;

pub fn parse_json(json_reader: &File) -> anyhow::Result<CompositionGraph> {
    let graph = CompositionGraph::from_json_reader(json_reader)?;
    if let Err(e) = graph.validate() {
        return Err(anyhow::anyhow!(e));
    }
    Ok(graph)
}

pub fn parse_json_str(json: &str) -> anyhow::Result<CompositionGraph> {
    let graph = CompositionGraph::from_json_str(json)?;
    if let Err(e) = graph.validate() {
        return Err(anyhow::anyhow!(e));
    }
    Ok(graph)
}

impl CompositionGraph {
    fn from_json_str(input: &str) -> Result<Self, serde_json::Error> {
        let model: JsonCompositionGraph = serde_json::from_str(input)?;
        Self::from_json_model(model)
    }
    fn from_json_reader<R: std::io::Read>(reader: R) -> Result<Self, serde_json::Error> {
        let model: JsonCompositionGraph = serde_json::from_reader(reader)?;
        Self::from_json_model(model)
    }
}

impl CompositionGraph {
    fn from_json_model(model: JsonCompositionGraph) -> Result<Self, serde_json::Error> {
        let mut arena = TypeArena::default();
        let mut nodes = BTreeMap::new();

        for json_node in model.nodes {
            let mut node = ComponentNode::new(
                format!("${}", json_node.name),
                json_node.component_index,
                json_node.component_num,
            );

            for conn in json_node.imports {
                let interface_type = conn
                    .interface_type
                    .map(|t| convert_interface_type(t, &mut arena))
                    .transpose()
                    .map_err(serde_json::Error::custom)?;

                node.add_import(InterfaceConnection {
                    interface_name: conn.interface,
                    source_instance: conn.source_instance,
                    is_host_import: conn.is_host_import,
                    interface_type,
                    fingerprint: conn.fingerprint,
                });
            }

            nodes.insert(json_node.id, node);
        }

        let mut component_exports = BTreeMap::new();
        for export in model.exports {
            let iface_name = export.interface.clone();
            let info = convert_export(export, &mut arena).map_err(serde_json::Error::custom)?;
            component_exports.insert(iface_name, info);
        }

        Ok(CompositionGraph {
            nodes,
            component_exports,
            arena,
        })
    }
}

fn convert_export(json: JsonExport, arena: &mut TypeArena) -> Result<ExportInfo, String> {
    let (ty, fingerprint) = match json.interface_type {
        Some(it) => {
            let ity = convert_interface_type(it, arena)?;
            let id = arena.intern_interface(&ity);
            let fp = ity.fingerprint(arena);
            (Some(InternedId::Interface(id)), Some(fp))
        }
        None => (None, json.fingerprint),
    };
    Ok(ExportInfo {
        source_instance: json.source_instance,
        fingerprint,
        ty,
    })
}

fn convert_interface_type(
    json: InterfaceTypeJson,
    arena: &mut TypeArena,
) -> Result<InterfaceType, String> {
    match json {
        InterfaceTypeJson::Func(f) => Ok(InterfaceType::Func(convert_func_signature(f, arena)?)),
        InterfaceTypeJson::Instance { functions } => {
            let funcs = functions
                .into_iter()
                .map(|(name, f)| Ok((name, convert_func_signature(f, arena)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            Ok(InterfaceType::Instance(InstanceInterface {
                functions: funcs,
            }))
        }
    }
}

fn convert_func_signature(
    json: FuncSignatureJson,
    arena: &mut TypeArena,
) -> Result<FuncSignature, String> {
    let params = json
        .params
        .into_iter()
        .map(|v| intern_value_type(v, arena))
        .collect::<Result<Vec<_>, _>>()?;
    let results = json
        .results
        .into_iter()
        .map(|v| intern_value_type(v, arena))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FuncSignature { params, results })
}

fn intern_value_type(json: ValueTypeJson, arena: &mut TypeArena) -> Result<ValueTypeId, String> {
    let ty = match json {
        ValueTypeJson::Bool => ValueType::Bool,
        ValueTypeJson::S8 => ValueType::S8,
        ValueTypeJson::U8 => ValueType::U8,
        ValueTypeJson::S16 => ValueType::S16,
        ValueTypeJson::U16 => ValueType::U16,
        ValueTypeJson::S32 => ValueType::S32,
        ValueTypeJson::U32 => ValueType::U32,
        ValueTypeJson::S64 => ValueType::S64,
        ValueTypeJson::U64 => ValueType::U64,
        ValueTypeJson::F32 => ValueType::F32,
        ValueTypeJson::F64 => ValueType::F64,
        ValueTypeJson::Char => ValueType::Char,
        ValueTypeJson::String => ValueType::String,
        ValueTypeJson::ErrorContext => ValueType::ErrorContext,
        ValueTypeJson::Resource => ValueType::Resource,
        ValueTypeJson::AsyncHandle => ValueType::AsyncHandle,
        ValueTypeJson::List { elem } => ValueType::List(intern_value_type(*elem, arena)?),
        ValueTypeJson::FixedSizeList { elem, size } => {
            ValueType::FixedSizeList(intern_value_type(*elem, arena)?, size)
        }
        ValueTypeJson::Tuple { items } => ValueType::Tuple(
            items
                .into_iter()
                .map(|v| intern_value_type(v, arena))
                .collect::<Result<_, _>>()?,
        ),
        ValueTypeJson::Record { fields } => ValueType::Record(
            fields
                .into_iter()
                .map(|(n, v)| Ok((n, intern_value_type(v, arena)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
        ValueTypeJson::Variant { cases } => ValueType::Variant(
            cases
                .into_iter()
                .map(|(n, v)| Ok((n, v.map(|v| intern_value_type(v, arena)).transpose()?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
        ValueTypeJson::Enum { cases } => ValueType::Enum(cases),
        ValueTypeJson::Flags { names } => ValueType::Flags(names),
        ValueTypeJson::Option { some } => ValueType::Option(intern_value_type(*some, arena)?),
        ValueTypeJson::Result { ok, err } => ValueType::Result {
            ok: ok.map(|v| intern_value_type(*v, arena)).transpose()?,
            err: err.map(|v| intern_value_type(*v, arena)).transpose()?,
        },
        ValueTypeJson::Map { key, value } => ValueType::Map(
            intern_value_type(*key, arena)?,
            intern_value_type(*value, arena)?,
        ),
    };
    Ok(arena.intern_val(ty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ComponentNode, FuncSignature, InstanceInterface, InterfaceConnection, InterfaceType,
        ValueType,
    };
    use crate::output::json::generate_json;
    use std::collections::BTreeMap;

    /// Serialize a graph to JSON then parse it back, returning the round-tripped graph.
    fn round_trip(graph: &CompositionGraph) -> CompositionGraph {
        let json = generate_json(graph, false).expect("serialization failed");
        parse_json_str(&json).expect("deserialization failed")
    }

    #[test]
    fn test_round_trip_basic_graph() {
        let mut graph = CompositionGraph::new();

        let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
        srv.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: 0,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, srv);

        let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: 1,
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(2, mw);

        graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, None);

        let rt = round_trip(&graph);

        assert_eq!(rt.nodes.len(), 2);
        assert_eq!(rt.component_exports.len(), 1);

        let srv = rt.nodes.get(&1).expect("node 1 missing");
        assert_eq!(srv.display_label(), "srv");
        assert_eq!(srv.component_index, 0);
        assert_eq!(srv.imports.len(), 1);
        assert!(srv.imports[0].is_host_import);
        assert_eq!(srv.imports[0].interface_name, "wasi:http/handler@0.3.0");

        let mw = rt.nodes.get(&2).expect("node 2 missing");
        assert_eq!(mw.imports.len(), 1);
        assert!(!mw.imports[0].is_host_import);
        assert_eq!(mw.imports[0].source_instance, 1);

        assert!(rt.component_exports.contains_key("wasi:http/handler@0.3.0"));
    }

    #[test]
    fn test_round_trip_typed_interface() {
        // Build a graph with a typed instance interface that uses several complex types:
        // greet(name: string, count: u32) -> result<list<string>, u32>
        // status() -> record { code: u32, message: string }
        let mut graph = CompositionGraph::new();
        let arena = &mut graph.arena;

        let str_id = arena.intern_val(ValueType::String);
        let u32_id = arena.intern_val(ValueType::U32);
        let list_str = arena.intern_val(ValueType::List(str_id));
        let result_ty = arena.intern_val(ValueType::Result {
            ok: Some(list_str),
            err: Some(u32_id),
        });
        let record_ty = arena.intern_val(ValueType::Record(vec![
            ("code".to_string(), u32_id),
            ("message".to_string(), str_id),
        ]));

        let mut functions = BTreeMap::new();
        functions.insert(
            "greet".to_string(),
            FuncSignature {
                params: vec![str_id, u32_id],
                results: vec![result_ty],
            },
        );
        functions.insert(
            "status".to_string(),
            FuncSignature {
                params: vec![],
                results: vec![record_ty],
            },
        );

        let iface = InterfaceType::Instance(InstanceInterface { functions });
        let fingerprint = iface.fingerprint(arena);

        let mut node = ComponentNode::new("$svc".to_string(), 0, 0);
        node.add_import(InterfaceConnection {
            interface_name: "my:pkg/api".to_string(),
            source_instance: 0,
            is_host_import: true,
            interface_type: Some(iface),
            fingerprint: Some(fingerprint.clone()),
        });
        graph.add_node(1, node);

        let rt = round_trip(&graph);

        let node = rt.nodes.get(&1).expect("node missing");
        let conn = &node.imports[0];

        // Fingerprint survives round-trip
        assert_eq!(conn.fingerprint.as_deref(), Some(fingerprint.as_str()));

        // Type info is present
        let iface = conn
            .interface_type
            .as_ref()
            .expect("interface type missing");
        let InterfaceType::Instance(inst) = iface else {
            panic!("expected Instance, got {:?}", iface);
        };

        assert!(inst.functions.contains_key("greet"), "greet missing");
        assert!(inst.functions.contains_key("status"), "status missing");

        let greet = &inst.functions["greet"];
        assert_eq!(greet.params.len(), 2);
        assert_eq!(greet.results.len(), 1);

        // The result type should round-trip as result<list<string>, u32>
        assert!(
            matches!(
                rt.arena.lookup_val(greet.results[0]),
                ValueType::Result { .. }
            ),
            "result type should survive round-trip"
        );

        let status = &inst.functions["status"];
        assert_eq!(status.params.len(), 0);
        assert_eq!(status.results.len(), 1);
        assert!(
            matches!(rt.arena.lookup_val(status.results[0]), ValueType::Record(_)),
            "record type should survive round-trip"
        );
    }
}
