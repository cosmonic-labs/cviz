use crate::model::{
    ComponentNode, CompositionGraph, ExportInfo, FuncSignature, InstanceInterface,
    InterfaceConnection, InterfaceType, InterfaceTypeId, TypeArena, TypeId, ValueType,
};
use crate::output::json::{
    FuncSignatureJson, InterfaceTypeJson, JsonCompositionGraph, JsonExport, ValueTypeJson,
};
use serde::de::Error;
use std::collections::BTreeMap;
use std::fs::File;

pub fn parse_json(json_reader: &File) -> anyhow::Result<CompositionGraph> {
    let graph = CompositionGraph::from_json_reader(json_reader)?;
    if let Err(e) = graph.validate() {
        serde_json::Error::custom(e.to_string());
    }
    Ok(graph)
}

pub fn parse_json_str(json: &str) -> anyhow::Result<CompositionGraph> {
    let graph = CompositionGraph::from_json_str(json)?;
    if let Err(e) = graph.validate() {
        serde_json::Error::custom(e.to_string());
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
        use std::collections::BTreeMap;

        let mut arena = TypeArena::default();
        let mut nodes = BTreeMap::new();

        for json_node in model.nodes {
            let mut node = ComponentNode::new(
                format!("${}", json_node.name), // restore `$` convention
                json_node.component_index,
                0,
            );

            for conn in json_node.imports {
                let interface_type = match conn.interface_type {
                    Some(json_ty) => {
                        Some(convert_interface_type(json_ty, &mut arena).map_err(|e| {
                            serde_json::Error::custom(format!(
                                "Failed to parse interface_type for {}: {}",
                                conn.interface, e
                            ))
                        })?)
                    }
                    None => None,
                };

                let connection = InterfaceConnection {
                    interface_name: conn.interface,
                    source_instance: conn.source_instance,
                    is_host_import: conn.is_host_import,
                    interface_type,
                    fingerprint: conn.fingerprint,
                };

                node.add_import(connection);
            }

            nodes.insert(json_node.id, node);
        }

        let mut component_exports = BTreeMap::new();
        for export in model.exports {
            component_exports.insert(
                export.interface.clone(),
                convert_export(export, &mut arena).map_err(|e| {
                    serde_json::Error::custom(format!("Failed to parse export for: {e}"))
                })?,
            );
        }

        Ok(CompositionGraph {
            nodes,
            component_exports,
            arena,
        })
    }
}

fn convert_export(json: JsonExport, arena: &mut TypeArena) -> Result<ExportInfo, String> {
    let (ty, fingerprint) = intern_interface_type(json.interface_type.unwrap(), arena)?;
    Ok(ExportInfo {
        source_instance: json.source_instance,
        fingerprint,
        ty,
    })
}

fn intern_interface_type(
    json: InterfaceTypeJson,
    arena: &mut TypeArena,
) -> Result<(InterfaceTypeId, String), String> {
    let ity = convert_interface_type(json, arena)?;
    Ok((arena.intern_interface(&ity), ity.fingerprint(arena)))
}

fn convert_interface_type(
    json: InterfaceTypeJson,
    arena: &mut TypeArena,
) -> Result<InterfaceType, String> {
    match json {
        InterfaceTypeJson::Func { params, results } => Ok(InterfaceType::Func(
            convert_func_signature(FuncSignatureJson { params, results }, arena)?,
        )),

        InterfaceTypeJson::Instance { functions } => {
            let mut funcs = BTreeMap::new();

            for (name, f) in functions {
                funcs.insert(name, convert_func_signature(f, arena)?);
            }

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
fn intern_value_type(json: ValueTypeJson, arena: &mut TypeArena) -> Result<TypeId, String> {
    let ty = match json {
        ValueTypeJson::Bool => ValueType::Bool,

        ValueTypeJson::List(inner) => {
            let inner_id = intern_value_type(*inner, arena)?;
            ValueType::List(inner_id)
        }

        ValueTypeJson::Tuple(items) => {
            let ids = items
                .into_iter()
                .map(|v| intern_value_type(*v, arena))
                .collect::<Result<Vec<_>, _>>()?;

            ValueType::Tuple(ids)
        }

        ValueTypeJson::Record(fields) => {
            let fields = fields
                .into_iter()
                .map(|(n, v)| Ok((n, intern_value_type(*v, arena)?)))
                .collect::<Result<Vec<_>, String>>()?;

            ValueType::Record(fields)
        }

        // convert everything recursively
        ValueTypeJson::Resource => ValueType::Resource,
        ValueTypeJson::AsyncHandle => ValueType::AsyncHandle,

        // TODO: FINISH THIS!
        _ => todo!(),
    };

    Ok(arena.intern_ty(ty))
}
