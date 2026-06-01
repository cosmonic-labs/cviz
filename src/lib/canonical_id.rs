//! Canonical string identifiers for nodes and edges in a
//! [`CompositionGraph`].
//!
//! These IDs are the cross-tool naming contract for composition artifacts.
//! Splicer (and any future composition-analysis tool) names selections with
//! these strings, the recorder archives them as filenames, and cviz uses them
//! to render highlights.
//!
//! # Stability contract
//!
//! - **Stable** across runs on the same composition bytes.
//! - **Stable** across cviz patch releases.
//! - **Not stable** across cviz minor/major releases without a deliberate
//!   major version bump and migration note.
//!
//! Every change to a canonical-ID format must be evaluated against the
//! load-bearing archived-recording use case ("does this invalidate previously
//! archived `<edge_id>.bin` files?"). Format changes are breaking changes.
//!
//! # Formats
//!
//! - Node ID: the existing [`ComponentNode::display_label`] (the instance
//!   name with the leading `$` stripped). No format change — promoting an
//!   existing concept.
//! - Edge ID:
//!   - Internal edge: `{interface}::{caller}->{provider}`
//!   - Boundary edge (no caller inside the composition, e.g. an exported
//!     interface): `{interface}::->{provider}`
//!
//! The internal edge form mirrors splicer's `derive_edge_id` so existing
//! archived edge IDs remain valid.

use crate::model::{ComponentNode, CompositionGraph};

impl ComponentNode {
    /// Canonical string identifier for this node within its composition.
    ///
    /// Equivalent to [`Self::display_label`]. Stability guarantees match the
    /// rest of the [`canonical_id`](crate::canonical_id) module: stable across
    /// runs on the same composition bytes and across patch releases.
    pub fn canonical_id(&self) -> &str {
        self.display_label()
    }
}

/// Compute the canonical ID for an edge.
///
/// Caller-friendly variant for tools that already hold the relevant strings
/// (e.g. splicer iterating over `SpliceSite`s). For an edge inside a
/// composition pass `caller = Some(...)`; for a boundary edge (exported
/// interface) pass `caller = None`.
///
/// # Examples
///
/// ```
/// use cviz::canonical_id::canonical_edge_id;
/// // Internal edge: middleware → srv on the http handler interface
/// assert_eq!(
///     canonical_edge_id("wasi:http/handler@0.3.0", Some("middleware"), "srv"),
///     "wasi:http/handler@0.3.0::middleware->srv",
/// );
/// // Boundary edge: composition exports the interface from `middleware`
/// assert_eq!(
///     canonical_edge_id("wasi:http/handler@0.3.0", None, "middleware"),
///     "wasi:http/handler@0.3.0::->middleware",
/// );
/// ```
pub fn canonical_edge_id(interface: &str, caller: Option<&str>, provider: &str) -> String {
    let caller = caller.unwrap_or("");
    format!("{interface}::{caller}->{provider}")
}

/// Look up a node in `graph` by its canonical ID.
///
/// Returns the first node whose [`ComponentNode::canonical_id`] equals `id`,
/// or `None` if no node matches. Instance names are unique within a real
/// composition, so the lookup is unambiguous in practice.
pub fn node_by_canonical_id<'g>(
    graph: &'g CompositionGraph,
    id: &str,
) -> Option<&'g ComponentNode> {
    graph.nodes.values().find(|n| n.canonical_id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ComponentNode;

    #[test]
    fn node_canonical_id_strips_dollar() {
        let node = ComponentNode::new("$srv".to_string(), 0, 0);
        assert_eq!(node.canonical_id(), "srv");
    }

    #[test]
    fn node_canonical_id_no_dollar_unchanged() {
        let node = ComponentNode::new("handler-shim".to_string(), 0, 0);
        assert_eq!(node.canonical_id(), "handler-shim");
    }

    #[test]
    fn edge_internal_format() {
        assert_eq!(
            canonical_edge_id("wasi:http/handler@0.3.0", Some("middleware"), "srv"),
            "wasi:http/handler@0.3.0::middleware->srv",
        );
    }

    #[test]
    fn edge_boundary_format() {
        assert_eq!(
            canonical_edge_id("wasi:http/handler@0.3.0", None, "middleware"),
            "wasi:http/handler@0.3.0::->middleware",
        );
    }

    #[test]
    fn node_by_canonical_id_roundtrip() {
        let mut graph = CompositionGraph::new();
        let srv = ComponentNode::new("$srv".to_string(), 0, 0);
        let mw = ComponentNode::new("$middleware".to_string(), 1, 1);
        graph.add_node(1, srv);
        graph.add_node(2, mw);

        let looked_up = node_by_canonical_id(&graph, "middleware").expect("found");
        assert_eq!(looked_up.canonical_id(), "middleware");
        assert_eq!(looked_up.component_index, 1);
    }

    #[test]
    fn node_by_canonical_id_missing_returns_none() {
        let graph = CompositionGraph::new();
        assert!(node_by_canonical_id(&graph, "nope").is_none());
    }

    #[test]
    fn node_by_canonical_id_ignores_dollar_in_query() {
        // Querying with a leading $ should NOT match — canonical IDs do not
        // contain it. This guards against callers accidentally passing the raw
        // instance name and silently failing to highlight.
        let mut graph = CompositionGraph::new();
        graph.add_node(1, ComponentNode::new("$srv".to_string(), 0, 0));
        assert!(node_by_canonical_id(&graph, "$srv").is_none());
        assert!(node_by_canonical_id(&graph, "srv").is_some());
    }

    /// Regression guard: the edge format is a public contract underpinning
    /// archived recordings. Any change here is a breaking change.
    #[test]
    fn edge_format_is_stable_contract() {
        assert_eq!(
            canonical_edge_id("a:b/c@1.0.0", Some("foo"), "bar"),
            "a:b/c@1.0.0::foo->bar",
        );
        assert_eq!(
            canonical_edge_id("a:b/c@1.0.0", None, "bar"),
            "a:b/c@1.0.0::->bar"
        );
    }
}
