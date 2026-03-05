use std::collections::BTreeMap;
use sha2::{Digest, Sha256};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TypeId(u32);

/// Sentinel value for synthetic component instances (e.g., export wrappers)
pub const SYNTHETIC_COMPONENT: u32 = u32::MAX;

/// Represents a component instance in the composition
#[derive(Debug, Clone)]
pub struct ComponentNode {
    /// Instance name (e.g., "$srv", "$mdl-a")
    pub name: String,
    /// Which component is being instantiated
    pub component_index: u32,
    /// Which component is being instantiated, these are numbered
    /// from 0->N in order of the components as they show up in the binary!
    pub component_num: u32,
    /// List of interface connections (what it receives)
    pub imports: Vec<InterfaceConnection>,
}

impl ComponentNode {
    pub fn new(name: String, component_index: u32, component_num: u32) -> Self {
        Self {
            name,
            component_index,
            component_num,
            imports: Vec::new(),
        }
    }

    pub fn add_import(&mut self, connection: InterfaceConnection) {
        self.imports.push(connection);
    }

    /// Get a display label for the node
    pub fn display_label(&self) -> &str {
        self.name.trim_start_matches('$')
    }
}

/// Represents wiring between instances
#[derive(Debug, Clone)]
pub struct InterfaceConnection {
    /// e.g., "wasi:http/handler@0.3.0-rc-2026-01-06"
    pub interface_name: String,
    /// Which instance provides this
    pub source_instance: u32,
    /// Whether this comes from the host
    pub is_host_import: bool,

    pub interface_type: Option<InterfaceType>,
    pub fingerprint: Option<String>,
}

impl InterfaceConnection {
    pub fn from_instance(
        interface_name: String,
        source_instance: u32,
        interface_type: Option<InterfaceType>,
    ) -> Self {
        let fingerprint = interface_type
            .as_ref()
            .map(|t| t.fingerprint());

        Self {
            interface_name,
            source_instance,
            is_host_import: false,
            interface_type,
            fingerprint,
        }
    }

    /// Get a short label for the interface (just the interface name without package/version)
    pub fn short_label(&self) -> String {
        short_interface_name(&self.interface_name)
    }
}

#[derive(Debug, Clone)]
pub enum InterfaceType {
    Instance(InstanceInterface),
    Func(FuncSignature),
}
impl InterfaceType {
    pub fn fingerprint(&self) -> String {
        let s = canonical_interface(self);
        let hash = Sha256::digest(s.as_bytes());
        hex::encode(hash)
    }
}

#[derive(Debug, Clone)]
pub struct InstanceInterface {
    pub functions: BTreeMap<String, FuncSignature>,
}

#[derive(Debug, Clone)]
pub struct FuncSignature {
    pub params: Vec<ValueType>,
    pub results: Vec<ValueType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueType {
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

    List(Box<ValueType>),
    FixedSizeList(Box<ValueType>, u32),
    Tuple(Vec<ValueType>),
    Record(Vec<(String, ValueType)>),
    Variant(Vec<(String, Option<ValueType>)>),
    Enum(Vec<String>),
    Option(Box<ValueType>),
    Result {
        ok: Option<Box<ValueType>>,
        err: Option<Box<ValueType>>,
    },
    Flags(Vec<String>),
    Map(Box<ValueType>, Box<ValueType>),
}
impl ValueType {
    pub fn canonical(&self) -> String {
        match self {
            ValueType::Map(k, v) =>
                format!("map<{},{}>", k.canonical(), v.canonical()),

            ValueType::FixedSizeList(t, n) =>
                format!("array{}<{}>", n, t.canonical()),

            ValueType::List(t) =>
                format!("list<{}>", t.canonical()),

            ValueType::Option(t) =>
                format!("option<{}>", t.canonical()),

            ValueType::Tuple(ts) =>
                format!("tuple({})",
                        ts.iter()
                            .map(|t| t.canonical())
                            .collect::<Vec<_>>()
                            .join(",")),

            ValueType::Record(fields) =>
                format!("record{{{}}}",
                        fields.iter()
                            .map(|(n,t)| format!("{}:{}", n, t.canonical()))
                            .collect::<Vec<_>>()
                            .join(",")),

            ValueType::Variant(cases) =>
                format!("variant{{{}}}",
                        cases.iter()
                            .map(|(n,t)| match t {
                                Some(t) => format!("{}:{}", n, t.canonical()),
                                None => n.clone(),
                            })
                            .collect::<Vec<_>>()
                            .join(",")),

            ValueType::Enum(names) =>
                format!("enum{{{}}}", names.join(",")),

            ValueType::Flags(names) =>
                format!("flags{{{}}}", names.join(",")),

            ValueType::Result { ok, err } =>
                format!(
                    "result<{},{}>",
                    ok.as_ref().map(|t| t.canonical()).unwrap_or("_".into()),
                    err.as_ref().map(|t| t.canonical()).unwrap_or("_".into())
                ),

            // resource handles
            ValueType::Resource => "resource".into(),
            ValueType::AsyncHandle => "async_handle".into(),

            // primitives
            ValueType::Bool => "bool".into(),
            ValueType::S8 => "s8".into(),
            ValueType::U8 => "u8".into(),
            ValueType::S16 => "s16".into(),
            ValueType::U16 => "u16".into(),
            ValueType::S32 => "s32".into(),
            ValueType::U32 => "u32".into(),
            ValueType::S64 => "s64".into(),
            ValueType::U64 => "u64".into(),
            ValueType::F32 => "f32".into(),
            ValueType::F64 => "f64".into(),
            ValueType::Char => "char".into(),
            ValueType::String => "string".into(),
            ValueType::ErrorContext => "error-context".into(),
        }
    }
}

fn canonical_interface(iface: &InterfaceType) -> String {
    match iface {
        InterfaceType::Func(f) => canonical_func(f),

        InterfaceType::Instance(inst) => {
            let mut funcs: Vec<_> = inst.functions.iter().collect();
            funcs.sort_by(|a, b| a.0.cmp(b.0));

            let mut out = String::from("instance{");

            for (name, func) in funcs {
                out.push_str(name);
                out.push(':');
                out.push_str(&canonical_func(func));
                out.push(';');
            }

            out.push('}');
            out
        }
    }
}

fn canonical_func(f: &FuncSignature) -> String {
    let mut out = String::from("func(");

    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&p.canonical());
    }

    out.push_str(")->(");

    for (i, r) in f.results.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&r.canonical());
    }

    out.push(')');
    out
}

/// The complete parsed composition structure
#[derive(Debug, Default)]
pub struct CompositionGraph {
    /// All component instances, keyed by instance index
    pub nodes: BTreeMap<u32, ComponentNode>,
    /// What the composed component exports (interface name -> source instance)
    pub component_exports: BTreeMap<String, u32>,
}

impl CompositionGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, instance_index: u32, node: ComponentNode) {
        self.nodes.insert(instance_index, node);
    }

    pub fn get_node(&self, instance_index: u32) -> Option<&ComponentNode> {
        self.nodes.get(&instance_index)
    }

    pub fn add_export(&mut self, interface_name: String, source_instance: u32) {
        self.component_exports
            .insert(interface_name, source_instance);
    }

    /// Get all real (non-synthetic) component nodes
    pub fn real_nodes(&self) -> Vec<&ComponentNode> {
        self.nodes
            .values()
            .filter(|n| n.component_index != SYNTHETIC_COMPONENT)
            .collect()
    }

    /// Get sorted list of unique host interface names across all real nodes
    pub fn host_interfaces(&self) -> Vec<String> {
        let mut interfaces: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for node in self.real_nodes() {
            for import in &node.imports {
                if import.is_host_import {
                    interfaces.insert(import.interface_name.clone());
                }
            }
        }
        interfaces.into_iter().collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        for (iface, src) in &self.component_exports {
            if !self.nodes.contains_key(src) {
                return Err(format!(
                    "Export '{}' references unknown instance {}",
                    iface, src
                ));
            }
        }

        for (id, node) in &self.nodes {
            for conn in &node.imports {
                let src = conn.source_instance;
                if !self.nodes.contains_key(&src) {
                    return Err(format!(
                        "Instance {} imports from unknown instance {}",
                        id, src
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Extract a short interface name from a full interface path
/// e.g., "wasi:http/handler@0.3.0-rc-2026-01-06" -> "handler"
pub fn short_interface_name(full_name: &str) -> String {
    if let Some(slash_pos) = full_name.rfind('/') {
        let after_slash = &full_name[slash_pos + 1..];
        if let Some(at_pos) = after_slash.find('@') {
            return after_slash[..at_pos].to_string();
        }
        return after_slash.to_string();
    }
    full_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_short_label() {
        let conn = InterfaceConnection::from_instance(
            "wasi:http/handler@0.3.0-rc-2026-01-06".to_string(),
            0,
            None
        );
        assert_eq!(conn.short_label(), "handler");

        let conn2 = InterfaceConnection::from_instance("wasi:io/streams@0.2.0".to_string(), 1, None);
        assert_eq!(conn2.short_label(), "streams");
    }

    #[test]
    fn test_short_interface_name() {
        assert_eq!(short_interface_name("wasi:http/handler@0.3.0"), "handler");
        assert_eq!(short_interface_name("wasi:io/streams@0.2.0"), "streams");
        assert_eq!(short_interface_name("simple"), "simple");
    }
}
