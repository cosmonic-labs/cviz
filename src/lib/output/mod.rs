pub mod ascii;
// pub mod json;
pub mod mermaid;

use crate::model::{short_interface_name, CompositionGraph, ExportInfo, FuncSignature, InterfaceConnection, InterfaceType, InternedId, TypeArena, SYNTHETIC_COMPONENT};

/// Format a function signature as `(param-type, ...) -> result-type`.
pub(crate) fn format_func_sig(sig: &FuncSignature, arena: &TypeArena) -> String {
    let params: Vec<String> = sig.params.iter().map(|id| arena.canonical_val(*id)).collect();
    let results: Vec<String> = sig.results.iter().map(|id| arena.canonical_val(*id)).collect();
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
        Some(InternedId::Interface(id)) => format_interface_type_lines(arena.lookup_interface(id), arena),
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
    '✦', '✧', '◆', '◇', '★', '☆', '●', '○', '▲', '△',
    '▼', '▽', '■', '□', '◉', '♦', '♠', '✱', '✴', '❖',
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
        Self { entries: Vec::new() }
    }

    /// Return (or assign) the symbol for a connection's interface type.
    /// Returns `None` if the connection carries no type info.
    pub(crate) fn symbol_for_conn(&mut self, conn: &InterfaceConnection, arena: &TypeArena) -> Option<&str> {
        let fp = conn.fingerprint.as_ref()?;
        let iface = conn.interface_type.as_ref()?;
        Some(self.get_or_insert(fp, iface, arena))
    }

    /// Return (or assign) the symbol for an export's interface type.
    /// Returns `None` if the export carries no interface type.
    pub(crate) fn symbol_for_export(&mut self, export_info: &ExportInfo, arena: &TypeArena) -> Option<&str> {
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
    pub type_lines: Vec<String>,
    /// true if host import
    pub is_dashed: bool,
}

/// An exported interface.
pub(crate) struct DiagramExport {
    pub from_name: String,
    pub from_display: String,
    pub full_name: String,
    pub short_name: String,
    pub type_lines: Vec<String>,
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
pub(crate) fn build_all_interfaces_view(graph: &CompositionGraph, show_types: bool) -> ConnectionsView {
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
                    is_dashed: true,
                });
            } else if let Some(src) = graph.get_node(import.source_instance) {
                if src.component_index != SYNTHETIC_COMPONENT {
                    edges.push(DiagramEdge {
                        from_name: src.name.clone(),
                        from_display: src.display_label().to_string(),
                        to_name: node.name.clone(),
                        to_display: node.display_label().to_string(),
                        label: import.short_label(),
                        type_lines: connection_type_lines(import, &graph.arena, show_types),
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
                });
            }
        }
    }

    ConnectionsView { host_names: graph.host_interfaces(), nodes, edges, exports }
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
                if let Some(src) = graph.get_node(import.source_instance) {
                    edges.push(DiagramEdge {
                        from_name: src.name.clone(),
                        from_display: src.display_label().to_string(),
                        to_name: node.name.clone(),
                        to_display: node.display_label().to_string(),
                        label: import.interface_name.clone(),
                        type_lines: connection_type_lines(import, &graph.arena, show_types),
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
            });
        }
    }

    ConnectionsView { host_names: vec![], nodes, edges, exports }
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
}
