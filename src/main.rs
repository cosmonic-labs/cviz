use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use cviz::output;
use cviz::output::{DetailLevel, Direction, OutputFormat};

#[derive(Parser, Debug)]
#[command(name = "cviz")]
#[command(about = "Visualize WebAssembly component composition")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("CVIZ_GIT_SHA"), ") with wasmparser ", env!("WASMPARSER_VERSION")))]
struct Args {
    /// Path to the .wasm component file
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "ascii", value_enum)]
    format: OutputFormat,

    /// Diagram direction (mermaid only)
    #[arg(short, long, default_value = "lr", value_enum)]
    direction: Direction,

    /// Detail level
    #[arg(short = 'l', long, default_value = "handler-chain", value_enum)]
    detail: DetailLevel,

    /// Show WIT type information on interface connections
    #[arg(short = 't', long, default_value = "true")]
    types: bool,

    /// Output file (stdout if not specified)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Read the component file
    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

    // Parse the component
    let graph = cviz::parse::component::parse_component(&bytes)
        .with_context(|| format!("Failed to parse component: {}", args.file.display()))?;

    // Generate the diagram based on format.  The Graph detail level under
    // Ascii uses a richer entry point that can report when it had to condense
    // the layout to fit the terminal.
    let mut condensed = false;
    let diagram = match args.format {
        OutputFormat::Ascii if matches!(args.detail, DetailLevel::Graph) => {
            let max_w = terminal_columns();
            let out = output::graph::generate_graph_ascii(&graph, args.types, max_w);
            condensed = out.condensed;
            out.ascii
        }
        OutputFormat::Ascii => output::ascii::generate_ascii(&graph, args.detail, args.types),
        OutputFormat::Mermaid => {
            output::mermaid::generate_mermaid(&graph, args.detail, args.direction, args.types)
        }
        OutputFormat::Json => output::json::generate_json(&graph, false)?, // always generates the full graph
        OutputFormat::JsonPretty => output::json::generate_json(&graph, true)?, // always generates the full graph
    };

    // Output
    if let Some(output_path) = args.output {
        std::fs::write(&output_path, &diagram)
            .with_context(|| format!("Failed to write output: {}", output_path.display()))?;
        eprintln!("Diagram written to: {}", output_path.display());
    } else {
        println!("{}", diagram);
    }

    if condensed {
        eprintln!();
        eprintln!(
            "note: the diagram was condensed to fit; rerun with `-f mermaid` for a wider view."
        );
    }

    Ok(())
}

/// Detect the terminal column count.  Prefers the OS ioctl (works even when
/// `$COLUMNS` isn't exported into the child process, which is the default in
/// most shells); falls back to `$COLUMNS` if the ioctl says we're not on a
/// terminal (e.g. when piping output to a file).
fn terminal_columns() -> Option<usize> {
    if let Some((terminal_size::Width(w), _)) = terminal_size::terminal_size() {
        if w > 0 {
            return Some(w as usize);
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&w| w > 0)
}
