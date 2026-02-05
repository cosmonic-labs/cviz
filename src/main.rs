mod ascii;
mod mermaid;
mod model;
mod output;
mod parser;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use output::{DetailLevel, Direction, OutputFormat};

#[derive(Parser, Debug)]
#[command(name = "cviz")]
#[command(about = "Visualize WebAssembly component composition")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("CVIZ_GIT_SHA"), ") with wasmparser ", env!("WASMPARSER_VERSION")))]
struct Args {
    /// Path to the .wasm component file
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "ascii", value_parser = parse_format)]
    format: OutputFormat,

    /// Diagram direction (mermaid only)
    #[arg(short, long, default_value = "lr", value_parser = parse_direction)]
    direction: Direction,

    /// Detail level
    #[arg(short = 'l', long, default_value = "handler-chain", value_parser = parse_detail)]
    detail: DetailLevel,

    /// Output file (stdout if not specified)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn parse_format(s: &str) -> Result<OutputFormat, String> {
    s.parse()
}

fn parse_direction(s: &str) -> Result<Direction, String> {
    s.parse()
}

fn parse_detail(s: &str) -> Result<DetailLevel, String> {
    s.parse()
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Read the component file
    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

    // Parse the component
    let graph = parser::parse_component(&bytes)
        .with_context(|| format!("Failed to parse component: {}", args.file.display()))?;

    // Generate the diagram based on format
    let diagram = match args.format {
        OutputFormat::Ascii => ascii::generate_ascii(&graph, args.detail),
        OutputFormat::Mermaid => mermaid::generate_mermaid(&graph, args.detail, args.direction),
    };

    // Output
    if let Some(output_path) = args.output {
        std::fs::write(&output_path, &diagram)
            .with_context(|| format!("Failed to write output: {}", output_path.display()))?;
        eprintln!("Diagram written to: {}", output_path.display());
    } else {
        println!("{}", diagram);
    }

    Ok(())
}
