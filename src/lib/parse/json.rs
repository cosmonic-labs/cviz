use crate::model::{ComponentNode, CompositionGraph, InterfaceConnection, InterfaceType};
use crate::output::json::JsonCompositionGraph;
use serde::de::Error;
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

        let mut nodes = BTreeMap::new();

        for json_node in model.nodes {
            let mut node = ComponentNode::new(
                format!("${}", json_node.name), // restore `$` convention
                json_node.component_index,
                0,
            );

            for conn in json_node.imports {
                let interface_type = match conn.interface_type {
                    Some(json_ty) => Some(InterfaceType::try_from(json_ty).map_err(|e| serde_json::Error::custom(format!(
                        "Failed to parse interface_type for {}: {}",
                        conn.interface, e
                    )))?),
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
            component_exports.insert(export.interface, export.source_instance);
        }

        Ok(CompositionGraph {
            nodes,
            component_exports,
        })
    }
}
