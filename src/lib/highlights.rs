//! Caller-supplied emphasis ("highlight these nodes/edges, and here's why")
//! that the graph renderers consume.
//!
//! Identity is by **canonical ID strings** (see [`canonical_id`]).  A caller
//! such as splicer computes which nodes and edges its YAML rules would touch,
//! stuffs the canonical IDs into a [`Highlights`], and hands it to cviz —
//! cviz then surfaces the emphasis in whichever output format it's asked to
//! produce.
//!
//! # Mark vs highlight
//!
//! - [`mark_node`](Highlights::mark_node) /
//!   [`mark_edge`](Highlights::mark_edge): "this thing is highlighted, no
//!   reason attached."  The renderer draws emphasis but no numeric label.
//! - [`highlight_node`](Highlights::highlight_node) /
//!   [`highlight_edge`](Highlights::highlight_edge): "this thing is
//!   highlighted, here is one reason."  Calling multiple times accumulates
//!   reasons; each reason becomes a numbered tag entry.
//!
//! # Colors
//!
//! Every selection has a [`HighlightColor`].  The default is
//! [`HighlightColor::Yellow`] (colorblind-safe baseline).  Callers that want
//! to colorize different selections distinctly use the `_with` builders,
//! e.g. [`mark_node_with`](Highlights::mark_node_with).  When the renderer
//! is asked to emit ANSI, the colored selection lights up in its assigned
//! color; when ANSI is off, color falls back to the same heavy box-drawing
//! used for the default color, so the visual signal survives.
//!
//! # Context IDs (tag numbering)
//!
//! Context strings are numbered 1-based in **insertion order across the
//! whole map** — first context seen anywhere (whether attached to a node or
//! an edge) gets `1`, the next *new* string gets `2`, and so on.  Duplicate
//! strings collapse to the same ID, so a caller that inserts contexts in
//! YAML-rule order can expect the tag list to mirror that order without
//! special bookkeeping.
//!
//! [`canonical_id`]: crate::canonical_id

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Color applied to a highlighted node or edge.
///
/// Used by ANSI-aware renderers to colorize the emphasis.  Picked from a
/// colorblind-safe palette — there is no pure red/green pair.  The default
/// is [`HighlightColor::Yellow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HighlightColor {
    #[default]
    Yellow,
    Cyan,
    Magenta,
    Blue,
    Orange,
    White,
}

impl HighlightColor {
    /// ANSI SGR sequence that opens bold + this color.  Pair with
    /// [`Self::ANSI_RESET`].
    pub fn ansi_open(self) -> &'static str {
        match self {
            HighlightColor::Yellow => "\x1b[1;33m",
            HighlightColor::Cyan => "\x1b[1;36m",
            HighlightColor::Magenta => "\x1b[1;35m",
            HighlightColor::Blue => "\x1b[1;34m",
            // 38;5;208 is a 256-color orange; broadly supported on modern
            // terminals, no red/green confusion.
            HighlightColor::Orange => "\x1b[1;38;5;208m",
            HighlightColor::White => "\x1b[1;97m",
        }
    }

    /// ANSI SGR reset.  Closes any of the [`Self::ansi_open`] sequences.
    pub const ANSI_RESET: &'static str = "\x1b[0m";

    /// Mermaid `stroke:` hex value for this color.  Used by the Mermaid
    /// renderer when it writes a `classDef` for a highlighted selection.
    pub fn mermaid_hex(self) -> &'static str {
        match self {
            HighlightColor::Yellow => "#d4a017",
            HighlightColor::Cyan => "#1ca3a3",
            HighlightColor::Magenta => "#a3338f",
            HighlightColor::Blue => "#2c5fb3",
            HighlightColor::Orange => "#d97706",
            HighlightColor::White => "#cccccc",
        }
    }

    /// Short kebab-case name, used as a stable key for Mermaid classDef
    /// identifiers (`hl_yellow`, `hl_orange`, …).
    pub fn slug(self) -> &'static str {
        match self {
            HighlightColor::Yellow => "yellow",
            HighlightColor::Cyan => "cyan",
            HighlightColor::Magenta => "magenta",
            HighlightColor::Blue => "blue",
            HighlightColor::Orange => "orange",
            HighlightColor::White => "white",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Selection {
    contexts: Vec<String>,
    color: HighlightColor,
}

/// A set of highlighted node and edge IDs, plus reasons (contexts) and an
/// optional per-selection color.
///
/// See the [module docs](self) for the mark-vs-highlight distinction, the
/// rules for tag numbering, and the color contract.
#[derive(Debug, Clone, Default)]
pub struct Highlights {
    nodes: BTreeMap<String, Selection>,
    edges: BTreeMap<String, Selection>,
    /// Insertion order of unique context strings.  Position + 1 is the
    /// tag ID.
    context_order: Vec<String>,
}

impl Highlights {
    /// New empty [`Highlights`].  Equivalent to [`Highlights::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Highlight `id` with a reason (`ctx`), default color.
    ///
    /// Multiple calls accumulate reasons; duplicate reason strings collapse
    /// to a single tag ID.
    pub fn highlight_node(&mut self, id: impl Into<String>, ctx: impl Into<String>) {
        self.highlight_node_with(id, ctx, HighlightColor::default());
    }

    /// Like [`highlight_node`](Self::highlight_node) but with an explicit
    /// color override.  When this id is highlighted multiple times the most
    /// recent color wins, so callers that care about consistency should
    /// pass the same color each time.
    pub fn highlight_node_with(
        &mut self,
        id: impl Into<String>,
        ctx: impl Into<String>,
        color: HighlightColor,
    ) {
        let ctx = ctx.into();
        self.record_context(&ctx);
        let entry = self.nodes.entry(id.into()).or_default();
        entry.contexts.push(ctx);
        entry.color = color;
    }

    /// Highlight `id` with a reason (`ctx`), default color.
    pub fn highlight_edge(&mut self, id: impl Into<String>, ctx: impl Into<String>) {
        self.highlight_edge_with(id, ctx, HighlightColor::default());
    }

    /// Like [`highlight_edge`](Self::highlight_edge) but with an explicit
    /// color override.
    pub fn highlight_edge_with(
        &mut self,
        id: impl Into<String>,
        ctx: impl Into<String>,
        color: HighlightColor,
    ) {
        let ctx = ctx.into();
        self.record_context(&ctx);
        let entry = self.edges.entry(id.into()).or_default();
        entry.contexts.push(ctx);
        entry.color = color;
    }

    /// Highlight `id` with no attached reason, default color.
    pub fn mark_node(&mut self, id: impl Into<String>) {
        self.mark_node_with(id, HighlightColor::default());
    }

    /// Like [`mark_node`](Self::mark_node) but with an explicit color
    /// override.  Most recent color wins on repeated calls.
    pub fn mark_node_with(&mut self, id: impl Into<String>, color: HighlightColor) {
        let entry = self.nodes.entry(id.into()).or_default();
        entry.color = color;
    }

    /// Highlight `id` with no attached reason, default color.
    pub fn mark_edge(&mut self, id: impl Into<String>) {
        self.mark_edge_with(id, HighlightColor::default());
    }

    /// Like [`mark_edge`](Self::mark_edge) but with an explicit color
    /// override.
    pub fn mark_edge_with(&mut self, id: impl Into<String>, color: HighlightColor) {
        let entry = self.edges.entry(id.into()).or_default();
        entry.color = color;
    }

    /// True when no node or edge has been added.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// True iff `id` was added (via any of the `mark_node` /
    /// `highlight_node` variants).
    pub fn is_node_highlighted(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// True iff `id` was added (via any of the `mark_edge` /
    /// `highlight_edge` variants).
    pub fn is_edge_highlighted(&self, id: &str) -> bool {
        self.edges.contains_key(id)
    }

    /// Color assigned to node `id`, or `None` if not highlighted.
    pub fn node_color(&self, id: &str) -> Option<HighlightColor> {
        self.nodes.get(id).map(|sel| sel.color)
    }

    /// Color assigned to edge `id`, or `None` if not highlighted.
    pub fn edge_color(&self, id: &str) -> Option<HighlightColor> {
        self.edges.get(id).map(|sel| sel.color)
    }

    /// Tag IDs (1-based) for the contexts attached to node `id`, in the
    /// order they were inserted on that node, with duplicates removed.
    /// Empty if the node was only marked (no contexts) or not highlighted
    /// at all.
    pub fn node_context_ids(&self, id: &str) -> Vec<usize> {
        let Some(sel) = self.nodes.get(id) else {
            return Vec::new();
        };
        self.context_ids(&sel.contexts)
    }

    /// Tag IDs for edge `id`.  See
    /// [`node_context_ids`](Self::node_context_ids).
    pub fn edge_context_ids(&self, id: &str) -> Vec<usize> {
        let Some(sel) = self.edges.get(id) else {
            return Vec::new();
        };
        self.context_ids(&sel.contexts)
    }

    /// Tag lines, one per unique context string, in insertion order.
    /// Each line is formatted `"N <context>"` (1-based).
    pub fn tag_lines(&self) -> Vec<String> {
        self.context_order
            .iter()
            .enumerate()
            .map(|(i, ctx)| format!("{} {}", i + 1, ctx))
            .collect()
    }

    /// Like [`Self::tag_lines`], but filtered to contexts that are
    /// attached to at least one *matched* node or edge ID.
    ///
    /// `present_nodes` and `present_edges` are the canonical IDs the
    /// caller knows exist in the rendered graph.  Tags whose only
    /// attachments are unmatched (typo'd) IDs are omitted — this avoids
    /// misleading entries in the rendered Tags list when a `--highlight`
    /// id failed to bind to anything.  Numeric IDs stay stable so the
    /// in-diagram `[1,3]` brackets still line up.
    pub fn tag_lines_referenced_by<I, J>(&self, present_nodes: I, present_edges: J) -> Vec<String>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
        J: IntoIterator,
        J::Item: AsRef<str>,
    {
        let node_set: BTreeSet<String> = present_nodes
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let edge_set: BTreeSet<String> = present_edges
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let mut live: BTreeSet<&str> = BTreeSet::new();
        for (id, sel) in &self.nodes {
            if node_set.contains(id) {
                for c in &sel.contexts {
                    live.insert(c.as_str());
                }
            }
        }
        for (id, sel) in &self.edges {
            if edge_set.contains(id) {
                for c in &sel.contexts {
                    live.insert(c.as_str());
                }
            }
        }
        self.context_order
            .iter()
            .enumerate()
            .filter(|(_, ctx)| live.contains(ctx.as_str()))
            .map(|(i, ctx)| format!("{} {}", i + 1, ctx))
            .collect()
    }

    /// True when there are any contexts at all — i.e. a tag list should be
    /// rendered.
    pub fn has_tags(&self) -> bool {
        !self.context_order.is_empty()
    }

    /// Distinct colors used across all selections (sorted by the order they
    /// appear in [`HighlightColor`]).  Mermaid uses this to emit one
    /// `classDef` per color actually in use.
    pub fn colors_used(&self) -> Vec<HighlightColor> {
        let mut set: BTreeSet<&'static str> = BTreeSet::new();
        let mut out: Vec<HighlightColor> = Vec::new();
        for sel in self.nodes.values().chain(self.edges.values()) {
            if set.insert(sel.color.slug()) {
                out.push(sel.color);
            }
        }
        out
    }

    /// Of the node IDs the caller added, those that no node in `present`
    /// matches.  Useful for warning on typos / stale references.
    pub fn unmatched_node_ids<'a, I>(&'a self, present: I) -> Vec<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let present: BTreeSet<&str> = present.into_iter().collect();
        self.nodes
            .keys()
            .filter(|id| !present.contains(id.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// Of the edge IDs the caller added, those that no edge in `present`
    /// matches.
    pub fn unmatched_edge_ids<'a, I>(&'a self, present: I) -> Vec<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let present: BTreeSet<&str> = present.into_iter().collect();
        self.edges
            .keys()
            .filter(|id| !present.contains(id.as_str()))
            .map(String::as_str)
            .collect()
    }

    fn record_context(&mut self, ctx: &str) {
        if !self.context_order.iter().any(|c| c == ctx) {
            self.context_order.push(ctx.to_string());
        }
    }

    fn context_ids(&self, ctxs: &[String]) -> Vec<usize> {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut out: Vec<usize> = Vec::new();
        for ctx in ctxs {
            if let Some(pos) = self.context_order.iter().position(|c| c == ctx) {
                let id = pos + 1;
                if seen.insert(id) {
                    out.push(id);
                }
            }
        }
        out
    }
}

/// Render a list of context IDs as the inline label that goes next to a
/// highlighted node or edge.  `[1,3,5]`.  Returns an empty string when
/// `ids` is empty so callers can `concat` unconditionally.
pub fn format_context_label(ids: &[usize]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = ids.iter().map(|n| n.to_string()).collect();
    format!("[{}]", parts.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_by_default() {
        let h = Highlights::new();
        assert!(h.is_empty());
        assert!(!h.has_tags());
        assert!(h.tag_lines().is_empty());
        assert!(h.colors_used().is_empty());
    }

    #[test]
    fn mark_node_no_context() {
        let mut h = Highlights::new();
        h.mark_node("srv");
        assert!(h.is_node_highlighted("srv"));
        assert!(h.node_context_ids("srv").is_empty());
        assert!(!h.has_tags());
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
    }

    #[test]
    fn highlight_node_adds_tag_entry() {
        let mut h = Highlights::new();
        h.highlight_node("srv", "outdated");
        assert!(h.is_node_highlighted("srv"));
        assert_eq!(h.node_context_ids("srv"), vec![1]);
        assert_eq!(h.tag_lines(), vec!["1 outdated".to_string()]);
    }

    #[test]
    fn context_ids_are_insertion_ordered_globally() {
        // doc example: ["A", "B", "A", "C"] should number A=1, B=2, C=3
        let mut h = Highlights::new();
        h.highlight_node("n1", "A");
        h.highlight_edge("e1", "B");
        h.highlight_node("n2", "A"); // duplicate context, reuses 1
        h.highlight_edge("e2", "C");

        assert_eq!(
            h.tag_lines(),
            vec!["1 A".to_string(), "2 B".to_string(), "3 C".to_string()]
        );
        assert_eq!(h.node_context_ids("n1"), vec![1]);
        assert_eq!(h.node_context_ids("n2"), vec![1]);
        assert_eq!(h.edge_context_ids("e1"), vec![2]);
        assert_eq!(h.edge_context_ids("e2"), vec![3]);
    }

    #[test]
    fn multiple_contexts_on_same_id_dedupe() {
        let mut h = Highlights::new();
        h.highlight_node("srv", "outdated");
        h.highlight_node("srv", "high-cpu");
        h.highlight_node("srv", "outdated"); // duplicate
        assert_eq!(h.node_context_ids("srv"), vec![1, 2]);
        assert_eq!(
            h.tag_lines(),
            vec!["1 outdated".to_string(), "2 high-cpu".to_string()]
        );
    }

    #[test]
    fn mark_then_highlight_promotes_to_tag_entry() {
        let mut h = Highlights::new();
        h.mark_node("srv");
        assert!(h.node_context_ids("srv").is_empty());
        h.highlight_node("srv", "drained");
        assert_eq!(h.node_context_ids("srv"), vec![1]);
    }

    #[test]
    fn color_override_per_selection() {
        let mut h = Highlights::new();
        h.mark_node_with("srv", HighlightColor::Orange);
        h.highlight_edge_with(
            "wasi:http/handler@0.3.0::middleware->srv",
            "drained",
            HighlightColor::Cyan,
        );
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Orange));
        assert_eq!(
            h.edge_color("wasi:http/handler@0.3.0::middleware->srv"),
            Some(HighlightColor::Cyan),
        );
        // colors_used returns each distinct color once
        let used = h.colors_used();
        assert_eq!(used.len(), 2);
        assert!(used.contains(&HighlightColor::Orange));
        assert!(used.contains(&HighlightColor::Cyan));
    }

    #[test]
    fn color_last_write_wins() {
        let mut h = Highlights::new();
        h.mark_node_with("srv", HighlightColor::Yellow);
        h.mark_node_with("srv", HighlightColor::Cyan);
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Cyan));
    }

    #[test]
    fn default_color_is_yellow() {
        let mut h = Highlights::new();
        h.mark_node("srv");
        h.highlight_edge("a::b->c", "ctx");
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
        assert_eq!(h.edge_color("a::b->c"), Some(HighlightColor::Yellow));
    }

    #[test]
    fn ansi_codes_distinct_per_color() {
        // Every variant has a non-empty open and a distinct one — guards
        // against accidental copy-paste collisions.
        let colors = [
            HighlightColor::Yellow,
            HighlightColor::Cyan,
            HighlightColor::Magenta,
            HighlightColor::Blue,
            HighlightColor::Orange,
            HighlightColor::White,
        ];
        let mut opens: BTreeSet<&'static str> = BTreeSet::new();
        for c in colors {
            assert!(!c.ansi_open().is_empty());
            assert!(opens.insert(c.ansi_open()), "duplicate ansi for {:?}", c);
        }
    }

    #[test]
    fn format_context_label_examples() {
        assert_eq!(format_context_label(&[]), "");
        assert_eq!(format_context_label(&[1]), "[1]");
        assert_eq!(format_context_label(&[1, 3, 5]), "[1,3,5]");
    }

    #[test]
    fn unmatched_node_ids_reports_typos() {
        let mut h = Highlights::new();
        h.mark_node("srv");
        h.mark_node("middlewre"); // typo
        let unmatched: Vec<&str> = h.unmatched_node_ids(["srv", "middleware"].iter().copied());
        assert_eq!(unmatched, vec!["middlewre"]);
    }

    #[test]
    fn unmatched_edge_ids_reports_typos() {
        let mut h = Highlights::new();
        h.mark_edge("wasi:http/handler@0.3.0::middleware->srv");
        h.mark_edge("nope::a->b");
        let present = ["wasi:http/handler@0.3.0::middleware->srv"];
        let unmatched: Vec<&str> = h.unmatched_edge_ids(present.iter().copied());
        assert_eq!(unmatched, vec!["nope::a->b"]);
    }

    #[test]
    fn missing_lookup_returns_empty() {
        let h = Highlights::new();
        assert!(!h.is_node_highlighted("nope"));
        assert!(h.node_context_ids("nope").is_empty());
        assert!(!h.is_edge_highlighted("nope"));
        assert!(h.edge_context_ids("nope").is_empty());
        assert_eq!(h.node_color("nope"), None);
        assert_eq!(h.edge_color("nope"), None);
    }
}
