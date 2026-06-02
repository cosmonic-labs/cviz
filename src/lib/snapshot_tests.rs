/// Snapshot (golden-file) tests for the full rendered output of every
/// `(graph_builder × format × opts)` combination.
///
/// Snapshots live in `src/lib/snapshots/` and are managed by the `insta`
/// crate.  To review and accept new or changed snapshots run:
///
///   cargo insta review
///
/// Or accept all pending snapshots without interactive review:
///
///   cargo insta accept
#[cfg(test)]
mod tests {
    use crate::model::CompositionGraph;
    use crate::output::graph::{generate_graph_ascii, GraphRenderOpts};
    use crate::output::{mermaid, Direction};
    use crate::test_utils::*;

    fn ascii_snap(graph: &CompositionGraph, opts: &GraphRenderOpts, show_types: bool) -> String {
        generate_graph_ascii(graph, opts, show_types, None, None, false).ascii
    }

    fn mermaid_snap(graph: &CompositionGraph, opts: &GraphRenderOpts, show_types: bool) -> String {
        mermaid::generate_mermaid(graph, opts, Direction::LeftToRight, show_types, None)
    }

    fn chain_only() -> GraphRenderOpts {
        GraphRenderOpts {
            chain_only: true,
            ..Default::default()
        }
    }

    fn host_imports() -> GraphRenderOpts {
        GraphRenderOpts {
            show_host_imports: true,
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------
    // simple_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn simple_chain_ascii_default() {
        insta::assert_snapshot!(ascii_snap(
            &simple_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn simple_chain_ascii_chain_only() {
        insta::assert_snapshot!(ascii_snap(&simple_chain_graph(), &chain_only(), false));
    }

    #[test]
    fn simple_chain_ascii_host_imports() {
        insta::assert_snapshot!(ascii_snap(&simple_chain_graph(), &host_imports(), false));
    }

    #[test]
    fn simple_chain_mermaid_default() {
        insta::assert_snapshot!(mermaid_snap(
            &simple_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn simple_chain_mermaid_chain_only() {
        insta::assert_snapshot!(mermaid_snap(&simple_chain_graph(), &chain_only(), false));
    }

    #[test]
    fn simple_chain_mermaid_host_imports() {
        insta::assert_snapshot!(mermaid_snap(&simple_chain_graph(), &host_imports(), false));
    }

    // -----------------------------------------------------------------------
    // two_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn two_chain_ascii_default() {
        insta::assert_snapshot!(ascii_snap(
            &two_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn two_chain_ascii_chain_only() {
        insta::assert_snapshot!(ascii_snap(&two_chain_graph(), &chain_only(), false));
    }

    #[test]
    fn two_chain_mermaid_default() {
        insta::assert_snapshot!(mermaid_snap(
            &two_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn two_chain_mermaid_chain_only() {
        insta::assert_snapshot!(mermaid_snap(&two_chain_graph(), &chain_only(), false));
    }

    // -----------------------------------------------------------------------
    // long_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn long_chain_ascii_default() {
        insta::assert_snapshot!(ascii_snap(
            &long_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn long_chain_ascii_chain_only() {
        insta::assert_snapshot!(ascii_snap(&long_chain_graph(), &chain_only(), false));
    }

    #[test]
    fn long_chain_mermaid_default() {
        insta::assert_snapshot!(mermaid_snap(
            &long_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn long_chain_mermaid_chain_only() {
        insta::assert_snapshot!(mermaid_snap(&long_chain_graph(), &chain_only(), false));
    }

    // -----------------------------------------------------------------------
    // chain_plus_utility_graph
    // -----------------------------------------------------------------------

    #[test]
    fn chain_plus_utility_ascii_default() {
        insta::assert_snapshot!(ascii_snap(
            &chain_plus_utility_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn chain_plus_utility_ascii_chain_only() {
        insta::assert_snapshot!(ascii_snap(
            &chain_plus_utility_graph(),
            &chain_only(),
            false
        ));
    }

    #[test]
    fn chain_plus_utility_mermaid_default() {
        insta::assert_snapshot!(mermaid_snap(
            &chain_plus_utility_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn chain_plus_utility_mermaid_chain_only() {
        insta::assert_snapshot!(mermaid_snap(
            &chain_plus_utility_graph(),
            &chain_only(),
            false
        ));
    }

    // -----------------------------------------------------------------------
    // typed_chain_graph (show_types matrix)
    // -----------------------------------------------------------------------

    #[test]
    fn typed_chain_ascii_default_no_types() {
        insta::assert_snapshot!(ascii_snap(
            &typed_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn typed_chain_ascii_default_with_types() {
        insta::assert_snapshot!(ascii_snap(
            &typed_chain_graph(),
            &GraphRenderOpts::default(),
            true
        ));
    }

    #[test]
    fn typed_chain_mermaid_default_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &typed_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn typed_chain_mermaid_default_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &typed_chain_graph(),
            &GraphRenderOpts::default(),
            true
        ));
    }

    // -----------------------------------------------------------------------
    // two_typed_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn two_typed_chain_ascii_default_no_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn two_typed_chain_ascii_default_with_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            &GraphRenderOpts::default(),
            true
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_default_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            &GraphRenderOpts::default(),
            false
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_default_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            &GraphRenderOpts::default(),
            true
        ));
    }
}
