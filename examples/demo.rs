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
