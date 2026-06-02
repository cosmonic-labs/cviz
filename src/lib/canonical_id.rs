//! Canonical string identifiers for nodes and edges in a
//! [`CompositionGraph`] — the cross-tool naming contract for composition
//! artifacts.

use crate::model::{ComponentNode, CompositionGraph};

impl ComponentNode {
    /// Canonical string identifier for this node within its composition.
    /// Equivalent to [`Self::display_label`].
    pub fn canonical_id(&self) -> &str {
        self.display_label()
    }
}

/// Canonical ID for an edge.  `caller = None` for a boundary edge
/// (exported interface); `caller = Some(...)` for an internal edge.
///
/// # Examples
///
/// ```
/// use cviz::canonical_id::canonical_edge_id;
/// assert_eq!(
///     canonical_edge_id("wasi:http/handler@0.3.0", Some("middleware"), "srv"),
///     "wasi:http/handler@0.3.0::middleware->srv",
/// );
/// assert_eq!(
///     canonical_edge_id("wasi:http/handler@0.3.0", None, "middleware"),
///     "wasi:http/handler@0.3.0::->middleware",
/// );
/// ```
pub fn canonical_edge_id(interface: &str, caller: Option<&str>, provider: &str) -> String {
    let caller = caller.unwrap_or("");
    format!("{interface}::{caller}->{provider}")
}

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
