use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use cviz::output;
use cviz::output::{DetailLevel, Direction, OutputFormat};
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

    /// Detail level
    #[arg(short = 'l', long, default_value = "handler-chain", value_enum)]
    detail: DetailLevel,

    /// Hide WIT type information on interface connections.
    #[arg(long = "no-types", action = clap::ArgAction::SetTrue)]
    no_types: bool,

    /// Output file (stdout if not specified)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Highlight a node or edge in the graph view.  Repeatable.  Format:
    ///
    ///   node:<id>[=<context>][@<color>]
    ///   edge:<id>[=<context>][@<color>]
    ///
    /// Examples:
    ///   --highlight node:srv
    ///   --highlight 'node:srv=outdated'
    ///   --highlight 'edge:wasi:http/handler@0.3.0::middleware->srv=drained@orange'
    ///
    /// Colors: yellow (default), cyan, magenta, blue, orange, white.
    /// Only the `graph` detail level renders highlights.
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

    let mut highlights = Highlights::new();
    // CLI-side ctx→tag-id map so consecutive `--highlight` flags that
    // mention the same context string reuse the same tag number rather
    // than each getting their own.  Auto-assigned in insertion order
    // (first new ctx → 0, next → 1, …) so the in-diagram brackets line
    // up with the Tags list under the rendering.
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
    let mut condensed = false;
    let mut unmatched_ids: Vec<String> = Vec::new();
    let diagram = match args.format {
        OutputFormat::Ascii if matches!(args.detail, DetailLevel::Graph) => {
            let max_w = terminal_columns();
            let out = output::graph::generate_graph_ascii(
                &graph,
                show_types,
                max_w,
                highlights.as_ref(),
                use_color,
            );
            condensed = out.condensed;
            unmatched_ids = out.unmatched_highlight_ids;
            out.ascii
        }
        OutputFormat::Ascii => output::ascii::generate_ascii(&graph, args.detail, show_types),
        OutputFormat::Mermaid => output::mermaid::generate_mermaid(
            &graph,
            args.detail,
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
/// `node:<id>[=<ctx>][@<color>]` or `edge:<id>[=<ctx>][@<color>]` and
/// register it into `out`.
///
/// `<id>` may contain `::`, `->`, etc. (canonical edge IDs do); the
/// parser splits on the **first** `:` for the kind and recognises the
/// **last unescaped** `@<color>` as the optional trailing color override.
fn parse_highlight_spec(
    spec: &str,
    out: &mut Highlights,
    ctx_to_tag: &mut BTreeMap<String, u32>,
) -> Result<()> {
    let (kind, rest) = spec
        .split_once(':')
        .ok_or_else(|| anyhow!("missing `kind:` prefix; expected `node:` or `edge:`"))?;

    // Detect an optional `@<color>` color override at the very end.
    //
    // Canonical edge IDs already contain `@` inside the interface version
    // (`wasi:http/handler@0.3.0::...`), so a naive "split on the last `@`"
    // would munch the version.  Instead we only treat the trailing chunk
    // as a color suffix when it's a pure ASCII-letter word.  The version
    // tail (digits + dots + `::` + arrow) never matches that, so it's
    // safely left alone.  A user who wants `@` in a context value can
    // backslash-escape it (`\@`).
    let (id_and_ctx_raw, color) = split_color_suffix(rest)?;
    let id_and_ctx = id_and_ctx_raw.replace("\\@", "@");

    let (id, ctx) = match id_and_ctx.split_once('=') {
        Some((id, ctx)) => (id.to_string(), Some(ctx.to_string())),
        None => (id_and_ctx, None),
    };
    if id.is_empty() {
        return Err(anyhow!("id is empty"));
    }

    let mut sel = match kind {
        "node" => Selection::node(id),
        "edge" => Selection::edge(id),
        k => return Err(anyhow!("unknown kind `{k}`; expected `node` or `edge`")),
    };
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

/// Look for an `@<color>` suffix at the very end of `rest`.
///
/// Returns `(prefix, Some(color))` when the trailing `@<word>` is
/// composed of ASCII letters only and parses as a known color name.
/// Returns `(rest_to_string, None)` when:
/// - there's no `@` at all, or
/// - the trailing `@` is backslash-escaped (`\@`), or
/// - the trailing chunk after `@` contains non-letters (e.g. the version
///   tail of a canonical edge id like `wasi:http/handler@0.3.0::...`).
///
/// Errors only when the trailing chunk is letter-only but isn't a valid
/// color — that's the "user typo" path.
fn split_color_suffix(rest: &str) -> Result<(String, Option<HighlightColor>)> {
    let bytes = rest.as_bytes();
    let Some(at_idx) = rest.rfind('@') else {
        return Ok((rest.to_string(), None));
    };
    // Backslash-escaped → not a color suffix.
    let preceding_backslashes = bytes[..at_idx]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\\')
        .count();
    if preceding_backslashes % 2 == 1 {
        return Ok((rest.to_string(), None));
    }
    let suffix = &rest[at_idx + 1..];
    let is_letter_word = !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphabetic());
    if !is_letter_word {
        // Looks like part of an id (e.g. interface version), leave it alone.
        return Ok((rest.to_string(), None));
    }
    let color = parse_color(suffix)?;
    Ok((rest[..at_idx].to_string(), Some(color)))
}

fn parse_color(s: &str) -> Result<HighlightColor> {
    match s.to_ascii_lowercase().as_str() {
        "yellow" => Ok(HighlightColor::Yellow),
        "cyan" => Ok(HighlightColor::Cyan),
        "magenta" => Ok(HighlightColor::Magenta),
        "blue" => Ok(HighlightColor::Blue),
        "orange" => Ok(HighlightColor::Orange),
        "white" => Ok(HighlightColor::White),
        other => Err(anyhow!(
            "unknown color `{other}`; valid: yellow, cyan, magenta, blue, orange, white"
        )),
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
        let mut h = Highlights::new();
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
        let h = parse_one("node:srv@orange").unwrap();
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Orange));
    }

    #[test]
    fn parse_highlight_node_full() {
        let h = parse_one("node:srv=outdated@cyan").unwrap();
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Cyan));
        assert_eq!(h.node_tag_ids("srv"), vec![0]);
    }

    #[test]
    fn parse_highlight_edge_with_canonical_id() {
        let h = parse_one("edge:wasi:http/handler@0.3.0::middleware->srv=drained").unwrap();
        // The `@0.3.0` should NOT be parsed as a color — it's part of the id.
        assert!(h.is_edge_highlighted("wasi:http/handler@0.3.0::middleware->srv"));
        assert_eq!(
            h.edge_tag_ids("wasi:http/handler@0.3.0::middleware->srv"),
            vec![0]
        );
    }

    #[test]
    fn parse_highlight_edge_with_color_and_canonical_id() {
        let h = parse_one("edge:wasi:http/handler@0.3.0::middleware->srv=drained@orange").unwrap();
        assert_eq!(
            h.edge_color("wasi:http/handler@0.3.0::middleware->srv"),
            Some(HighlightColor::Orange)
        );
    }

    #[test]
    fn parse_highlight_escaped_at_in_context() {
        // Backslash escapes `@` so it ends up in the context.
        let h = parse_one("node:srv=tag\\@v2").unwrap();
        assert_eq!(h.tag_lines(), vec!["0 tag@v2".to_string()]);
        assert_eq!(h.node_color("srv"), Some(HighlightColor::Yellow));
    }

    #[test]
    fn parse_highlight_repeated_ctx_reuses_tag_id() {
        // First spec assigns tag 0 to "drained"; second spec mentions the
        // same context and should reuse 0 rather than mint 1.
        let mut h = Highlights::new();
        let mut map = BTreeMap::new();
        parse_highlight_spec("node:srv=drained", &mut h, &mut map).unwrap();
        parse_highlight_spec("edge:e1::a->b=drained", &mut h, &mut map).unwrap();
        assert_eq!(h.node_tag_ids("srv"), vec![0]);
        assert_eq!(h.edge_tag_ids("e1::a->b"), vec![0]);
        assert_eq!(h.tag_lines(), vec!["0 drained".to_string()]);
    }

    #[test]
    fn parse_highlight_distinct_ctxs_get_distinct_ids() {
        let mut h = Highlights::new();
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
        assert!(parse_one("node:srv@chartreuse").is_err());
    }
}
