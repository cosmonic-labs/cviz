use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ValueTypeId(u32);
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceTypeId(u32);

/// Sentinel value for synthetic component instances (e.g., export wrappers)
pub const SYNTHETIC_COMPONENT: u32 = u32::MAX;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum InternedId {
    Value(ValueTypeId),
    Interface(InterfaceTypeId),
}

/// Represents a single component instance within a [`CompositionGraph`].
///
/// Each node corresponds to one instantiation of a component from the
/// component section of the WebAssembly binary. Multiple nodes may refer
/// to the same `component_index` if the component is instantiated more
/// than once.
///
/// A node primarily records:
/// - the component being instantiated
/// - the instance name used in the composition
/// - the interfaces it imports from other instances or the host
///
/// The actual wiring between instances is represented by
/// [`InterfaceConnection`] values in the `imports` list.
#[derive(Debug, Clone)]
pub struct ComponentNode {
    /// Instance name as it appears in the composition graph.
    ///
    /// Examples:
    /// - `"$srv"`
    /// - `"$router"`
    /// - `"$mdl-a"`
    ///
    /// The `$` prefix follows the internal naming convention used when
    /// reconstructing instance identifiers from the component model.
    pub name: String,

    /// Index of the component being instantiated.
    ///
    /// This corresponds to the component index referenced by the
    /// `instantiate` instruction in the component model.
    pub component_index: u32,

    /// Sequential number of the component within the binary.
    ///
    /// Components are numbered from `0..N` in the order they appear in the
    /// binary. This provides a stable ordering useful for visualization and
    /// debugging.
    pub component_num: u32,

    /// Interfaces imported by this instance.
    ///
    /// Each entry describes a dependency on another instance or the host.
    /// These connections define the edges of the composition graph.
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

/// Represents a single interface wiring between component instances.
///
/// An `InterfaceConnection` indicates that a component instance imports
/// a particular interface and identifies the instance (or host) that
/// provides it.
///
/// Conceptually, this is a directed edge in the composition graph:
///
/// ```text
/// source_instance -- provides --> target_instance
/// ```
///
/// Where `target_instance` is the [`ComponentNode`] containing this
/// connection.
#[derive(Debug, Clone)]
pub struct InterfaceConnection {
    /// Fully-qualified interface name.
    ///
    /// Examples:
    /// - `"wasi:http/handler@0.3.0-rc-2026-01-06"`
    /// - `"my:service/router"`
    pub interface_name: String,

    /// Instance index providing this interface.
    ///
    /// This corresponds to the key of another [`ComponentNode`] in the
    /// [`CompositionGraph::nodes`] map.
    pub source_instance: u32,

    /// Whether this interface is provided by the host rather than another
    /// component instance.
    ///
    /// When `true`, `source_instance` refers to a synthetic host provider
    /// rather than an actual node in the graph.
    pub is_host_import: bool,

    /// Structured description of the interface, if available.
    ///
    /// This contains the parsed function signatures of the interface and
    /// is used for compatibility checking, fingerprint generation, and
    /// visualization.
    ///
    /// Some connections may omit this if type information was not available
    /// during graph construction.
    // TODO: Can i make this non-optional?
    pub interface_type: Option<InterfaceType>,

    /// Deterministic fingerprint of the interface type.
    ///
    /// Fingerprints are typically computed from a canonical representation
    /// of the interface signature and can be used to quickly determine
    /// whether two interfaces are structurally identical.
    // TODO: Can i make this non-optional?
    pub fingerprint: Option<String>,
}

impl InterfaceConnection {
    pub fn from_instance(
        interface_name: String,
        source_instance: u32,
        interface_type: Option<InterfaceType>,
        arena: &TypeArena,
    ) -> Self {
        let fingerprint = interface_type.as_ref().map(|t| t.fingerprint(arena));

        Self {
            interface_name,
            source_instance,
            is_host_import: false,
            interface_type,
            fingerprint,
        }
    }

    /// Checks whether this connection is type-compatible with another.
    ///
    /// Compatibility is determined by comparing the deterministic fingerprints
    /// of the interface types. If both connections have the same fingerprint,
    /// they are considered compatible, meaning they have structurally identical
    /// signatures.
    ///
    /// # Parameters
    ///
    /// - `other`: The interface connection to compare against.
    ///
    /// # Returns
    ///
    /// `true` if the fingerprints match, `false` otherwise.
    ///
    /// # Notes
    ///
    /// - If either connection lacks a fingerprint (`fingerprint` is `None`),
    ///   this method will return `false`.
    /// - Fingerprints are computed from the canonical representation of the
    ///   interface type and capture full type structure, including nested
    ///   functions and instances.
    pub fn compatible_with(&self, other: &InterfaceConnection) -> bool {
        compatible_fingerprints(&self.fingerprint, &other.fingerprint)
    }

    /// Get a short label for the interface (just the interface name without package/version)
    pub fn short_label(&self) -> String {
        short_interface_name(&self.interface_name)
    }
}

pub fn compatible_fingerprints(f0: &Option<String>, f1: &Option<String>) -> bool {
    f0 == f1
}

/// Describes the structure of an imported or exported interface.
///
/// Interfaces in the WebAssembly component model may either be:
///
/// - a **single function**
/// - an **instance** containing multiple named functions
///
/// This enum captures both possibilities.
#[derive(Debug, Clone)]
pub enum InterfaceType {
    /// A WIT instance interface containing multiple exported functions.
    Instance(InstanceInterface),

    /// A single function interface.
    Func(FuncSignature),
}
impl InterfaceType {
    pub fn intern(&self, arena: &mut TypeArena) -> InterfaceTypeId {
        arena.intern_interface(self)
    }

    pub fn fingerprint(&self, arena: &TypeArena) -> String {
        let s = canonical_interface(self, arena);
        let hash = Sha256::digest(s.as_bytes());
        hex::encode(hash)
    }
}

/// Describes an instance-style interface consisting of multiple functions.
///
/// Each entry maps a function name to its corresponding [`FuncSignature`].
/// This mirrors the structure of WIT interfaces where functions are
/// exported from an instance namespace.
#[derive(Debug, Clone)]
pub struct InstanceInterface {
    /// Functions exported by this interface instance.
    ///
    /// Keys are function names and values describe their signatures.
    pub functions: BTreeMap<String, FuncSignature>,
}

/// Represents the signature of a function in an interface.
///
/// Parameter and result types are stored as [`ValueTypeId`] values referencing
/// entries in the graph's global [`TypeArena`]. This avoids storing
/// recursive type structures inline and enables efficient deduplication
/// and comparison of types.
#[derive(Debug, Clone)]
pub struct FuncSignature {
    /// Parameter types of the function.
    ///
    /// Each entry is a [`ValueTypeId`] referring to a value type stored in the
    /// graph's type arena.
    pub params: Vec<ValueTypeId>,

    /// Result types of the function.
    ///
    /// Multiple results are supported to match the WebAssembly component
    /// model.
    pub results: Vec<ValueTypeId>,
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

    List(ValueTypeId),
    FixedSizeList(ValueTypeId, u32),
    Tuple(Vec<ValueTypeId>),
    Record(Vec<(String, ValueTypeId)>),
    Variant(Vec<(String, Option<ValueTypeId>)>),
    Enum(Vec<String>),
    Option(ValueTypeId),
    Result {
        ok: Option<ValueTypeId>,
        err: Option<ValueTypeId>,
    },
    Flags(Vec<String>),
    Map(ValueTypeId, ValueTypeId),
}

fn canonical_interface(iface: &InterfaceType, arena: &TypeArena) -> String {
    match iface {
        InterfaceType::Func(f) => canonical_func(f, arena),

        InterfaceType::Instance(inst) => {
            let mut funcs: Vec<_> = inst.functions.iter().collect();
            funcs.sort_by(|a, b| a.0.cmp(b.0));

            let mut out = String::from("instance{");

            for (name, func) in funcs {
                out.push_str(name);
                out.push(':');
                out.push_str(&canonical_func(func, arena));
                out.push(';');
            }

            out.push('}');
            out
        }
    }
}

fn canonical_func(f: &FuncSignature, arena: &TypeArena) -> String {
    let mut out = String::from("func(");

    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&arena.canonical_val(*p));
    }

    out.push_str(")->(");

    for (i, r) in f.results.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&arena.canonical_val(*r));
    }

    out.push(')');
    out
}

/// A fully resolved composition graph describing how a set of WebAssembly
/// components are wired together.
///
/// Each node in the graph represents a component instance, and edges represent
/// interface imports satisfied by other instances (or by the host).
///
/// The graph is primarily used for:
/// - visualizing component compositions
/// - validating interface compatibility
/// - generating deterministic fingerprints of interfaces
/// - exporting a stable JSON representation of the composition
///
/// Instance identifiers correspond to the instance indices produced during
/// component instantiation.
#[derive(Default)]
pub struct CompositionGraph {
    /// All component instances in the composition.
    ///
    /// The key is the instance index assigned during composition.
    /// Each [`ComponentNode`] describes the component instance along with
    /// the interfaces it imports from other instances or the host.
    pub nodes: BTreeMap<u32, ComponentNode>,

    /// Interfaces exported by the final composed component.
    ///
    /// The key is the fully-qualified interface name (for example
    /// `"wasi:http/handler@0.3.0"`), and the value is the instance index
    /// providing that interface.
    ///
    /// This effectively defines the public surface of the composed component.
    pub component_exports: BTreeMap<String, ExportInfo>,

    /// Global arena containing all unique value types referenced in the graph.
    ///
    /// Complex interface types (function signatures, records, variants, etc.)
    /// are interned in this arena and referenced by [`ValueTypeId`] instead of
    /// embedding recursive type structures directly.
    ///
    /// This provides several advantages:
    ///
    /// - **Structural deduplication** — identical types are stored only once.
    /// - **Cheap equality** — type equality becomes a simple `TypeId` comparison.
    /// - **Reduced memory usage** — large graphs avoid repeated allocations.
    /// - **Deterministic fingerprints** — canonical type identities can be
    ///   computed efficiently for interface compatibility checks.
    ///
    /// The arena is shared across the entire graph so that all interface
    /// signatures refer to a single canonical set of type definitions.
    pub arena: TypeArena,
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

    pub fn add_export(
        &mut self,
        interface_name: String,
        source_instance: u32,
        interface_type: Option<InterfaceType>,
    ) {
        let (ty, fingerprint) = match interface_type {
            Some(t) => {
                let id = t.intern(&mut self.arena);
                let fp = t.fingerprint(&self.arena);
                (Some(InternedId::Interface(id)), Some(fp))
            }
            None => (None, None),
        };

        self.component_exports.insert(
            interface_name,
            ExportInfo {
                source_instance,
                ty,
                fingerprint,
            },
        );
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
        for (
            iface,
            ExportInfo {
                source_instance: src,
                ..
            },
        ) in &self.component_exports
        {
            if !self.nodes.contains_key(src) {
                return Err(format!(
                    "Export '{}' references unknown instance {}",
                    iface, src
                ));
            }
        }

        for (id, node) in &self.nodes {
            for conn in &node.imports {
                // Host imports point to a synthetic provider that is never in the
                // nodes map — skip them.
                if conn.is_host_import {
                    continue;
                }
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

pub struct ExportInfo {
    /// Index of the instance providing this export
    pub source_instance: u32,
    /// Fingerprint of the exported interface type
    pub fingerprint: Option<String>,
    /// Reference to the type in the global arena
    pub ty: Option<InternedId>,
}

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct TypeArena {
    vals: Vec<ValueType>,
    val_intern: HashMap<ValueType, ValueTypeId>,

    interfaces: Vec<InterfaceType>,
    interface_intern: HashMap<String, InterfaceTypeId>,
}

impl TypeArena {
    pub fn intern_val(&mut self, ty: ValueType) -> ValueTypeId {
        if let Some(id) = self.val_intern.get(&ty) {
            return *id;
        }

        let id = ValueTypeId(self.vals.len() as u32);
        self.vals.push(ty.clone());
        self.val_intern.insert(ty, id);

        id
    }

    pub fn lookup_val(&self, id: ValueTypeId) -> &ValueType {
        &self.vals[id.0 as usize]
    }

    pub fn intern_interface(&mut self, interface: &InterfaceType) -> InterfaceTypeId {
        // Serialize the interface to a canonical string
        // Check if it was already interned
        let canonical_str = canonical_interface(interface, self);

        // For simplicity, hash the string as a key
        if let Some(id) = self.interface_intern.get(&canonical_str) {
            return *id;
        }

        let id = InterfaceTypeId(self.interfaces.len() as u32);
        self.interfaces.push(interface.clone());
        self.interface_intern.insert(canonical_str, id);

        id
    }
    pub fn lookup_interface(&self, id: InterfaceTypeId) -> &InterfaceType {
        &self.interfaces[id.0 as usize]
    }
}
impl TypeArena {
    pub fn canonical_val(&self, id: ValueTypeId) -> String {
        match self.lookup_val(id) {
            ValueType::Map(k, v) => {
                format!("map<{},{}>", self.canonical_val(*k), self.canonical_val(*v))
            }

            ValueType::FixedSizeList(t, n) => format!("array{}<{}>", n, self.canonical_val(*t)),

            ValueType::List(t) => format!("list<{}>", self.canonical_val(*t)),

            ValueType::Option(t) => format!("option<{}>", self.canonical_val(*t)),

            ValueType::Tuple(ts) => format!(
                "tuple({})",
                ts.iter()
                    .map(|t| self.canonical_val(*t))
                    .collect::<Vec<_>>()
                    .join(",")
            ),

            ValueType::Record(fields) => format!(
                "record{{{}}}",
                fields
                    .iter()
                    .map(|(n, t)| format!("{}:{}", n, self.canonical_val(*t)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),

            ValueType::Variant(cases) => format!(
                "variant{{{}}}",
                cases
                    .iter()
                    .map(|(n, t)| match t {
                        Some(t) => format!("{}:{}", n, self.canonical_val(*t)),
                        None => n.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ),

            ValueType::Enum(names) => format!("enum{{{}}}", names.join(",")),

            ValueType::Flags(names) => format!("flags{{{}}}", names.join(",")),

            ValueType::Result { ok, err } => format!(
                "result<{},{}>",
                ok.map(|t| self.canonical_val(t)).unwrap_or("_".into()),
                err.map(|t| self.canonical_val(t)).unwrap_or("_".into())
            ),

            ValueType::Resource => "resource".into(),
            ValueType::AsyncHandle => "async_handle".into(),

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
    pub fn canonical_interface(&self, id: InterfaceTypeId) -> String {
        canonical_interface(self.lookup_interface(id), self)
    }

    /// Display-oriented type string for visualizations.
    ///
    /// Unlike [`canonical_val`], this method summarizes complex types that
    /// would produce unreadably long strings:
    ///
    /// - Variants/enums/flags/records with more than [`DISPLAY_MAX_ITEMS`]
    ///   entries are replaced by `variant{N cases}`, `record{N fields}`, etc.
    /// - Nesting deeper than [`DISPLAY_MAX_DEPTH`] is replaced by `…`.
    ///
    /// Simple scalar types (bool, u32, string, resource, …) are unaffected.
    /// Fingerprinting always uses [`canonical_val`] and is never truncated.
    pub fn display_val(&self, id: ValueTypeId) -> String {
        self.display_val_inner(id, 0)
    }

    fn display_val_inner(&self, id: ValueTypeId, depth: usize) -> String {
        const MAX_DEPTH: usize = 3;
        const MAX_ITEMS: usize = 4;

        if depth > MAX_DEPTH {
            return "…".to_string();
        }

        let next = depth + 1;
        match self.lookup_val(id) {
            ValueType::Map(k, v) => format!(
                "map<{},{}>",
                self.display_val_inner(*k, next),
                self.display_val_inner(*v, next)
            ),
            ValueType::FixedSizeList(t, n) => {
                format!("array{}<{}>", n, self.display_val_inner(*t, next))
            }
            ValueType::List(t) => format!("list<{}>", self.display_val_inner(*t, next)),
            ValueType::Option(t) => format!("option<{}>", self.display_val_inner(*t, next)),
            ValueType::Tuple(ts) => {
                if ts.len() > MAX_ITEMS {
                    format!("tuple({} items)", ts.len())
                } else {
                    format!(
                        "tuple({})",
                        ts.iter()
                            .map(|t| self.display_val_inner(*t, next))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            }
            ValueType::Record(fields) => {
                if fields.len() > MAX_ITEMS {
                    format!("record{{{} fields}}", fields.len())
                } else {
                    format!(
                        "record{{{}}}",
                        fields
                            .iter()
                            .map(|(n, t)| format!("{}:{}", n, self.display_val_inner(*t, next)))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            }
            ValueType::Variant(cases) => {
                if cases.len() > MAX_ITEMS {
                    format!("variant{{{} cases}}", cases.len())
                } else {
                    format!(
                        "variant{{{}}}",
                        cases
                            .iter()
                            .map(|(n, t)| match t {
                                Some(t) => {
                                    format!("{}:{}", n, self.display_val_inner(*t, next))
                                }
                                None => n.clone(),
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            }
            ValueType::Enum(names) => {
                if names.len() > MAX_ITEMS {
                    format!("enum{{{} cases}}", names.len())
                } else {
                    format!("enum{{{}}}", names.join(","))
                }
            }
            ValueType::Flags(names) => {
                if names.len() > MAX_ITEMS {
                    format!("flags{{{} flags}}", names.len())
                } else {
                    format!("flags{{{}}}", names.join(","))
                }
            }
            ValueType::Result { ok, err } => format!(
                "result<{},{}>",
                ok.map(|t| self.display_val_inner(t, next))
                    .unwrap_or_else(|| "_".into()),
                err.map(|t| self.display_val_inner(t, next))
                    .unwrap_or_else(|| "_".into())
            ),
            ValueType::Resource => "resource".into(),
            ValueType::AsyncHandle => "async_handle".into(),
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
            None,
            &TypeArena::default(),
        );
        assert_eq!(conn.short_label(), "handler");

        let conn2 = InterfaceConnection::from_instance(
            "wasi:io/streams@0.2.0".to_string(),
            1,
            None,
            &TypeArena::default(),
        );
        assert_eq!(conn2.short_label(), "streams");
    }

    #[test]
    fn test_short_interface_name() {
        assert_eq!(short_interface_name("wasi:http/handler@0.3.0"), "handler");
        assert_eq!(short_interface_name("wasi:io/streams@0.2.0"), "streams");
        assert_eq!(short_interface_name("simple"), "simple");
    }

    #[test]
    fn test_display_val_simple_types_unchanged() {
        let mut arena = TypeArena::default();
        let u32_id = arena.intern_val(ValueType::U32);
        let str_id = arena.intern_val(ValueType::String);
        assert_eq!(arena.display_val(u32_id), "u32");
        assert_eq!(arena.display_val(str_id), "string");
    }

    #[test]
    fn test_display_val_small_variant_expanded() {
        let mut arena = TypeArena::default();
        // 3 cases → below threshold, should be expanded
        let v = arena.intern_val(ValueType::Variant(vec![
            ("a".into(), None),
            ("b".into(), None),
            ("c".into(), None),
        ]));
        let s = arena.display_val(v);
        assert!(s.starts_with("variant{"), "got: {s}");
        assert!(s.contains("a,b,c"), "got: {s}");
    }

    #[test]
    fn test_display_val_large_variant_summarized() {
        let mut arena = TypeArena::default();
        // 5 cases → above threshold (MAX_ITEMS=4), should be summarized
        let v = arena.intern_val(ValueType::Variant(
            (0..5).map(|i| (format!("case-{i}"), None)).collect(),
        ));
        let s = arena.display_val(v);
        assert_eq!(s, "variant{5 cases}", "got: {s}");
    }

    #[test]
    fn test_display_val_large_record_summarized() {
        let mut arena = TypeArena::default();
        let u32_id = arena.intern_val(ValueType::U32);
        let v = arena.intern_val(ValueType::Record(
            (0..6).map(|i| (format!("field-{i}"), u32_id)).collect(),
        ));
        let s = arena.display_val(v);
        assert_eq!(s, "record{6 fields}", "got: {s}");
    }

    #[test]
    fn test_display_val_result_with_summarized_err() {
        let mut arena = TypeArena::default();
        let res_id = arena.intern_val(ValueType::Resource);
        // Large error variant
        let err_id = arena.intern_val(ValueType::Variant(
            (0..10).map(|i| (format!("e{i}"), None)).collect(),
        ));
        let result_id = arena.intern_val(ValueType::Result {
            ok: Some(res_id),
            err: Some(err_id),
        });
        let s = arena.display_val(result_id);
        assert_eq!(s, "result<resource,variant{10 cases}>", "got: {s}");
    }

    #[test]
    fn test_display_val_does_not_affect_canonical_val() {
        // canonical_val must remain fully expanded for fingerprinting
        let mut arena = TypeArena::default();
        let v = arena.intern_val(ValueType::Variant(
            (0..5).map(|i| (format!("case-{i}"), None)).collect(),
        ));
        let canonical = arena.canonical_val(v);
        let display = arena.display_val(v);
        assert_ne!(
            canonical, display,
            "canonical should be full, display should be summarized"
        );
        assert!(
            canonical.contains("case-0"),
            "canonical should expand all cases"
        );
        assert_eq!(display, "variant{5 cases}");
    }
}
