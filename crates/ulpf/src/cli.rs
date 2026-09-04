use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};

use crate::engine;

#[derive(Parser)]
#[command(name = "ulpf", version, about = "Universal Log Pre-processing Framework")]
pub struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Process files or directories end to end: raw store, parse, normalize, JSON Lines.
    Run {
        /// Files or directories (scanned recursively).
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Raw store directory (created if missing; only ever appended to).
        #[arg(long, default_value = "ulpf.ulpf-store")]
        store: PathBuf,
        /// JSON Lines output file, or `-` for stdout.
        #[arg(long, short, default_value = "out.jsonl")]
        output: PathBuf,
        #[arg(long, default_value = "parsers")]
        parsers: PathBuf,
        #[arg(long, default_value = "mappings")]
        mappings: PathBuf,
        /// Mapping schema name (default: the first loaded).
        #[arg(long)]
        schema: Option<String>,
        /// Worker threads (default: available cores minus one).
        #[arg(long, short = 'j')]
        threads: Option<usize>,
        /// Offset applied to timestamps that carry no zone: `+05:30`, `-0800`, `Z`, or seconds.
        #[arg(long, default_value = "Z")]
        tz: String,
        #[arg(long, default_value_t = 1024)]
        batch: usize,
        /// Bounded queue capacity in batches; when full, ingest blocks.
        #[arg(long, default_value_t = 64)]
        queue: usize,
        /// Also write the counter report as JSON to this path.
        #[arg(long)]
        report_json: Option<PathBuf>,
    },
    /// Load every parser and mapping file and report problems with path and line.
    Check {
        #[arg(long, default_value = "parsers")]
        parsers: PathBuf,
        #[arg(long, default_value = "mappings")]
        mappings: PathBuf,
    },
    /// Recompute every digest in a raw store.
    Verify {
        #[arg(long, default_value = "ulpf.ulpf-store")]
        store: PathBuf,
    },
    /// Print one raw record's exact bytes to stdout (header on stderr).
    Raw {
        id: u64,
        #[arg(long, default_value = "ulpf.ulpf-store")]
        store: PathBuf,
    },
    /// Emit fixture skeleton lines for a sample file (review each line before committing).
    Fixture {
        sample: PathBuf,
        #[arg(long, default_value = "parsers")]
        parsers: PathBuf,
        #[arg(long, default_value = "mappings")]
        mappings: PathBuf,
        #[arg(long, default_value = "Z")]
        tz: String,
    },
}

pub fn parse_tz(s: &str) -> Result<i32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("z") || s.eq_ignore_ascii_case("utc") {
        return Ok(0);
    }
    if let Ok(n) = s.parse::<i32>()
        && !s.starts_with('+')
        && s.len() != 5
    {
        return Ok(n);
    }
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'+') => (1, &s[1..]),
        Some(b'-') => (-1, &s[1..]),
        _ => bail!("timezone must be Z, +HH:MM, -HHMM or seconds, got `{s}`"),
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    anyhow::ensure!(digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()), "timezone must be Z, +HH:MM, -HHMM or seconds, got `{s}`");
    let h: i32 = digits[..2].parse()?;
    let m: i32 = digits[2..].parse()?;
    Ok(sign * (h * 3600 + m * 60))
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { inputs, store, output, parsers, mappings, schema, threads, tz, batch, queue, report_json } => {
            let threads = threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(1));
            let cfg = engine::Config {
                inputs,
                store,
                output,
                parsers,
                mappings,
                schema,
                threads,
                default_offset_secs: parse_tz(&tz)?,
                batch_events: batch,
                queue_batches: queue,
            };
            let report = engine::run(&cfg)?;
            let mut err = std::io::stderr().lock();
            for p in &report.load_problems {
                writeln!(err, "load problem: {p}")?;
            }
            for p in &report.input_problems {
                writeln!(err, "input problem: {p}")?;
            }
            writeln!(err, "definitions: {} parsers loaded, {} file problems", report.parsers_loaded, report.load_problems.len())?;
            writeln!(err, "{}", report.snapshot)?;
            if let Some(path) = report_json {
                std::fs::write(&path, serde_json::to_string_pretty(&report.snapshot)?).with_context(|| format!("writing {}", path.display()))?;
            }
            Ok(())
        }
        Cmd::Check { parsers, mappings } => {
            let p = ulpf_parse::load_dir(&parsers).with_context(|| format!("parsers directory {}", parsers.display()))?;
            let m = ulpf_normalize::load_dir(&mappings).with_context(|| format!("mappings directory {}", mappings.display()))?;
            let mut out = std::io::stdout().lock();
            for parser in &p.parsers {
                let d = parser.definition();
                writeln!(out, "parser  {:<24} {} {} ({} subs, {} timestamp candidates)", d.parser.name, d.parser.vendor, d.parser.product, d.sub.len(), d.timestamp.len())?;
            }
            for map in &m.mappings {
                let f = map.file();
                writeln!(out, "mapping {:<24} {} fields, {} enums, {} classes", f.schema.name, f.fields.len(), f.enums.len(), f.class.len())?;
            }
            let problems = p.errors.len() + m.errors.len();
            for e in &p.errors {
                writeln!(out, "ERROR parser  {e}")?;
            }
            for e in &m.errors {
                writeln!(out, "ERROR mapping {e}")?;
            }
            writeln!(out, "{} parsers, {} mappings loaded; {} problems", p.parsers.len(), m.mappings.len(), problems)?;
            if problems > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Verify { store } => {
            let reader = ulpf_store::RawReader::open(&store).with_context(|| format!("opening store {}", store.display()))?;
            let report = reader.verify();
            println!("verified {} records, {} corrupt", report.checked, report.corrupt.len());
            for id in report.corrupt.iter().take(20) {
                println!("corrupt: raw id {}", id.0);
            }
            if !report.corrupt.is_empty() {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Raw { id, store } => {
            let reader = ulpf_store::RawReader::open(&store).with_context(|| format!("opening store {}", store.display()))?;
            let rec = reader.get(ulpf_store::RawId(id)).with_context(|| format!("no record with id {id} (store holds {})", reader.len()))?;
            let names = reader.source_names().unwrap_or_default();
            let mut rfc = String::new();
            ulpf_time::format_rfc3339(rec.receipt_nanos, &mut rfc);
            eprintln!(
                "raw id {}  source {}  received {}  {} bytes  sha256 {}",
                rec.id.0,
                names.get(&rec.source).map(String::as_str).unwrap_or("?"),
                rfc,
                rec.bytes.len(),
                rec.sha256.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            std::io::stdout().lock().write_all(rec.bytes)?;
            Ok(())
        }
        Cmd::Fixture { sample, parsers, mappings, tz } => {
            let (pipeline, problems) = crate::pipeline::Pipeline::load(&parsers, &mappings, None, parse_tz(&tz)?)?;
            for p in problems {
                eprintln!("load problem: {p}");
            }
            let bytes = std::fs::read(&sample).with_context(|| format!("reading {}", sample.display()))?;
            let name = sample.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mut scratch = pipeline.registry.scratch();
            let mut out = std::io::stdout().lock();
            for (i, range) in ulpf_store::Framer::new(&bytes, true).enumerate() {
                let line = crate::fixture::skeleton(&pipeline, &bytes[range], i as u64, &name, &mut scratch)?;
                writeln!(out, "{line}")?;
            }
            Ok(())
        }
    }
}
