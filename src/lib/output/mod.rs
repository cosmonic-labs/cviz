pub mod ascii;
pub mod json;
pub mod mermaid;

use crate::model::{
    short_interface_name, CompositionGraph, ExportInfo, FuncSignature, InterfaceConnection,
    InterfaceType, InternedId, TypeArena, SYNTHETIC_COMPONENT,
};

/// Format a function signature as `(param-type, ...) -> result-type`.
///
/// Uses [`TypeArena::display_val`] so that large complex types (variants with
/// many cases, records with many fields, etc.) are summarised rather than
/// expanded in full.  Fingerprinting is unaffected — it always uses the
/// lossless [`TypeArena::canonical_val`].
pub(crate) fn format_func_sig(sig: &FuncSignature, arena: &TypeArena) -> String {
    let params: Vec<String> = sig.params.iter().map(|id| arena.display_val(*id)).collect();
    let results: Vec<String> = sig
        .results
        .iter()
        .map(|id| arena.display_val(*id))
        .collect();
    let result_str = match results.as_slice() {
        [] => "()".to_string(),
        [single] => single.clone(),
        _ => format!("({})", results.join(", ")),
    };
    format!("({}) -> {}", params.join(", "), result_str)
}

/// Return type lines for an [`InterfaceConnection`], or an empty vec when
/// `show_types` is false or the connection carries no type information.
pub(crate) fn connection_type_lines(
    conn: &InterfaceConnection,
    arena: &TypeArena,
    show_types: bool,
) -> Vec<String> {
    if !show_types {
        return vec![];
    }
    conn.interface_type
        .as_ref()
        .map(|t| format_interface_type_lines(t, arena))
        .unwrap_or_default()
}

/// Return type lines for an [`ExportInfo`], or an empty vec when `show_types`
/// is false or the export carries no interface type.
pub(crate) fn export_type_lines(
    export_info: &ExportInfo,
    arena: &TypeArena,
    show_types: bool,
) -> Vec<String> {
    if !show_types {
        return vec![];
    }
    match export_info.ty {
        Some(InternedId::Interface(id)) => {
            format_interface_type_lines(arena.lookup_interface(id), arena)
        }
        _ => vec![],
    }
}

/// Return one display line per exported function in the interface.
///
/// - `Instance` interfaces produce `"fn-name: (params) -> result"` per function.
/// - `Func` interfaces produce a single `"(params) -> result"` line.
pub(crate) fn format_interface_type_lines(iface: &InterfaceType, arena: &TypeArena) -> Vec<String> {
    match iface {
        InterfaceType::Func(sig) => vec![format_func_sig(sig, arena)],
        InterfaceType::Instance(inst) => inst
            .functions
            .iter()
            .map(|(name, sig)| format!("`{}`: {}", name, format_func_sig(sig, arena)))
            .collect(),
    }
}

const SYMBOL_POOL: &[char] = &[
    '✦', '✧', '◆', '◇', '★', '☆', '●', '○', '▲', '△', '▼', '▽', '■', '□', '◉', '♦', '♠', '✱', '✴',
    '❖',
];

/// Compute the symbol string for a given assignment index using base-N encoding
/// over [`SYMBOL_POOL`].
///
/// - Indices `0..N` produce single-character identifiers (`"✦"`, `"✧"`, …).
/// - Indices `N..N+N²` produce two-character identifiers (`"✦✦"`, `"✦✧"`, …).
/// - Indices beyond that produce three-character identifiers, and so on.
///
/// This guarantees an unbounded, collision-free sequence of compact identifiers
/// without ever reusing a symbol string.
fn symbol_at(index: usize) -> String {
    let n = SYMBOL_POOL.len();
    // Find which "length tier" this index falls into and the offset within it.
    // Tier 1 covers [0, n), tier 2 covers [n, n + n²), tier 3 covers [n + n², n + n² + n³), …
    let mut tier_size = n;
    let mut offset = index;
    let mut len = 1;
    while offset >= tier_size {
        offset -= tier_size;
        tier_size *= n;
        len += 1;
    }
    // Decode `offset` as a base-N number of `len` digits (most-significant first).
    let mut digits = vec![0usize; len];
    let mut remainder = offset;
    for d in digits.iter_mut().rev() {
        *d = remainder % n;
        remainder /= n;
    }
    digits.iter().map(|&d| SYMBOL_POOL[d]).collect()
}

/// Assigns a unique identifier to each distinct interface type encountered
/// during rendering and collects a display key.
///
/// Types are distinguished by fingerprint, so structurally identical interfaces
/// always receive the same symbol within one diagram.  Identifiers are drawn
/// from [`SYMBOL_POOL`] via [`symbol_at`]: single glyphs first, then
/// two-glyph combinations, then three-glyph, and so on — so the pool never
/// truly exhausts.
pub(crate) struct SymbolMap {
    /// `(fingerprint, symbol string, formatted type lines)`
    entries: Vec<(String, String, Vec<String>)>,
}

impl SymbolMap {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Return (or assign) the symbol for a connection's interface type.
    /// Returns `None` if the connection carries no type info.
    pub(crate) fn symbol_for_conn(
        &mut self,
        conn: &InterfaceConnection,
        arena: &TypeArena,
    ) -> Option<&str> {
        let fp = conn.fingerprint.as_ref()?;
        let iface = conn.interface_type.as_ref()?;
        Some(self.get_or_insert(fp, iface, arena))
    }

    /// Return (or assign) the symbol for an export's interface type.
    /// Returns `None` if the export carries no interface type.
    pub(crate) fn symbol_for_export(
        &mut self,
        export_info: &ExportInfo,
        arena: &TypeArena,
    ) -> Option<&str> {
        let fp = export_info.fingerprint.as_ref()?;
        let id = match export_info.ty {
            Some(InternedId::Interface(id)) => id,
            _ => return None,
        };
        Some(self.get_or_insert(fp, arena.lookup_interface(id), arena))
    }

    fn get_or_insert(&mut self, fp: &str, iface: &InterfaceType, arena: &TypeArena) -> &str {
        if let Some(pos) = self.entries.iter().position(|(f, _, _)| f == fp) {
            return &self.entries[pos].1;
        }
        let symbol = symbol_at(self.entries.len());
        let lines = format_interface_type_lines(iface, arena);
        self.entries.push((fp.to_string(), symbol, lines));
        &self.entries.last().unwrap().1
    }

    /// Return (or assign) the symbol for a pre-computed fingerprint + type lines,
    /// or an empty string when `show_types` is false or no fingerprint is present.
    ///
    /// This is the primary entry point for AllInterfaces/Full renderers that
    /// receive type data from the [`DiagramEdge`]/[`DiagramExport`] IR.
    pub(crate) fn assign(
        &mut self,
        show_types: bool,
        fingerprint: Option<&str>,
        type_lines: Vec<String>,
    ) -> String {
        if !show_types {
            return String::new();
        }
        let Some(fp) = fingerprint else {
            return String::new();
        };
        if let Some(pos) = self.entries.iter().position(|(f, _, _)| f == fp) {
            return self.entries[pos].1.clone();
        }
        let symbol = symbol_at(self.entries.len());
        self.entries.push((fp.to_string(), symbol, type_lines));
        self.entries.last().unwrap().1.clone()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return key lines: `"✦ fn-name: sig"` per unique symbol, with
    /// continuation lines for multi-function interfaces indented.
    pub(crate) fn key_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (_, symbol, type_lines) in &self.entries {
            for (i, line) in type_lines.iter().enumerate() {
                if i == 0 {
                    out.push(format!("{} {}", symbol, line));
                } else {
                    out.push(format!("  {}", line));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Intermediate representation — shared graph traversal
// ---------------------------------------------------------------------------

/// A node to be rendered in the diagram.
pub(crate) struct DiagramNode {
    pub name: String,
    pub display: String,
    pub is_synthetic: bool,
    pub component_index: u32,
}

/// A directed edge between two nodes.
pub(crate) struct DiagramEdge {
    /// Raw name of the source (interface_name for host imports, node.name otherwise).
    /// Renderers that need sanitized IDs (Mermaid) apply their own transform.
    pub from_name: String,
    pub from_display: String,
    pub to_name: String,
    pub to_display: String,
    /// Ready-to-use edge label (short interface name or full name, depending on mode).
    pub label: String,
    /// Pre-formatted type lines for this connection (empty when show_types=false).
    pub type_lines: Vec<String>,
    /// Fingerprint for deduplication in a [`SymbolMap`] (None when no type info).
    pub fingerprint: Option<String>,
    /// true if host import
    pub is_dashed: bool,
}

/// An exported interface.
pub(crate) struct DiagramExport {
    pub from_name: String,
    pub from_display: String,
    pub full_name: String,
    pub short_name: String,
    /// Pre-formatted type lines for this export (empty when show_types=false).
    pub type_lines: Vec<String>,
    /// Fingerprint for deduplication in a [`SymbolMap`] (None when no type info).
    pub fingerprint: Option<String>,
}

/// Pre-computed graph data for rendering, independent of output format.
pub(crate) struct ConnectionsView {
    /// Raw host interface names (AllInterfaces only; empty for Full).
    pub host_names: Vec<String>,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    pub exports: Vec<DiagramExport>,
}

/// Build a [`ConnectionsView`] for `AllInterfaces` detail level.
///
/// Includes real (non-synthetic) component nodes, host-import edges (dashed),
/// inter-component edges (solid), and exported interfaces.  Edge labels use
/// the short interface name.
pub(crate) fn build_all_interfaces_view(
    graph: &CompositionGraph,
    show_types: bool,
) -> ConnectionsView {
    let component_nodes = graph.real_nodes();

    let nodes = component_nodes
        .iter()
        .map(|n| DiagramNode {
            name: n.name.clone(),
            display: n.display_label().to_string(),
            is_synthetic: false,
            component_index: n.component_index,
        })
        .collect();

    let mut edges = Vec::new();
    for node in &component_nodes {
        for import in &node.imports {
            if import.is_host_import {
                edges.push(DiagramEdge {
                    from_name: import.interface_name.clone(),
                    from_display: short_interface_name(&import.interface_name),
                    to_name: node.name.clone(),
                    to_display: node.display_label().to_string(),
                    label: import.short_label(),
                    type_lines: connection_type_lines(import, &graph.arena, show_types),
                    fingerprint: import.fingerprint.clone(),
                    is_dashed: true,
                });
            } else if let Some(src) = import.source_instance.and_then(|id| graph.get_node(id)) {
                if src.component_index != SYNTHETIC_COMPONENT {
                    edges.push(DiagramEdge {
                        from_name: src.name.clone(),
                        from_display: src.display_label().to_string(),
                        to_name: node.name.clone(),
                        to_display: node.display_label().to_string(),
                        label: import.short_label(),
                        type_lines: connection_type_lines(import, &graph.arena, show_types),
                        fingerprint: import.fingerprint.clone(),
                        is_dashed: false,
                    });
                }
            }
        }
    }

    let mut exports = Vec::new();
    for (export_name, export_info) in &graph.component_exports {
        if let Some(node) = graph.get_node(export_info.source_instance) {
            if node.component_index != SYNTHETIC_COMPONENT {
                exports.push(DiagramExport {
                    from_name: node.name.clone(),
                    from_display: node.display_label().to_string(),
                    full_name: export_name.clone(),
                    short_name: short_interface_name(export_name),
                    type_lines: export_type_lines(export_info, &graph.arena, show_types),
                    fingerprint: export_info.fingerprint.clone(),
                });
            }
        }
    }

    ConnectionsView {
        host_names: graph.host_interfaces(),
        nodes,
        edges,
        exports,
    }
}

/// Build a [`ConnectionsView`] for `Full` detail level.
///
/// Includes all nodes (including synthetic), all non-host-import edges with
/// full interface names, and all exported interfaces.
pub(crate) fn build_full_view(graph: &CompositionGraph, show_types: bool) -> ConnectionsView {
    let nodes = graph
        .nodes
        .values()
        .map(|n| DiagramNode {
            name: n.name.clone(),
            display: n.display_label().to_string(),
            is_synthetic: n.component_index == SYNTHETIC_COMPONENT,
            component_index: n.component_index,
        })
        .collect();

    let mut edges = Vec::new();
    for node in graph.nodes.values() {
        for import in &node.imports {
            if !import.is_host_import {
                if let Some(src) = import.source_instance.and_then(|id| graph.get_node(id)) {
                    edges.push(DiagramEdge {
                        from_name: src.name.clone(),
                        from_display: src.display_label().to_string(),
                        to_name: node.name.clone(),
                        to_display: node.display_label().to_string(),
                        label: import.interface_name.clone(),
                        type_lines: connection_type_lines(import, &graph.arena, show_types),
                        fingerprint: import.fingerprint.clone(),
                        is_dashed: false,
                    });
                }
            }
        }
    }

    let mut exports = Vec::new();
    for (export_name, export_info) in &graph.component_exports {
        if let Some(node) = graph.get_node(export_info.source_instance) {
            exports.push(DiagramExport {
                from_name: node.name.clone(),
                from_display: node.display_label().to_string(),
                full_name: export_name.clone(),
                short_name: short_interface_name(export_name),
                type_lines: export_type_lines(export_info, &graph.arena, show_types),
                fingerprint: export_info.fingerprint.clone(),
            });
        }
    }

    ConnectionsView {
        host_names: vec![],
        nodes,
        edges,
        exports,
    }
}

/// Output format for visualization
#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    #[default]
    Ascii,
    Mermaid,
    Json,
    JsonPretty,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ascii" => Ok(OutputFormat::Ascii),
            "mermaid" => Ok(OutputFormat::Mermaid),
            "json" => Ok(OutputFormat::Json),
            "json-pretty" => Ok(OutputFormat::JsonPretty),
            _ => Err(format!(
                "Invalid output format: {}. Valid values: ascii, mermaid, json, json-pretty",
                s
            )),
        }
    }
}

/// Diagram direction (applies to Mermaid only)
#[derive(Debug, Clone, Copy, Default)]
pub enum Direction {
    #[default]
    LeftToRight,
    TopDown,
}

impl Direction {
    pub fn to_mermaid(self) -> &'static str {
        match self {
            Direction::LeftToRight => "LR",
            Direction::TopDown => "TD",
        }
    }
}

impl std::str::FromStr for Direction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "lr" | "left-to-right" => Ok(Direction::LeftToRight),
            "td" | "top-down" => Ok(Direction::TopDown),
            _ => Err(format!("Invalid direction: {}", s)),
        }
    }
}

/// Detail level for the diagram
#[derive(Debug, Clone, Copy, Default)]
pub enum DetailLevel {
    /// Only show the HTTP handler chain
    #[default]
    HandlerChain,
    /// Show all interfaces
    AllInterfaces,
    /// Show everything including internal details
    Full,
}

impl std::str::FromStr for DetailLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "handler-chain" | "handler" => Ok(DetailLevel::HandlerChain),
            "all-interfaces" | "all" => Ok(DetailLevel::AllInterfaces),
            "full" => Ok(DetailLevel::Full),
            _ => Err(format!("Invalid detail level: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_parse() {
        assert!(matches!(
            "ascii".parse::<OutputFormat>().unwrap(),
            OutputFormat::Ascii
        ));
        assert!(matches!(
            "mermaid".parse::<OutputFormat>().unwrap(),
            OutputFormat::Mermaid
        ));
        assert!(matches!(
            "json".parse::<OutputFormat>().unwrap(),
            OutputFormat::Json
        ));
        assert!("invalid".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_direction_parse() {
        assert!(matches!(
            "lr".parse::<Direction>().unwrap(),
            Direction::LeftToRight
        ));
        assert!(matches!(
            "td".parse::<Direction>().unwrap(),
            Direction::TopDown
        ));
    }

    #[test]
    fn test_detail_level_parse() {
        assert!(matches!(
            "handler-chain".parse::<DetailLevel>().unwrap(),
            DetailLevel::HandlerChain
        ));
        assert!(matches!(
            "all-interfaces".parse::<DetailLevel>().unwrap(),
            DetailLevel::AllInterfaces
        ));
        assert!(matches!(
            "full".parse::<DetailLevel>().unwrap(),
            DetailLevel::Full
        ));
    }

    #[test]
    fn test_symbol_at_tier_boundaries() {
        let n = SYMBOL_POOL.len();

        // Last single-char symbol
        assert_eq!(symbol_at(n - 1).chars().count(), 1);
        // First two-char symbol
        assert_eq!(symbol_at(n).chars().count(), 2);
        // Last two-char symbol
        assert_eq!(symbol_at(n + n * n - 1).chars().count(), 2);
        // First three-char symbol
        assert_eq!(symbol_at(n + n * n).chars().count(), 3);
    }

    #[test]
    fn test_symbol_at_no_duplicates() {
        let n = SYMBOL_POOL.len();
        // Verify the first n + n² symbols are all distinct
        let symbols: Vec<String> = (0..n + n * n).map(symbol_at).collect();
        let unique: std::collections::HashSet<&String> = symbols.iter().collect();
        assert_eq!(symbols.len(), unique.len(), "symbol_at produced duplicates");
    }

    // -----------------------------------------------------------------------
    // format_func_sig edge cases
    // -----------------------------------------------------------------------

    use crate::model::{FuncSignature, InterfaceType, ValueType};

    fn make_arena() -> crate::model::TypeArena {
        crate::model::TypeArena::default()
    }

    #[test]
    fn test_format_func_sig_no_params() {
        let mut arena = make_arena();
        let bool_id = arena.intern_val(ValueType::Bool);
        let sig = FuncSignature {
            params: vec![],
            results: vec![bool_id],
        };
        assert_eq!(format_func_sig(&sig, &arena), "() -> bool");
    }

    #[test]
    fn test_format_func_sig_no_results() {
        let mut arena = make_arena();
        let u32_id = arena.intern_val(ValueType::U32);
        let sig = FuncSignature {
            params: vec![u32_id],
            results: vec![],
        };
        assert_eq!(format_func_sig(&sig, &arena), "(u32) -> ()");
    }

    #[test]
    fn test_format_func_sig_multiple_results() {
        let mut arena = make_arena();
        let u32_id = arena.intern_val(ValueType::U32);
        let str_id = arena.intern_val(ValueType::String);
        let bool_id = arena.intern_val(ValueType::Bool);
        let sig = FuncSignature {
            params: vec![u32_id, str_id],
            results: vec![bool_id, str_id],
        };
        assert_eq!(
            format_func_sig(&sig, &arena),
            "(u32, string) -> (bool, string)"
        );
    }

    #[test]
    fn test_format_interface_type_lines_func_variant() {
        let mut arena = make_arena();
        let u32_id = arena.intern_val(ValueType::U32);
        let bool_id = arena.intern_val(ValueType::Bool);
        let sig = FuncSignature {
            params: vec![u32_id],
            results: vec![bool_id],
        };
        // Func variant: single bare sig line, no backtick-name prefix
        let iface = InterfaceType::Func(sig);
        let lines = format_interface_type_lines(&iface, &arena);
        assert_eq!(lines, vec!["(u32) -> bool"]);
    }

    #[test]
    fn test_connection_type_lines_missing_type_info() {
        use crate::model::InterfaceConnection;
        let arena = make_arena();
        let conn = InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: None,
            is_host_import: true,
            interface_type: None, // no type info
            fingerprint: None,
        };
        // show_types=true but no type info → should return empty, not panic
        let lines = connection_type_lines(&conn, &arena, true);
        assert!(
            lines.is_empty(),
            "missing type info should produce no type lines"
        );
    }

    // -----------------------------------------------------------------------
    // ConnectionsView IR tests
    // -----------------------------------------------------------------------

    use crate::test_utils::*;

    #[test]
    fn test_view_all_interfaces_node_count() {
        let graph = simple_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        // $srv and $middleware are real; no synthetic nodes in this graph
        assert_eq!(view.nodes.len(), 2);
        assert!(view.nodes.iter().any(|n| n.display.contains("srv")));
        assert!(view.nodes.iter().any(|n| n.display.contains("middleware")));
    }

    #[test]
    fn test_view_all_interfaces_host_names() {
        let graph = simple_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        // Two distinct host interfaces: handler (from srv) and log (from middleware)
        assert_eq!(view.host_names.len(), 2);
        assert!(view.host_names.iter().any(|n| n.contains("handler")));
        assert!(view.host_names.iter().any(|n| n.contains("log")));
    }

    #[test]
    fn test_view_all_interfaces_edge_dashed() {
        let graph = simple_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        // 2 host-import edges (dashed) + 1 component edge (solid)
        let dashed: Vec<_> = view.edges.iter().filter(|e| e.is_dashed).collect();
        let solid: Vec<_> = view.edges.iter().filter(|e| !e.is_dashed).collect();
        assert_eq!(
            dashed.len(),
            2,
            "two host imports should produce dashed edges"
        );
        assert_eq!(
            solid.len(),
            1,
            "one inter-component import should produce a solid edge"
        );
    }

    #[test]
    fn test_view_all_interfaces_edge_endpoints() {
        let graph = simple_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        let solid = view.edges.iter().find(|e| !e.is_dashed).unwrap();
        assert!(
            solid.from_display.contains("srv"),
            "solid edge should come from srv"
        );
        assert!(
            solid.to_display.contains("middleware"),
            "solid edge should go to middleware"
        );
        assert_eq!(
            solid.label, "handler",
            "edge label should be short interface name"
        );
    }

    #[test]
    fn test_view_all_interfaces_export() {
        let graph = simple_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        assert_eq!(view.exports.len(), 1);
        let exp = &view.exports[0];
        assert!(exp.from_display.contains("middleware"));
        assert_eq!(exp.short_name, "handler");
        assert!(exp.full_name.contains("wasi:http/handler"));
    }

    #[test]
    fn test_view_all_interfaces_non_http_chain() {
        // Verify the IR works for a non-http chain (keyvalue/store)
        let graph = two_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        let kv_export = view
            .exports
            .iter()
            .find(|e| e.full_name.contains("keyvalue"));
        assert!(kv_export.is_some(), "should have a keyvalue export");
        assert_eq!(kv_export.unwrap().short_name, "store");

        let kv_solid = view
            .edges
            .iter()
            .find(|e| !e.is_dashed && e.label == "store");
        assert!(kv_solid.is_some(), "should have solid keyvalue/store edge");
        let kv_solid = kv_solid.unwrap();
        assert!(kv_solid.from_display.contains("db"));
        assert!(kv_solid.to_display.contains("cache"));
    }

    #[test]
    fn test_view_all_interfaces_excludes_synthetic_source() {
        // A synthetic node as the *source* of an import should not produce an
        // edge in AllInterfaces mode (only real component sources are shown).
        use crate::model::{ComponentNode, InterfaceConnection, SYNTHETIC_COMPONENT};

        let mut graph = CompositionGraph::new();

        // A real component that imports from a synthetic source
        let mut real = ComponentNode::new("$real".to_string(), 0, 0);
        real.add_import(InterfaceConnection {
            interface_name: "wasi:http/handler@0.3.0".to_string(),
            source_instance: Some(99), // will be a synthetic node
            is_host_import: false,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, real);

        // The synthetic node at idx 99
        let synthetic = ComponentNode::new(
            "$synthetic".to_string(),
            SYNTHETIC_COMPONENT,
            SYNTHETIC_COMPONENT,
        );
        graph.add_node(99, synthetic);

        let view = build_all_interfaces_view(&graph, false);

        // The edge from the synthetic source should be dropped
        assert!(
            view.edges.is_empty(),
            "edges from synthetic source nodes should be excluded, got: {:?}",
            view.edges
                .iter()
                .map(|e| (&e.from_display, &e.to_display))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_view_full_includes_all_nodes() {
        let graph = simple_chain_graph();
        let view = build_full_view(&graph, false);
        // Full includes all nodes; host_names is empty
        assert!(view.nodes.len() >= 2);
        assert!(
            view.host_names.is_empty(),
            "Full mode has no host node list"
        );
    }

    #[test]
    fn test_view_full_no_dashed_edges() {
        let graph = simple_chain_graph();
        let view = build_full_view(&graph, false);
        assert!(
            view.edges.iter().all(|e| !e.is_dashed),
            "Full mode skips host imports so no edge should be dashed"
        );
    }

    #[test]
    fn test_view_full_edge_uses_full_name() {
        let graph = simple_chain_graph();
        let view = build_full_view(&graph, false);
        let edge = view
            .edges
            .iter()
            .find(|e| e.label.contains("handler"))
            .unwrap();
        assert!(
            edge.label.contains("wasi:http/handler@0.3.0"),
            "Full mode should use full interface name as label, got: {}",
            edge.label
        );
    }

    #[test]
    fn test_view_all_interfaces_two_chains() {
        let graph = two_chain_graph();
        let view = build_all_interfaces_view(&graph, false);
        // 4 real nodes: srv-http, mw-http, db, cache
        assert_eq!(view.nodes.len(), 4);
        // 2 solid edges (one per chain) + 2 host-import edges (one per inner node)
        let solid: Vec<_> = view.edges.iter().filter(|e| !e.is_dashed).collect();
        assert_eq!(solid.len(), 2, "two inter-component edges expected");
        // 2 exports
        assert_eq!(view.exports.len(), 2);
        let names: Vec<&str> = view.exports.iter().map(|e| e.short_name.as_str()).collect();
        assert!(names.contains(&"handler"), "should have handler export");
        assert!(names.contains(&"store"), "should have store export");
    }

    #[test]
    fn test_view_full_synthetic_node_included() {
        use crate::model::{ComponentNode, SYNTHETIC_COMPONENT};
        let mut graph = CompositionGraph::new();

        let real = ComponentNode::new("$real".to_string(), 0, 0);
        graph.add_node(1, real);
        let synthetic = ComponentNode::new(
            "$synth".to_string(),
            SYNTHETIC_COMPONENT,
            SYNTHETIC_COMPONENT,
        );
        graph.add_node(99, synthetic);

        let view = build_full_view(&graph, false);
        assert_eq!(
            view.nodes.len(),
            2,
            "Full mode should include synthetic nodes"
        );
        assert!(
            view.nodes.iter().any(|n| n.is_synthetic),
            "synthetic flag should be set"
        );
        assert!(view.nodes.iter().any(|n| n.display.contains("synth")));
    }

    #[test]
    fn test_view_host_interfaces_deduplicated() {
        use crate::model::{ComponentNode, InterfaceConnection};
        let mut graph = CompositionGraph::new();

        // Two real nodes both importing the same host interface
        let mut a = ComponentNode::new("$a".to_string(), 0, 0);
        a.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".to_string(),
            source_instance: None,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(1, a);

        let mut b = ComponentNode::new("$b".to_string(), 1, 1);
        b.add_import(InterfaceConnection {
            interface_name: "wasi:logging/log@0.1.0".to_string(),
            source_instance: None,
            is_host_import: true,
            interface_type: None,
            fingerprint: None,
        });
        graph.add_node(2, b);

        let view = build_all_interfaces_view(&graph, false);
        assert_eq!(
            view.host_names.len(),
            1,
            "same host interface imported by two nodes should appear once"
        );
        assert_eq!(view.host_names[0], "wasi:logging/log@0.1.0");
    }
}
