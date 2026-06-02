//! Caller-supplied emphasis ("highlight these nodes/edges, and here's why")
//! that the graph renderers consume.
//!
//! Identity is by **canonical ID strings** (see [`canonical_id`]).  A caller
//! such as splicer computes which nodes and edges its YAML rules would touch,
//! stuffs the canonical IDs into a [`Highlights`], and hands it to cviz —
//! cviz then surfaces the emphasis in whichever output format it's asked to
//! produce.
//!
//! # Shape
//!
//! Two concepts:
//!
//! - **Tags** are `(tag_id: u32, ctx: String)` pairs the consumer registers
//!   up front via [`Highlights::register_tag`] (or
//!   [`Highlights::register_tags`] for a batch).  Tag IDs are consumer-owned
//!   — splicer's rule #5 can register as tag 5 and that's the number the
//!   renderer draws.
//! - **Selections** are nodes or edges the consumer highlights via
//!   [`Highlights::mark`], passing a [`Selection`] built with
//!   [`Selection::node`] / [`Selection::edge`], optionally attaching tags
//!   with [`Selection::tag`] / [`Selection::tags`] and an explicit color
//!   with [`Selection::color`].
//!
//! A selection with no tags is just an emphasized highlight (no `[N]`
//! bracket, no entry in the Tags list).  A selection with tags references
//! ones previously registered — citing an unknown tag id is a consumer
//! bug and panics at [`Highlights::mark`] time.
//!
//! # Colors
//!
//! Every selection has an effective [`HighlightColor`].  If the builder
//! didn't call [`Selection::color`], the renderer uses
//! [`HighlightColor::Yellow`] (a colorblind-safe default).  Consumers that
//! want to colorize different selections distinctly call `.color(...)` on
//! the relevant selections.
//!
//! [`canonical_id`]: crate::canonical_id

use std::collections::{BTreeMap, BTreeSet};

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

/// Returned by [`Highlights::register_tag`] when a tag id is already
/// registered to a different context string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagConflict {
    pub tag_id: u32,
    pub existing_ctx: String,
    pub new_ctx: String,
}

impl std::fmt::Display for TagConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tag id {} already registered to {:?}; attempted to re-register as {:?}",
            self.tag_id, self.existing_ctx, self.new_ctx,
        )
    }
}

impl std::error::Error for TagConflict {}

/// What the [`Selection`] builder is producing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionKind {
    Node,
    Edge,
}

/// One node or edge the consumer wants emphasized.
///
/// Construct via [`Selection::node`] or [`Selection::edge`], chain
/// `.tag(...)` / `.tags(...)` / `.color(...)` to attach metadata, and pass
/// to [`Highlights::add`].
#[derive(Debug, Clone)]
pub struct Selection {
    kind: SelectionKind,
    id: String,
    tags: Vec<u32>,
    color: Option<HighlightColor>,
}

impl Selection {
    /// Start a node selection for the given canonical node id.
    pub fn node(id: impl Into<String>) -> Self {
        Self {
            kind: SelectionKind::Node,
            id: id.into(),
            tags: Vec::new(),
            color: None,
        }
    }

    /// Start an edge selection for the given canonical edge id (e.g.
    /// `wasi:http/handler@0.3.0::middleware->srv`).
    pub fn edge(id: impl Into<String>) -> Self {
        Self {
            kind: SelectionKind::Edge,
            id: id.into(),
            tags: Vec::new(),
            color: None,
        }
    }

    /// Attach one tag.  The tag id must have been registered via
    /// [`Highlights::register_tag`] (or
    /// [`Highlights::register_tags`]) before the selection is passed to
    /// [`Highlights::mark`].
    pub fn tag(mut self, tag_id: u32) -> Self {
        self.tags.push(tag_id);
        self
    }

    /// Attach multiple tags at once.
    pub fn tags<I>(mut self, tag_ids: I) -> Self
    where
        I: IntoIterator<Item = u32>,
    {
        self.tags.extend(tag_ids);
        self
    }

    /// Override the default color ([`HighlightColor::Yellow`]).
    pub fn color(mut self, color: HighlightColor) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(Debug, Clone)]
struct StoredSelection {
    tags: Vec<u32>,
    color: HighlightColor,
}

/// A set of highlighted node and edge IDs, plus a tag pool the consumer
/// owns and an optional per-selection color.
///
/// See the [module docs](self) for the registration-and-add flow, the
/// tag-id ownership contract, and the color contract.
#[derive(Debug, Clone, Default)]
pub struct Highlights {
    nodes: BTreeMap<String, StoredSelection>,
    edges: BTreeMap<String, StoredSelection>,
    /// Consumer-registered (id → ctx) tag pool.
    tags: BTreeMap<u32, String>,
}

impl Highlights {
    /// New empty [`Highlights`].  Equivalent to [`Highlights::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a single tag.
    ///
    /// `tag_id` is the consumer-chosen number that the renderer will draw
    /// (so the diagram bracket and the Tags list both read `[5]` /
    /// `5 spliced` when the consumer registers tag `5`).
    ///
    /// Repeated registration of the same `(tag_id, ctx)` is idempotent.
    /// Re-registering the same `tag_id` with a different `ctx` returns
    /// [`TagConflict`].
    pub fn register_tag(&mut self, tag_id: u32, ctx: impl Into<String>) -> Result<(), TagConflict> {
        let ctx = ctx.into();
        if let Some(existing) = self.tags.get(&tag_id) {
            if existing == &ctx {
                return Ok(());
            }
            return Err(TagConflict {
                tag_id,
                existing_ctx: existing.clone(),
                new_ctx: ctx,
            });
        }
        self.tags.insert(tag_id, ctx);
        Ok(())
    }

    /// Register a batch of tags.  Stops at the first conflict and returns
    /// it; any tags before the conflict are still registered (so callers
    /// who want all-or-nothing semantics should validate up front).
    pub fn register_tags<I, S>(&mut self, tags: I) -> Result<(), TagConflict>
    where
        I: IntoIterator<Item = (u32, S)>,
        S: Into<String>,
    {
        for (tag_id, ctx) in tags {
            self.register_tag(tag_id, ctx)?;
        }
        Ok(())
    }

    /// Mark a node or edge as highlighted, applying the metadata in the
    /// built [`Selection`].
    ///
    /// Panics if `selection` cites a tag id that hasn't been registered —
    /// that's a consumer bug (you cited a tag you didn't register),
    /// not a runtime condition worth `Result`-threading.
    ///
    /// Re-marking the same id (node or edge) replaces the previous tags
    /// and color for that id — later writes win.
    pub fn mark(&mut self, selection: Selection) {
        for tag_id in &selection.tags {
            assert!(
                self.tags.contains_key(tag_id),
                "Highlights::mark: unregistered tag id {tag_id} (register with `register_tag` first)"
            );
        }
        // Dedup while preserving insertion order — same tag listed twice
        // should still only render once in the `[N,M]` bracket.
        let mut seen = BTreeSet::new();
        let mut tags = Vec::with_capacity(selection.tags.len());
        for t in selection.tags {
            if seen.insert(t) {
                tags.push(t);
            }
        }
        let stored = StoredSelection {
            tags,
            color: selection.color.unwrap_or_default(),
        };
        match selection.kind {
            SelectionKind::Node => {
                self.nodes.insert(selection.id, stored);
            }
            SelectionKind::Edge => {
                self.edges.insert(selection.id, stored);
            }
        }
    }

    /// True when no node or edge has been added.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// True iff `id` was added as a node selection.
    pub fn is_node_highlighted(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// True iff `id` was added as an edge selection.
    pub fn is_edge_highlighted(&self, id: &str) -> bool {
        self.edges.contains_key(id)
    }

    /// Effective color for node `id`, or `None` if not highlighted.
    pub fn node_color(&self, id: &str) -> Option<HighlightColor> {
        self.nodes.get(id).map(|sel| sel.color)
    }

    /// Effective color for edge `id`, or `None` if not highlighted.
    pub fn edge_color(&self, id: &str) -> Option<HighlightColor> {
        self.edges.get(id).map(|sel| sel.color)
    }

    /// Tag IDs attached to node `id`, in attach order with duplicates
    /// removed.  Empty when the selection had no tags or `id` isn't
    /// highlighted.
    pub fn node_tag_ids(&self, id: &str) -> Vec<u32> {
        self.nodes
            .get(id)
            .map(|sel| sel.tags.clone())
            .unwrap_or_default()
    }

    /// Tag IDs attached to edge `id`.  See
    /// [`node_tag_ids`](Self::node_tag_ids).
    pub fn edge_tag_ids(&self, id: &str) -> Vec<u32> {
        self.edges
            .get(id)
            .map(|sel| sel.tags.clone())
            .unwrap_or_default()
    }

    /// Tag lines, one per registered tag, sorted by tag id.  Each line is
    /// formatted `"N <ctx>"`.
    pub fn tag_lines(&self) -> Vec<String> {
        self.tags
            .iter()
            .map(|(id, ctx)| format!("{} {}", id, ctx))
            .collect()
    }

    /// Like [`Self::tag_lines`], but filtered to tags actually attached to
    /// at least one *matched* node or edge id.  `present_nodes` and
    /// `present_edges` are the canonical IDs the caller knows exist in
    /// the rendered graph; tags whose only attachments are unmatched
    /// (typo'd) IDs are omitted so the Tags list and the in-diagram
    /// brackets stay consistent.
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
        let mut live: BTreeSet<u32> = BTreeSet::new();
        for (id, sel) in &self.nodes {
            if node_set.contains(id) {
                live.extend(sel.tags.iter().copied());
            }
        }
        for (id, sel) in &self.edges {
            if edge_set.contains(id) {
                live.extend(sel.tags.iter().copied());
            }
        }
        self.tags
            .iter()
            .filter(|(id, _)| live.contains(id))
            .map(|(id, ctx)| format!("{} {}", id, ctx))
            .collect()
    }

    /// True when at least one tag has been registered.  Note this can be
    /// true even when no selections cite any tag — the consumer registered
    /// a pool but only used a subset (or none yet).
    pub fn has_tags(&self) -> bool {
        !self.tags.is_empty()
    }

    /// Distinct colors used across all selections.  Mermaid uses this to
    /// emit one `classDef` per color actually in use.
    pub fn colors_used(&self) -> Vec<HighlightColor> {
        let mut seen: BTreeSet<&'static str> = BTreeSet::new();
        let mut out: Vec<HighlightColor> = Vec::new();
        for sel in self.nodes.values().chain(self.edges.values()) {
            if seen.insert(sel.color.slug()) {
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
}

/// Render a list of tag IDs as the inline label that goes next to a
/// highlighted node or edge.  `[1,3,5]`.  Returns an empty string when
/// `ids` is empty so callers can `concat` unconditionally.
pub fn format_tag_label(ids: &[u32]) -> String {
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
    fn highlight_no_tags_no_color() {
        let mut h = Highlights::new();
        h.mark(Selection::node("srv"));
        assert!(h.is_node_highlighted("srv"));
        assert!(h.node_tag_ids("srv").is_empty());
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
        assert!(!h.has_tags());
    }

    #[test]
    fn register_then_attach() {
        let mut h = Highlights::new();
        h.register_tag(1, "outdated").unwrap();
        h.mark(Selection::node("srv").tag(1));
        assert_eq!(h.node_tag_ids("srv"), vec![1]);
        assert_eq!(h.tag_lines(), vec!["1 outdated".to_string()]);
    }

    #[test]
    fn consumer_chosen_tag_ids() {
        let mut h = Highlights::new();
        h.register_tags([(5, "spliced"), (8, "drained")]).unwrap();
        h.mark(Selection::node("srv").tag(5));
        h.mark(Selection::edge("a::b->c").tags([5, 8]));
        assert_eq!(h.node_tag_ids("srv"), vec![5]);
        assert_eq!(h.edge_tag_ids("a::b->c"), vec![5, 8]);
        // Tag list is sorted by id (BTreeMap iteration order).
        assert_eq!(
            h.tag_lines(),
            vec!["5 spliced".to_string(), "8 drained".to_string()]
        );
    }

    #[test]
    fn duplicate_tags_on_same_selection_dedupe() {
        let mut h = Highlights::new();
        h.register_tag(1, "outdated").unwrap();
        h.mark(Selection::node("srv").tag(1).tag(1));
        assert_eq!(h.node_tag_ids("srv"), vec![1]);
    }

    #[test]
    fn register_tag_idempotent_same_ctx() {
        let mut h = Highlights::new();
        h.register_tag(5, "spliced").unwrap();
        h.register_tag(5, "spliced").unwrap();
        assert_eq!(h.tag_lines(), vec!["5 spliced".to_string()]);
    }

    #[test]
    fn register_tag_conflict_returns_err() {
        let mut h = Highlights::new();
        h.register_tag(5, "spliced").unwrap();
        let err = h.register_tag(5, "different").unwrap_err();
        assert_eq!(err.tag_id, 5);
        assert_eq!(err.existing_ctx, "spliced");
        assert_eq!(err.new_ctx, "different");
    }

    #[test]
    #[should_panic(expected = "unregistered tag id 99")]
    fn mark_with_unregistered_tag_panics() {
        let mut h = Highlights::new();
        h.mark(Selection::node("srv").tag(99));
    }

    #[test]
    fn color_override_per_selection() {
        let mut h = Highlights::new();
        h.mark(Selection::node("srv").color(HighlightColor::Orange));
        h.mark(Selection::edge("a::b->c").color(HighlightColor::Cyan));
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Orange));
        assert_eq!(h.edge_color("a::b->c"), Some(HighlightColor::Cyan));
        let used = h.colors_used();
        assert_eq!(used.len(), 2);
        assert!(used.contains(&HighlightColor::Orange));
        assert!(used.contains(&HighlightColor::Cyan));
    }

    #[test]
    fn re_marking_same_id_replaces_previous() {
        let mut h = Highlights::new();
        h.register_tag(1, "outdated").unwrap();
        h.register_tag(2, "drained").unwrap();
        h.mark(Selection::node("srv").tag(1));
        h.mark(Selection::node("srv").tag(2).color(HighlightColor::Cyan));
        assert_eq!(h.node_tag_ids("srv"), vec![2]);
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Cyan));
    }

    #[test]
    fn default_color_is_yellow() {
        let mut h = Highlights::new();
        h.mark(Selection::node("srv"));
        h.mark(Selection::edge("a::b->c"));
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
        assert_eq!(h.edge_color("a::b->c"), Some(HighlightColor::Yellow));
    }

    #[test]
    fn tag_lines_referenced_by_filters_unmatched() {
        let mut h = Highlights::new();
        h.register_tags([(1, "spliced"), (2, "sup")]).unwrap();
        h.mark(Selection::node("real").tag(2));
        h.mark(Selection::edge("bogus::a->b").tag(1));
        let lines = h.tag_lines_referenced_by(["real"], ["e1::a->b"]);
        // Only `2 sup` survives — tag 1 is only attached to the bogus edge.
        assert_eq!(lines, vec!["2 sup".to_string()]);
    }

    #[test]
    fn tag_lines_sorted_by_id() {
        let mut h = Highlights::new();
        h.register_tags([(10, "ten"), (3, "three"), (7, "seven")])
            .unwrap();
        assert_eq!(
            h.tag_lines(),
            vec![
                "3 three".to_string(),
                "7 seven".to_string(),
                "10 ten".to_string(),
            ]
        );
    }

    #[test]
    fn ansi_codes_distinct_per_color() {
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
    fn format_tag_label_examples() {
        assert_eq!(format_tag_label(&[]), "");
        assert_eq!(format_tag_label(&[1]), "[1]");
        assert_eq!(format_tag_label(&[1, 3, 5]), "[1,3,5]");
    }

    #[test]
    fn unmatched_node_ids_reports_typos() {
        let mut h = Highlights::new();
        h.mark(Selection::node("srv"));
        h.mark(Selection::node("middlewre"));
        let unmatched: Vec<&str> = h.unmatched_node_ids(["srv", "middleware"].iter().copied());
        assert_eq!(unmatched, vec!["middlewre"]);
    }

    #[test]
    fn unmatched_edge_ids_reports_typos() {
        let mut h = Highlights::new();
        h.mark(Selection::edge("wasi:http/handler@0.3.0::middleware->srv"));
        h.mark(Selection::edge("nope::a->b"));
        let present = ["wasi:http/handler@0.3.0::middleware->srv"];
        let unmatched: Vec<&str> = h.unmatched_edge_ids(present.iter().copied());
        assert_eq!(unmatched, vec!["nope::a->b"]);
    }

    #[test]
    fn missing_lookup_returns_empty() {
        let h = Highlights::new();
        assert!(!h.is_node_highlighted("nope"));
        assert!(h.node_tag_ids("nope").is_empty());
        assert!(!h.is_edge_highlighted("nope"));
        assert!(h.edge_tag_ids("nope").is_empty());
        assert_eq!(h.node_color("nope"), None);
        assert_eq!(h.edge_color("nope"), None);
    }
}
