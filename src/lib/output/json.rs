use crate::model::{
    CompositionGraph, FuncSignature, InterfaceConnection, InterfaceType, InternedId, TypeArena,
    ValueType, ValueTypeId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Serialize a [`CompositionGraph`] to JSON.
pub fn generate_json(graph: &CompositionGraph, pretty: bool) -> Result<String, serde_json::Error> {
    let model = generate_json_model(graph);
    if pretty {
        serde_json::to_string_pretty(&model)
    } else {
        serde_json::to_string(&model)
    }
}

fn generate_json_model(graph: &CompositionGraph) -> JsonCompositionGraph {
    let arena = &graph.arena;

    let nodes = graph
        .nodes
        .iter()
        .map(|(&id, node)| JsonNode {
            id,
            name: node.display_label().to_string(),
            component_index: node.component_index,
            component_num: node.component_num,
            imports: node
                .imports
                .iter()
                .map(|ic| JsonInterfaceConnection::from_ir(ic, arena))
                .collect(),
        })
        .collect();

    let exports = graph
        .component_exports
        .iter()
        .map(|(iface, info)| JsonExport {
            interface: iface.clone(),
            source_instance: info.source_instance,
            fingerprint: info.fingerprint.clone(),
            interface_type: match &info.ty {
                Some(InternedId::Interface(id)) => Some(InterfaceTypeJson::from_ir(
                    arena.lookup_interface(*id),
                    arena,
                )),
                _ => None,
            },
        })
        .collect();

    JsonCompositionGraph {
        version: 2,
        nodes,
        exports,
    }
}

#[derive(Deserialize, Serialize)]
pub struct JsonCompositionGraph {
    pub version: u32,
    pub nodes: Vec<JsonNode>,
    pub exports: Vec<JsonExport>,
}

#[derive(Deserialize, Serialize)]
pub struct JsonNode {
    pub id: u32,
    pub name: String,
    pub component_index: u32,
    pub component_num: u32,
    pub imports: Vec<JsonInterfaceConnection>,
}

#[derive(Deserialize, Serialize)]
pub struct JsonInterfaceConnection {
    /// Full interface name (e.g., "wasi:http/handler@0.3.0-rc-2026-01-06")
    pub interface: String,

    /// Short interface name (human-readable)
    pub short: String,

    /// Which instance provides this interface
    pub source_instance: u32,

    /// True if this is a host-provided import
    pub is_host_import: bool,

    /// Structured type of the interface, if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_type: Option<InterfaceTypeJson>,

    /// Deterministic fingerprint of the interface type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl JsonInterfaceConnection {
    pub fn from_ir(ic: &InterfaceConnection, arena: &TypeArena) -> Self {
        JsonInterfaceConnection {
            interface: ic.interface_name.clone(),
            short: ic.short_label(),
            source_instance: ic.source_instance,
            is_host_import: ic.is_host_import,
            interface_type: ic
                .interface_type
                .as_ref()
                .map(|t| InterfaceTypeJson::from_ir(t, arena)),
            fingerprint: ic.fingerprint.clone(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InterfaceTypeJson {
    /// Single function signature
    Func(FuncSignatureJson),

    /// Instance with named exported functions
    Instance {
        functions: BTreeMap<String, FuncSignatureJson>,
    },
}

impl InterfaceTypeJson {
    pub fn from_ir(it: &InterfaceType, arena: &TypeArena) -> Self {
        match it {
            InterfaceType::Func(f) => InterfaceTypeJson::Func(FuncSignatureJson::from_ir(f, arena)),
            InterfaceType::Instance(inst) => InterfaceTypeJson::Instance {
                functions: inst
                    .functions
                    .iter()
                    .map(|(n, f)| (n.clone(), FuncSignatureJson::from_ir(f, arena)))
                    .collect(),
            },
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct FuncSignatureJson {
    pub params: Vec<ValueTypeJson>,
    pub results: Vec<ValueTypeJson>,
}

impl FuncSignatureJson {
    fn from_ir(f: &FuncSignature, arena: &TypeArena) -> Self {
        FuncSignatureJson {
            params: f
                .params
                .iter()
                .map(|&id| ValueTypeJson::from_ir(id, arena))
                .collect(),
            results: f
                .results
                .iter()
                .map(|&id| ValueTypeJson::from_ir(id, arena))
                .collect(),
        }
    }
}

/// A fully-resolved value type with no index references, suitable for JSON serialization.
///
/// Recursive types (list, tuple, record, etc.) embed their inner types inline.
/// Serde internally-tagged format: every variant serializes as `{"type": "<name>", ...fields}`.
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ValueTypeJson {
    Bool,
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    S64,
    U64,
    F32,
    F64,
    Char,
    String,
    ErrorContext,
    Resource,
    AsyncHandle,
    List {
        elem: Box<ValueTypeJson>,
    },
    FixedSizeList {
        elem: Box<ValueTypeJson>,
        size: u32,
    },
    Tuple {
        items: Vec<ValueTypeJson>,
    },
    Record {
        fields: Vec<(std::string::String, ValueTypeJson)>,
    },
    Variant {
        cases: Vec<(std::string::String, Option<ValueTypeJson>)>,
    },
    Enum {
        cases: Vec<std::string::String>,
    },
    Flags {
        names: Vec<std::string::String>,
    },
    Option {
        some: Box<ValueTypeJson>,
    },
    Result {
        ok: Option<Box<ValueTypeJson>>,
        err: Option<Box<ValueTypeJson>>,
    },
    Map {
        key: Box<ValueTypeJson>,
        value: Box<ValueTypeJson>,
    },
}

impl ValueTypeJson {
    pub fn from_ir(id: ValueTypeId, arena: &TypeArena) -> Self {
        match arena.lookup_val(id) {
            ValueType::Bool => ValueTypeJson::Bool,
            ValueType::S8 => ValueTypeJson::S8,
            ValueType::U8 => ValueTypeJson::U8,
            ValueType::S16 => ValueTypeJson::S16,
            ValueType::U16 => ValueTypeJson::U16,
            ValueType::S32 => ValueTypeJson::S32,
            ValueType::U32 => ValueTypeJson::U32,
            ValueType::S64 => ValueTypeJson::S64,
            ValueType::U64 => ValueTypeJson::U64,
            ValueType::F32 => ValueTypeJson::F32,
            ValueType::F64 => ValueTypeJson::F64,
            ValueType::Char => ValueTypeJson::Char,
            ValueType::String => ValueTypeJson::String,
            ValueType::ErrorContext => ValueTypeJson::ErrorContext,
            ValueType::Resource => ValueTypeJson::Resource,
            ValueType::AsyncHandle => ValueTypeJson::AsyncHandle,
            ValueType::List(inner) => ValueTypeJson::List {
                elem: Box::new(ValueTypeJson::from_ir(*inner, arena)),
            },
            ValueType::FixedSizeList(inner, n) => ValueTypeJson::FixedSizeList {
                elem: Box::new(ValueTypeJson::from_ir(*inner, arena)),
                size: *n,
            },
            ValueType::Tuple(items) => ValueTypeJson::Tuple {
                items: items
                    .iter()
                    .map(|&t| ValueTypeJson::from_ir(t, arena))
                    .collect(),
            },
            ValueType::Record(fields) => ValueTypeJson::Record {
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), ValueTypeJson::from_ir(*t, arena)))
                    .collect(),
            },
            ValueType::Variant(cases) => ValueTypeJson::Variant {
                cases: cases
                    .iter()
                    .map(|(n, t)| (n.clone(), t.map(|t| ValueTypeJson::from_ir(t, arena))))
                    .collect(),
            },
            ValueType::Enum(names) => ValueTypeJson::Enum {
                cases: names.clone(),
            },
            ValueType::Flags(names) => ValueTypeJson::Flags {
                names: names.clone(),
            },
            ValueType::Option(inner) => ValueTypeJson::Option {
                some: Box::new(ValueTypeJson::from_ir(*inner, arena)),
            },
            ValueType::Result { ok, err } => ValueTypeJson::Result {
                ok: ok.map(|t| Box::new(ValueTypeJson::from_ir(t, arena))),
                err: err.map(|t| Box::new(ValueTypeJson::from_ir(t, arena))),
            },
            ValueType::Map(k, v) => ValueTypeJson::Map {
                key: Box::new(ValueTypeJson::from_ir(*k, arena)),
                value: Box::new(ValueTypeJson::from_ir(*v, arena)),
            },
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct JsonExport {
    pub interface: String,
    pub source_instance: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_type: Option<InterfaceTypeJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComponentNode, InterfaceConnection};

    fn test_graph() -> CompositionGraph {
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
        mw.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".to_string(),
            source_instance: 0,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(2, mw);

        graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, None);
        graph
    }

    #[test]
    fn test_full_json() {
        let graph = test_graph();
        let output = generate_json(&graph, true).unwrap();
        assert!(output.contains("srv"), "should show srv");
        assert!(output.contains("middleware"), "should show middleware");
        assert!(
            output.contains("wasi:http/handler@0.3.0"),
            "should show full interface name"
        );
    }

    #[test]
    fn test_empty_graph_json() {
        let graph = CompositionGraph::new();
        let output = generate_json(&graph, true).unwrap();
        assert!(output.contains("[]"));
    }
}
