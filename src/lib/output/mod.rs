pub mod graph;
pub mod json;
pub mod mermaid;

use crate::model::{
    CompositionGraph, ExportInfo, FuncSignature, InterfaceType, InternedId, TypeArena,
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

    pub(crate) fn export_symbol(
        &mut self,
        graph: &CompositionGraph,
        interface_name: &str,
        show_types: bool,
    ) -> String {
        if !show_types {
            return String::new();
        }
        graph
            .component_exports
            .get(interface_name)
            .and_then(|info| self.symbol_for_export(info, &graph.arena))
            .map(str::to_string)
            .unwrap_or_default()
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

/// Output format for visualization
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Ascii,
    Mermaid,
    Json,
    #[value(name = "json-pretty")]
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
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum Direction {
    #[default]
    #[value(name = "lr", alias = "left-to-right")]
    LeftToRight,
    #[value(name = "td", alias = "top-down")]
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

/// CLI knob for ANSI color emission on ASCII output.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}
impl ColorMode {
    /// Resolve to a concrete on/off.  When `self` is [`ColorMode::Auto`],
    /// `auto_on` is returned — the caller decides what "auto" means.
    pub fn resolve(self, auto_on: bool) -> bool {
        match self {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => auto_on,
        }
    }

    /// Resolve for the CLI-default case: color when output is going to
    /// a TTY stdout (and stays off when piped or redirected to a file).
    pub fn resolve_for_stdout(self, to_file: bool) -> bool {
        use std::io::IsTerminal;
        self.resolve(!to_file && std::io::stdout().is_terminal())
    }
}

/// Terminal column count, or `None` when not on a TTY.
/// Convenience to specify max width of ascii rendering.
pub fn terminal_columns() -> Option<usize> {
    let (terminal_size::Width(w), _) = terminal_size::terminal_size()?;
    (w > 0).then_some(w as usize)
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
            is_async: false,
            param_names: vec![],
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
            is_async: false,
            param_names: vec![],
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
            is_async: false,
            param_names: vec![],
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
            is_async: false,
            param_names: vec![],
            params: vec![u32_id],
            results: vec![bool_id],
        };
        // Func variant: single bare sig line, no backtick-name prefix
        let iface = InterfaceType::Func(sig);
        let lines = format_interface_type_lines(&iface, &arena);
        assert_eq!(lines, vec!["(u32) -> bool"]);
    }
}
