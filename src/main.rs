use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use cviz::output;
use cviz::output::graph::GraphRenderOpts;
use cviz::output::{Direction, OutputFormat};
use cviz::{HighlightColor, Highlights, Selection};

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

    /// Show only middleware-chain interfaces — those exported by the
    /// composition AND re-imported by another component instance.
    #[arg(long = "chain-only", action = clap::ArgAction::SetTrue)]
    chain_only: bool,

    /// Substring match on the fully-qualified interface name.  Applied
    /// after `--chain-only`.  Useful for narrowing to one namespace
    /// (e.g. `--filter wasi:http`).
    #[arg(long)]
    filter: Option<String>,

    /// Render host imports as dashed branches into each importing node
    /// (mermaid) or as a per-block "Host imports:" footer (ASCII).
    #[arg(long = "host-imports", action = clap::ArgAction::SetTrue)]
    host_imports: bool,

    /// Hide WIT type information on interface connections.
    #[arg(long = "no-types", action = clap::ArgAction::SetTrue)]
    no_types: bool,

    /// Output file (stdout if not specified)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Highlight a node or edge in the graph view.  Repeatable.  Format:
    ///
    ///   node:<id>[=<context>][>><color>]
    ///   edge:<id>[=<context>][>><color>]
    ///
    /// Examples:
    ///   --highlight node:srv
    ///   --highlight 'node:srv=outdated'
    ///   --highlight 'edge:wasi:http/handler@0.3.0::middleware->srv=drained>>orange'
    ///
    /// Colors: yellow (default), cyan, magenta, blue, orange, red,
    /// green, white.  (Red and green together are confusable for ~5% of
    /// readers — pick distinct hues when designing for a colorblind
    /// audience.)
    #[arg(long = "highlight", value_name = "SPEC", action = clap::ArgAction::Append)]
    highlight: Vec<String>,

    /// Force ANSI color emission (otherwise auto-detected from the stdout
    /// TTY).  Has no effect when `-o` is set or when the format isn't ASCII.
    #[arg(long, default_value = "auto")]
    color: ColorMode,
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

    let graph = cviz::parse::component::parse_component(&bytes)
        .with_context(|| format!("Failed to parse component: {}", args.file.display()))?;

    let mut highlights = Highlights::default();
    let mut ctx_to_tag: BTreeMap<String, u32> = BTreeMap::new();
    for spec in &args.highlight {
        parse_highlight_spec(spec, &mut highlights, &mut ctx_to_tag)
            .with_context(|| format!("Invalid --highlight value: {spec}"))?;
    }
    let highlights = if args.highlight.is_empty() {
        None
    } else {
        Some(highlights)
    };

    // ANSI color is meaningful only when ASCII output goes to a real TTY.
    // Forced "always" still emits even when piping (useful for CI logs).
    let use_color = match args.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => args.output.is_none() && std::io::stdout().is_terminal(),
    };

    let show_types = !args.no_types;
    let opts = GraphRenderOpts {
        chain_only: args.chain_only,
        filter: args.filter.clone(),
        show_host_imports: args.host_imports,
    };
    let mut condensed = false;
    let mut unmatched_ids: Vec<String> = Vec::new();
    let diagram = match args.format {
        OutputFormat::Ascii => {
            let max_w = terminal_columns();
            let out = output::graph::generate_graph_ascii(
                &graph,
                &opts,
                show_types,
                max_w,
                highlights.as_ref(),
                use_color,
            );
            condensed = out.condensed;
            unmatched_ids = out.unmatched_highlight_ids;
            out.ascii
        }
        OutputFormat::Mermaid => output::mermaid::generate_mermaid(
            &graph,
            &opts,
            args.direction,
            show_types,
            highlights.as_ref(),
        ),
        OutputFormat::Json => output::json::generate_json(&graph, false)?,
        OutputFormat::JsonPretty => output::json::generate_json(&graph, true)?,
    };

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
    if !unmatched_ids.is_empty() {
        let stderr_is_tty = std::io::stderr().is_terminal();
        let force_color = matches!(args.color, ColorMode::Always);
        let no_color = matches!(args.color, ColorMode::Never);
        let warn_color = !no_color && (force_color || stderr_is_tty);
        // Bold + 256-color orange for the warning — same colorblind-safe
        // palette as the diagram highlights, easy to spot under the
        // rendered output where this surface lands.
        let (bold_warn, reset) = if warn_color {
            ("\x1b[1;38;5;208m", "\x1b[0m")
        } else {
            ("", "")
        };
        eprintln!();
        eprintln!(
            "{bold_warn}!! warning: these --highlight ids did not match any node or edge:{reset}"
        );
        for id in unmatched_ids {
            eprintln!("{bold_warn}  - {id}{reset}");
        }
        eprintln!(
            "   (canonical edge ids look like `<interface>::<caller>-><provider>` — \
             try `cviz <file> --format json` to inspect available ids)"
        );
    }

    Ok(())
}

/// Parse one `--highlight` value of the form
/// `node:<id>[=<ctx>][>><color>]` or `edge:<id>[=<ctx>][>><color>]` and
/// register it into `out`.
fn parse_highlight_spec(
    spec: &str,
    out: &mut Highlights,
    ctx_to_tag: &mut BTreeMap<String, u32>,
) -> Result<()> {
    let (without_color, color) = split_color_suffix(spec)?;
    let (kind_id, ctx) = match without_color.split_once('=') {
        Some((k, c)) => (k.to_string(), Some(c.to_string())),
        None => (without_color, None),
    };

    let mut sel: Selection = kind_id.parse().map_err(|e| anyhow!("{e}"))?;
    if let Some(ctx) = ctx {
        let tag_id = match ctx_to_tag.get(&ctx) {
            Some(&existing) => existing,
            None => {
                let next = ctx_to_tag.len() as u32;
                out.register_tag(next, ctx.clone())
                    .map_err(|e| anyhow!("{e}"))?;
                ctx_to_tag.insert(ctx, next);
                next
            }
        };
        sel = sel.tag(tag_id);
    }
    if let Some(col) = color {
        sel = sel.color(col);
    }
    out.mark(sel);
    Ok(())
}

/// Look for a `>><color>` suffix at the very end of `rest`.
fn split_color_suffix(rest: &str) -> Result<(String, Option<HighlightColor>)> {
    match rest.rsplit_once(">>") {
        Some((prefix, suffix)) => {
            let color: HighlightColor = suffix.parse().map_err(|e| anyhow!("{e}"))?;
            Ok((prefix.to_string(), Some(color)))
        }
        None => Ok((rest.to_string(), None)),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Convenience: run a single spec through the parser starting from an
    /// empty Highlights / tag map.  Most tests don't need state across
    /// multiple specs.
    fn parse_one(spec: &str) -> Result<Highlights> {
        let mut h = Highlights::default();
        let mut map = BTreeMap::new();
        parse_highlight_spec(spec, &mut h, &mut map)?;
        Ok(h)
    }

    #[test]
    fn parse_highlight_node_basic() {
        let h = parse_one("node:srv").unwrap();
        assert!(h.is_node_highlighted("srv"));
        assert!(h.node_tag_ids("srv").is_empty());
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
    }

    #[test]
    fn parse_highlight_node_with_context() {
        let h = parse_one("node:srv=outdated").unwrap();
        assert_eq!(h.node_tag_ids("srv"), vec![0]);
        assert_eq!(h.tag_lines(), vec!["0 outdated".to_string()]);
    }

    #[test]
    fn parse_highlight_node_with_color() {
        let h = parse_one("node:srv>>orange").unwrap();
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Orange));
    }

    #[test]
    fn parse_highlight_node_full() {
        let h = parse_one("node:srv=outdated>>cyan").unwrap();
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Cyan));
        assert_eq!(h.node_tag_ids("srv"), vec![0]);
    }

    #[test]
    fn parse_highlight_edge_with_canonical_id() {
        let h = parse_one("edge:wasi:http/handler@0.3.0::middleware->srv=drained").unwrap();
        // The `@0.3.0` inside the id is no longer load-bearing on the
        // color-suffix detection (`>>` is the delimiter now), but this
        // test still guards that a canonical edge id parses cleanly.
        assert!(h.is_edge_highlighted("wasi:http/handler@0.3.0::middleware->srv"));
        assert_eq!(
            h.edge_tag_ids("wasi:http/handler@0.3.0::middleware->srv"),
            vec![0]
        );
    }

    #[test]
    fn parse_highlight_edge_with_color_and_canonical_id() {
        let h = parse_one("edge:wasi:http/handler@0.3.0::middleware->srv=drained>>orange").unwrap();
        assert_eq!(
            h.edge_color("wasi:http/handler@0.3.0::middleware->srv"),
            Some(HighlightColor::Orange)
        );
    }

    #[test]
    fn parse_highlight_at_in_context_needs_no_escape() {
        // `@` is no longer a color delimiter, so a context containing `@`
        // just works — no backslash escape needed.
        let h = parse_one("node:srv=tag@v2").unwrap();
        assert_eq!(h.tag_lines(), vec!["0 tag@v2".to_string()]);
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
    }

    #[test]
    fn parse_highlight_repeated_ctx_reuses_tag_id() {
        // First spec assigns tag 0 to "drained"; second spec mentions the
        // same context and should reuse 0 rather than mint 1.
        let mut h = Highlights::default();
        let mut map = BTreeMap::new();
        parse_highlight_spec("node:srv=drained", &mut h, &mut map).unwrap();
        parse_highlight_spec("edge:e1::a->b=drained", &mut h, &mut map).unwrap();
        assert_eq!(h.node_tag_ids("srv"), vec![0]);
        assert_eq!(h.edge_tag_ids("e1::a->b"), vec![0]);
        assert_eq!(h.tag_lines(), vec!["0 drained".to_string()]);
    }

    #[test]
    fn parse_highlight_distinct_ctxs_get_distinct_ids() {
        let mut h = Highlights::default();
        let mut map = BTreeMap::new();
        parse_highlight_spec("node:srv=outdated", &mut h, &mut map).unwrap();
        parse_highlight_spec("edge:e1::a->b=drained", &mut h, &mut map).unwrap();
        assert_eq!(h.node_tag_ids("srv"), vec![0]);
        assert_eq!(h.edge_tag_ids("e1::a->b"), vec![1]);
    }

    #[test]
    fn parse_highlight_rejects_bad_kind() {
        assert!(parse_one("nope:srv").is_err());
    }

    #[test]
    fn parse_highlight_rejects_empty_id() {
        assert!(parse_one("node:").is_err());
    }

    #[test]
    fn parse_highlight_rejects_bad_color() {
        assert!(parse_one("node:srv>>chartreuse").is_err());
    }
}
