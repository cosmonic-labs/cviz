//! Demo: cviz visualization across six composition topologies.
//!
//! Compiles WAT fixture files at runtime (using the `wat` crate) and runs the
//! cviz visualizer on each with several flag combinations.
//!
//! Run with:
//!   cargo run --example demo
//!
//! ASCII composition graphs are printed to the terminal.
//! Mermaid diagrams are printed to the terminal AND written to demo/out/*.mmd —
//! open them in https://mermaid.live or render with `mmdc -i <file>.mmd -o <file>.svg`.

use cviz::output::{DetailLevel, Direction};
use cviz::{output, parse};
use std::path::Path;

// Embed WAT sources at compile time so the example needs no external tooling.
const WAT_01: &str = include_str!("../demo/wat/01-simple-chain.wat");
const WAT_02: &str = include_str!("../demo/wat/02-three-layer-stack.wat");
const WAT_03: &str = include_str!("../demo/wat/03-multi-chain.wat");
const WAT_04: &str = include_str!("../demo/wat/04-chain-plus-utility.wat");
const WAT_05: &str = include_str!("../demo/wat/05-non-http-chain.wat");
const WAT_06: &str = include_str!("../demo/wat/06-typed-chain.wat");

const OUT_DIR: &str = "demo/out";

fn header(title: &str) {
    let bar = "═".repeat(60);
    println!("\n{bar}");
    println!("  {title}");
    println!("{bar}");
}

fn subheader(label: &str) {
    println!("\n  ── {label} ──");
}

/// Parse WAT, render ASCII composition graph, and print to the terminal.
fn print_ascii(label: &str, wat_src: &str, detail: DetailLevel, show_types: bool) {
    subheader(&format!("Composition graph / {label}"));
    let wasm = wat::parse_str(wat_src).expect("WAT parse failed");
    let graph = parse::component::parse_component(&wasm).expect("component parse failed");
    println!(
        "{}",
        output::ascii::generate_ascii(&graph, detail, show_types)
    );
}

/// Parse WAT, render Mermaid, print to the terminal, and write to demo/out/<filename>.
fn save_mermaid(
    label: &str,
    filename: &str,
    wat_src: &str,
    detail: DetailLevel,
    direction: Direction,
    show_types: bool,
) {
    let path = format!("{OUT_DIR}/{filename}");
    subheader(&format!("Mermaid / {label}  →  {path}"));
    let wasm = wat::parse_str(wat_src).expect("WAT parse failed");
    let graph = parse::component::parse_component(&wasm).expect("component parse failed");
    let diagram = output::mermaid::generate_mermaid(&graph, detail, direction, show_types);
    println!("{diagram}");
    std::fs::write(&path, &diagram).unwrap_or_else(|e| panic!("failed to write {path}: {e}"));
}

fn main() {
    std::fs::create_dir_all(OUT_DIR).expect("failed to create demo/out");

    // ──────────────────────────────────────────────────────────────────────────
    // Scenario 01 — Simple Chain
    // ──────────────────────────────────────────────────────────────────────────
    header("Scenario 01 — Simple Chain  (host → $core → $auth → export)");

    print_ascii(
        "handler-chain / types=on",
        WAT_01,
        DetailLevel::HandlerChain,
        true,
    );
    print_ascii("all-interfaces", WAT_01, DetailLevel::AllInterfaces, true);
    print_ascii("full", WAT_01, DetailLevel::Full, true);
    save_mermaid(
        "handler-chain",
        "01-simple-chain-handler-chain.mmd",
        WAT_01,
        DetailLevel::HandlerChain,
        Direction::LeftToRight,
        true,
    );

    // ──────────────────────────────────────────────────────────────────────────
    // Scenario 02 — Three-Layer Stack
    // ──────────────────────────────────────────────────────────────────────────
    header("Scenario 02 — Three-Layer Stack  (host → $core → $auth → $rate → export)");

    print_ascii(
        "handler-chain / types=on",
        WAT_02,
        DetailLevel::HandlerChain,
        true,
    );
    print_ascii("all-interfaces", WAT_02, DetailLevel::AllInterfaces, true);
    print_ascii("full", WAT_02, DetailLevel::Full, true);
    save_mermaid(
        "handler-chain",
        "02-three-layer-stack-handler-chain.mmd",
        WAT_02,
        DetailLevel::HandlerChain,
        Direction::LeftToRight,
        true,
    );

    // ──────────────────────────────────────────────────────────────────────────
    // Scenario 03 — Multi-Chain
    // ──────────────────────────────────────────────────────────────────────────
    header("Scenario 03 — Multi-Chain  (HTTP handler chain + keyvalue/store chain)");

    print_ascii(
        "handler-chain / types=on",
        WAT_03,
        DetailLevel::HandlerChain,
        true,
    );
    print_ascii("all-interfaces", WAT_03, DetailLevel::AllInterfaces, true);
    print_ascii("full", WAT_03, DetailLevel::Full, true);
    save_mermaid(
        "handler-chain / direction=LR",
        "03-multi-chain-handler-chain-lr.mmd",
        WAT_03,
        DetailLevel::HandlerChain,
        Direction::LeftToRight,
        true,
    );
    save_mermaid(
        "handler-chain / direction=TD",
        "03-multi-chain-handler-chain-td.mmd",
        WAT_03,
        DetailLevel::HandlerChain,
        Direction::TopDown,
        true,
    );

    // ──────────────────────────────────────────────────────────────────────────
    // Scenario 04 — Chain + Utility Node
    // ──────────────────────────────────────────────────────────────────────────
    header("Scenario 04 — Chain + Utility Node  ($logger absent from HandlerChain, present in AllInterfaces)");

    print_ascii(
        "handler-chain  ← $logger NOT shown",
        WAT_04,
        DetailLevel::HandlerChain,
        true,
    );
    print_ascii(
        "all-interfaces  ← $logger IS shown",
        WAT_04,
        DetailLevel::AllInterfaces,
        true,
    );
    save_mermaid(
        "handler-chain",
        "04-chain-plus-utility-handler-chain.mmd",
        WAT_04,
        DetailLevel::HandlerChain,
        Direction::LeftToRight,
        true,
    );

    // ──────────────────────────────────────────────────────────────────────────
    // Scenario 05 — Non-HTTP Chain
    // ──────────────────────────────────────────────────────────────────────────
    header("Scenario 05 — Non-HTTP Chain  (wasi:messaging/consumer pipeline)");

    print_ascii(
        "handler-chain / types=on",
        WAT_05,
        DetailLevel::HandlerChain,
        true,
    );
    print_ascii("all-interfaces", WAT_05, DetailLevel::AllInterfaces, true);
    save_mermaid(
        "handler-chain",
        "05-non-http-chain-handler-chain.mmd",
        WAT_05,
        DetailLevel::HandlerChain,
        Direction::LeftToRight,
        true,
    );

    // ──────────────────────────────────────────────────────────────────────────
    // Scenario 06 — Typed Chain
    // ──────────────────────────────────────────────────────────────────────────
    header("Scenario 06 — Typed Chain  (multi-param interface; types on vs off)");

    print_ascii(
        "handler-chain / types=ON  ← key with function signatures",
        WAT_06,
        DetailLevel::HandlerChain,
        true,
    );
    print_ascii(
        "handler-chain / types=OFF  ← no key section",
        WAT_06,
        DetailLevel::HandlerChain,
        false,
    );
    save_mermaid(
        "handler-chain / types=on",
        "06-typed-chain-handler-chain.mmd",
        WAT_06,
        DetailLevel::HandlerChain,
        Direction::LeftToRight,
        true,
    );

    // ──────────────────────────────────────────────────────────────────────────
    // Summary
    // ──────────────────────────────────────────────────────────────────────────
    println!();
    let bar = "═".repeat(60);
    println!("{bar}");
    println!("  Mermaid diagrams written to {OUT_DIR}/");
    let mut files: Vec<_> = std::fs::read_dir(OUT_DIR)
        .expect("demo/out missing")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "mmd"))
        .map(|e| Path::new(OUT_DIR).join(e.file_name()).display().to_string())
        .collect();
    files.sort();
    for f in &files {
        println!("    {f}");
    }
    println!();
    println!("  Render:        mmdc -i demo/out/<file>.mmd -o demo/out/<file>.svg");
    println!("  Or paste into: https://mermaid.live");
    println!("{bar}");
}

// ─── Tests ────────────────────────────────────────────────────────────────
// Run with: cargo test --example demo   (or cargo test --all-targets)

fn parse_wat(src: &str) -> cviz::model::CompositionGraph {
    let wasm = wat::parse_str(src).expect("WAT parse failed");
    parse::component::parse_component(&wasm).expect("component parse failed")
}

#[test]
fn test_01_simple_chain_parses_with_two_nodes() {
    let graph = parse_wat(WAT_01);
    assert_eq!(graph.nodes.len(), 2, "expected 2 nodes");
    let names: Vec<_> = graph.nodes.values().map(|n| n.display_label()).collect();
    assert!(names.iter().any(|n| *n == "core"), "expected 'core' node");
    assert!(names.iter().any(|n| *n == "auth"), "expected 'auth' node");
    assert!(
        graph.component_exports.contains_key("wasi:http/handler@0.3.0"),
        "expected handler export"
    );
}

#[test]
fn test_01_ascii_output_contains_node_names() {
    let graph = parse_wat(WAT_01);
    let ascii = output::ascii::generate_ascii(&graph, DetailLevel::HandlerChain, true);
    assert!(ascii.contains("core"), "ASCII should contain 'core'");
    assert!(ascii.contains("auth"), "ASCII should contain 'auth'");
}

#[test]
fn test_01_mermaid_output_is_non_empty() {
    let graph = parse_wat(WAT_01);
    let mermaid =
        output::mermaid::generate_mermaid(&graph, DetailLevel::HandlerChain, Direction::LeftToRight, true);
    assert!(!mermaid.is_empty(), "Mermaid output should be non-empty");
    assert!(mermaid.contains("core"), "Mermaid should contain 'core'");
}

#[test]
fn test_02_three_layer_stack_has_three_nodes() {
    let graph = parse_wat(WAT_02);
    assert_eq!(graph.nodes.len(), 3, "expected 3 nodes");
    let names: Vec<_> = graph.nodes.values().map(|n| n.display_label()).collect();
    assert!(names.iter().any(|n| *n == "core"));
    assert!(names.iter().any(|n| *n == "auth"));
    assert!(names.iter().any(|n| *n == "rate"));
}

#[test]
fn test_03_multi_chain_has_four_nodes() {
    let graph = parse_wat(WAT_03);
    assert_eq!(graph.nodes.len(), 4, "expected 4 nodes (2 HTTP + 2 KV)");
    let names: Vec<_> = graph.nodes.values().map(|n| n.display_label()).collect();
    assert!(names.iter().any(|n| *n == "http-core"));
    assert!(names.iter().any(|n| *n == "http-auth"));
    assert!(names.iter().any(|n| *n == "kv-store"));
    assert!(names.iter().any(|n| *n == "kv-cache"));
}

#[test]
fn test_04_chain_plus_utility_logger_visible_in_all_interfaces() {
    let graph = parse_wat(WAT_04);
    // HandlerChain excludes the logger utility node
    let ascii_hc = output::ascii::generate_ascii(&graph, DetailLevel::HandlerChain, true);
    // AllInterfaces includes the logger
    let ascii_all = output::ascii::generate_ascii(&graph, DetailLevel::AllInterfaces, true);
    assert!(
        ascii_all.contains("logger"),
        "AllInterfaces should show 'logger'"
    );
    // The handler chain still contains core and auth
    assert!(ascii_hc.contains("core"));
    assert!(ascii_hc.contains("auth"));
}

#[test]
fn test_05_non_http_chain_uses_messaging_interface() {
    let graph = parse_wat(WAT_05);
    assert_eq!(graph.nodes.len(), 2, "expected 2 nodes");
    let names: Vec<_> = graph.nodes.values().map(|n| n.display_label()).collect();
    assert!(names.iter().any(|n| *n == "consumer"));
    assert!(names.iter().any(|n| *n == "filter"));
    assert!(
        graph
            .component_exports
            .contains_key("wasi:messaging/consumer@0.2.0"),
        "expected messaging export"
    );
}

#[test]
fn test_06_typed_chain_export_has_fingerprint() {
    let graph = parse_wat(WAT_06);
    let fp = graph
        .component_exports
        .get("wasi:http/handler@0.3.0")
        .and_then(|e| e.fingerprint.as_ref());
    assert!(fp.is_some(), "typed chain export should have a fingerprint");
}

#[test]
fn test_06_typed_chain_types_on_shows_signatures() {
    let graph = parse_wat(WAT_06);
    let with_types =
        output::ascii::generate_ascii(&graph, DetailLevel::HandlerChain, true);
    let without_types =
        output::ascii::generate_ascii(&graph, DetailLevel::HandlerChain, false);
    // types=on should include more content (the type key section)
    assert!(
        with_types.len() > without_types.len(),
        "types=on output should be longer than types=off"
    );
}
