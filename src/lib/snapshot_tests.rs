/// Snapshot (golden-file) tests for the full rendered output of every
/// `(graph_builder × format × detail_level)` combination.
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
    use crate::output::{ascii, mermaid, DetailLevel, Direction};
    use crate::test_utils::*;

    fn ascii_snap(graph: &CompositionGraph, detail: DetailLevel, show_types: bool) -> String {
        ascii::generate_ascii(graph, detail, show_types)
    }

    fn mermaid_snap(graph: &CompositionGraph, detail: DetailLevel, show_types: bool) -> String {
        mermaid::generate_mermaid(graph, detail, Direction::LeftToRight, show_types)
    }

    // -----------------------------------------------------------------------
    // simple_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn simple_chain_ascii_handler_chain() {
        insta::assert_snapshot!(ascii_snap(&simple_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn simple_chain_ascii_all_interfaces() {
        insta::assert_snapshot!(ascii_snap(&simple_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn simple_chain_ascii_full() {
        insta::assert_snapshot!(ascii_snap(&simple_chain_graph(), DetailLevel::Full, false));
    }

    #[test]
    fn simple_chain_mermaid_handler_chain() {
        insta::assert_snapshot!(mermaid_snap(&simple_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn simple_chain_mermaid_all_interfaces() {
        insta::assert_snapshot!(mermaid_snap(&simple_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn simple_chain_mermaid_full() {
        insta::assert_snapshot!(mermaid_snap(&simple_chain_graph(), DetailLevel::Full, false));
    }

    // -----------------------------------------------------------------------
    // two_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn two_chain_ascii_handler_chain() {
        insta::assert_snapshot!(ascii_snap(&two_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn two_chain_ascii_all_interfaces() {
        insta::assert_snapshot!(ascii_snap(&two_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn two_chain_ascii_full() {
        insta::assert_snapshot!(ascii_snap(&two_chain_graph(), DetailLevel::Full, false));
    }

    #[test]
    fn two_chain_mermaid_handler_chain() {
        insta::assert_snapshot!(mermaid_snap(&two_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn two_chain_mermaid_all_interfaces() {
        insta::assert_snapshot!(mermaid_snap(&two_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn two_chain_mermaid_full() {
        insta::assert_snapshot!(mermaid_snap(&two_chain_graph(), DetailLevel::Full, false));
    }

    // -----------------------------------------------------------------------
    // long_chain_graph
    // -----------------------------------------------------------------------

    #[test]
    fn long_chain_ascii_handler_chain() {
        insta::assert_snapshot!(ascii_snap(&long_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn long_chain_ascii_all_interfaces() {
        insta::assert_snapshot!(ascii_snap(&long_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn long_chain_ascii_full() {
        insta::assert_snapshot!(ascii_snap(&long_chain_graph(), DetailLevel::Full, false));
    }

    #[test]
    fn long_chain_mermaid_handler_chain() {
        insta::assert_snapshot!(mermaid_snap(&long_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn long_chain_mermaid_all_interfaces() {
        insta::assert_snapshot!(mermaid_snap(&long_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn long_chain_mermaid_full() {
        insta::assert_snapshot!(mermaid_snap(&long_chain_graph(), DetailLevel::Full, false));
    }

    // -----------------------------------------------------------------------
    // chain_plus_utility_graph
    // -----------------------------------------------------------------------

    #[test]
    fn chain_plus_utility_ascii_handler_chain() {
        insta::assert_snapshot!(ascii_snap(
            &chain_plus_utility_graph(),
            DetailLevel::HandlerChain,
            false
        ));
    }

    #[test]
    fn chain_plus_utility_ascii_all_interfaces() {
        insta::assert_snapshot!(ascii_snap(
            &chain_plus_utility_graph(),
            DetailLevel::AllInterfaces,
            false
        ));
    }

    #[test]
    fn chain_plus_utility_ascii_full() {
        insta::assert_snapshot!(ascii_snap(
            &chain_plus_utility_graph(),
            DetailLevel::Full,
            false
        ));
    }

    #[test]
    fn chain_plus_utility_mermaid_handler_chain() {
        insta::assert_snapshot!(mermaid_snap(
            &chain_plus_utility_graph(),
            DetailLevel::HandlerChain,
            false
        ));
    }

    #[test]
    fn chain_plus_utility_mermaid_all_interfaces() {
        insta::assert_snapshot!(mermaid_snap(
            &chain_plus_utility_graph(),
            DetailLevel::AllInterfaces,
            false
        ));
    }

    #[test]
    fn chain_plus_utility_mermaid_full() {
        insta::assert_snapshot!(mermaid_snap(
            &chain_plus_utility_graph(),
            DetailLevel::Full,
            false
        ));
    }

    // -----------------------------------------------------------------------
    // typed_chain_graph  (show_types=false and show_types=true)
    // -----------------------------------------------------------------------

    #[test]
    fn typed_chain_ascii_handler_chain_no_types() {
        insta::assert_snapshot!(ascii_snap(&typed_chain_graph(), DetailLevel::HandlerChain, false));
    }

    #[test]
    fn typed_chain_ascii_handler_chain_with_types() {
        insta::assert_snapshot!(ascii_snap(&typed_chain_graph(), DetailLevel::HandlerChain, true));
    }

    #[test]
    fn typed_chain_ascii_all_interfaces_no_types() {
        insta::assert_snapshot!(ascii_snap(&typed_chain_graph(), DetailLevel::AllInterfaces, false));
    }

    #[test]
    fn typed_chain_ascii_all_interfaces_with_types() {
        insta::assert_snapshot!(ascii_snap(&typed_chain_graph(), DetailLevel::AllInterfaces, true));
    }

    #[test]
    fn typed_chain_ascii_full_no_types() {
        insta::assert_snapshot!(ascii_snap(&typed_chain_graph(), DetailLevel::Full, false));
    }

    #[test]
    fn typed_chain_ascii_full_with_types() {
        insta::assert_snapshot!(ascii_snap(&typed_chain_graph(), DetailLevel::Full, true));
    }

    #[test]
    fn typed_chain_mermaid_handler_chain_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &typed_chain_graph(),
            DetailLevel::HandlerChain,
            false
        ));
    }

    #[test]
    fn typed_chain_mermaid_handler_chain_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &typed_chain_graph(),
            DetailLevel::HandlerChain,
            true
        ));
    }

    #[test]
    fn typed_chain_mermaid_all_interfaces_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &typed_chain_graph(),
            DetailLevel::AllInterfaces,
            false
        ));
    }

    #[test]
    fn typed_chain_mermaid_all_interfaces_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &typed_chain_graph(),
            DetailLevel::AllInterfaces,
            true
        ));
    }

    #[test]
    fn typed_chain_mermaid_full_no_types() {
        insta::assert_snapshot!(mermaid_snap(&typed_chain_graph(), DetailLevel::Full, false));
    }

    #[test]
    fn typed_chain_mermaid_full_with_types() {
        insta::assert_snapshot!(mermaid_snap(&typed_chain_graph(), DetailLevel::Full, true));
    }

    // -----------------------------------------------------------------------
    // two_typed_chain_graph  (show_types=false and show_types=true)
    // -----------------------------------------------------------------------

    #[test]
    fn two_typed_chain_ascii_handler_chain_no_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            DetailLevel::HandlerChain,
            false
        ));
    }

    #[test]
    fn two_typed_chain_ascii_handler_chain_with_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            DetailLevel::HandlerChain,
            true
        ));
    }

    #[test]
    fn two_typed_chain_ascii_all_interfaces_no_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            DetailLevel::AllInterfaces,
            false
        ));
    }

    #[test]
    fn two_typed_chain_ascii_all_interfaces_with_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            DetailLevel::AllInterfaces,
            true
        ));
    }

    #[test]
    fn two_typed_chain_ascii_full_no_types() {
        insta::assert_snapshot!(ascii_snap(
            &two_typed_chain_graph(),
            DetailLevel::Full,
            false
        ));
    }

    #[test]
    fn two_typed_chain_ascii_full_with_types() {
        insta::assert_snapshot!(ascii_snap(&two_typed_chain_graph(), DetailLevel::Full, true));
    }

    #[test]
    fn two_typed_chain_mermaid_handler_chain_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            DetailLevel::HandlerChain,
            false
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_handler_chain_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            DetailLevel::HandlerChain,
            true
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_all_interfaces_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            DetailLevel::AllInterfaces,
            false
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_all_interfaces_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            DetailLevel::AllInterfaces,
            true
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_full_no_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            DetailLevel::Full,
            false
        ));
    }

    #[test]
    fn two_typed_chain_mermaid_full_with_types() {
        insta::assert_snapshot!(mermaid_snap(
            &two_typed_chain_graph(),
            DetailLevel::Full,
            true
        ));
    }
}
