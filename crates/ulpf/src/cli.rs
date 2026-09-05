use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};

use crate::engine;

#[derive(Parser)]
#[command(name = "ulpf", version, about = "Universal Log Pre-processing Framework")]
pub struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Options `run` and `serve` share: where things are and how the engine is sized.
#[derive(Args, Clone)]
struct EngineArgs {
    /// Raw store directory (created if missing; only ever appended to).
    #[arg(long, default_value = "ulpf.ulpf-store")]
    store: PathBuf,
    /// JSON Lines output file (appended), or `-` for stdout.
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
    /// Directory where parser proposals for unknown formats are written for review.
    #[arg(long, default_value = "pending")]
    pending: PathBuf,
    /// Unknown lines a source needs before its first proposal; 0 disables inference.
    #[arg(long, default_value_t = 64)]
    infer_threshold: usize,
}

#[derive(Subcommand)]
enum Cmd {
    /// Process files or directories end to end: raw store, parse, normalize, JSON Lines.
    Run {
        /// Files or directories (scanned recursively).
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        #[command(flatten)]
        engine: EngineArgs,
        /// Also write the counter report as JSON to this path.
        #[arg(long)]
        report_json: Option<PathBuf>,
    },
    /// Watch directories for new or growing files, serve the API and UI on localhost.
    Serve {
        /// Directories (or files) to tail.
        #[arg(required = true)]
        watch: Vec<PathBuf>,
        #[command(flatten)]
        engine: EngineArgs,
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        /// Emitted lines kept for the live tail.
        #[arg(long, default_value_t = 1000)]
        tail: usize,
        /// Serve the UI from this directory instead of the embedded copy (restyle without a rebuild).
        #[arg(long)]
        ui_dir: Option<PathBuf>,
        /// Directory poll interval in milliseconds.
        #[arg(long, default_value_t = 250)]
        poll_ms: u64,
    },
    /// Run inference over one file as if no parser covered it; write the proposal to the pending directory.
    Infer {
        file: PathBuf,
        #[arg(long, default_value = "pending")]
        pending: PathBuf,
        /// Skip lines an existing parser already detects.
        #[arg(long, default_value = "parsers")]
        parsers: PathBuf,
        /// Print the decision log.
        #[arg(long)]
        decisions: bool,
    },
    /// Load every parser and mapping file and report problems with path and line.
    Check {
        #[arg(long, default_value = "parsers")]
        parsers: PathBuf,
        #[arg(long, default_value = "mappings")]
        mappings: PathBuf,
        /// Also validate every pending proposal.
        #[arg(long)]
        pending: Option<PathBuf>,
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
    /// Print one entity's timeline from the index beside the output, or rebuild that index.
    Pivot {
        /// src_ip, dst_ip, user, dst_port or device.
        kind: Option<String>,
        value: Option<String>,
        #[arg(long, short, default_value = "out.jsonl")]
        output: PathBuf,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Re-derive the whole index from the output file.
        #[arg(long)]
        rebuild: bool,
        #[arg(long, default_value = "mappings")]
        mappings: PathBuf,
        /// Mapping schema name (default: the first loaded).
        #[arg(long)]
        schema: Option<String>,
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

impl EngineArgs {
    fn config(&self, inputs: Vec<PathBuf>, tail_capacity: usize) -> Result<engine::Config> {
        let threads = self.threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(1));
        Ok(engine::Config {
            inputs,
            store: self.store.clone(),
            output: self.output.clone(),
            parsers: self.parsers.clone(),
            mappings: self.mappings.clone(),
            schema: self.schema.clone(),
            threads,
            default_offset_secs: parse_tz(&self.tz)?,
            batch_events: self.batch,
            queue_batches: self.queue,
            pending: (self.infer_threshold > 0).then(|| self.pending.clone()),
            infer_threshold: self.infer_threshold,
            tail_capacity,
        })
    }
}

fn print_report(report: &engine::Report) -> Result<()> {
    let mut err = std::io::stderr().lock();
    for p in &report.load_problems {
        writeln!(err, "load problem: {p}")?;
    }
    for p in &report.input_problems {
        writeln!(err, "input problem: {p}")?;
    }
    writeln!(err, "definitions: {} parsers loaded, {} file problems", report.parsers_loaded, report.load_problems.len())?;
    writeln!(err, "{}", report.snapshot)?;
    if report.inference_secs > 0.0 || !report.pending.is_empty() {
        writeln!(err, "pending: {} proposals awaiting review (final inference pass {:.3} s)", report.pending.len(), report.inference_secs)?;
        for p in &report.pending {
            writeln!(err, "  {}  source {}  {} lines  {} templates  {} unmatched{}{}", p.id, p.source, p.lines, p.templates, p.unmatched, if p.edited { "  edited" } else { "" }, if p.problems > 0 { "  PROBLEMS" } else { "" })?;
        }
    }
    Ok(())
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { inputs, engine: args, report_json } => {
            let cfg = args.config(inputs, 16)?;
            let report = engine::run(&cfg)?;
            print_report(&report)?;
            if let Some(path) = report_json {
                std::fs::write(&path, serde_json::to_string_pretty(&report.snapshot)?).with_context(|| format!("writing {}", path.display()))?;
            }
            Ok(())
        }
        Cmd::Serve { watch, engine: args, listen, tail, ui_dir, poll_ms } => {
            let cfg = args.config(watch, tail)?;
            let live = engine::Live::open(&cfg, true)?;
            for p in live.load_problems.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                eprintln!("load problem: {p}");
            }
            let server = crate::server::Server::start(std::sync::Arc::clone(&live), listen, ui_dir)?;
            server.install_ctrl_c(std::sync::Arc::clone(&live));
            eprintln!("ulpf: serving http://{} ; watching {} ; {} parsers loaded ; ctrl-c to stop", server.addr, cfg.inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "), live.pipeline().registry.len());
            let report = engine::serve(&live, Duration::from_millis(poll_ms.max(50)));
            server.shutdown();
            print_report(&report?)
        }
        Cmd::Infer { file, pending, parsers, decisions } => {
            let bytes = std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let registry = ulpf_parse::load_dir(&parsers).ok().map(|r| ulpf_parse::Registry::new(r.parsers));
            let mut hint = None;
            let all: Vec<&[u8]> = ulpf_store::Framer::new(&bytes, true).map(|r| &bytes[r]).collect();
            let unknown: Vec<&[u8]> = all
                .iter()
                .copied()
                .filter(|e| match &registry {
                    Some(reg) => {
                        let hit = reg.detect(e, hint);
                        hint = hit.or(hint);
                        hit.is_none()
                    }
                    None => true,
                })
                .collect();
            eprintln!("{}: {} events, {} unknown to the {} loaded parsers", file.display(), all.len(), unknown.len(), registry.as_ref().map_or(0, ulpf_parse::Registry::len));
            let proposal = ulpf_infer::infer(&name, &unknown, &ulpf_infer::Params::default());
            let pend = crate::pending::Pending::open(&pending)?;
            let lines: Vec<Vec<u8>> = unknown.iter().map(|l| l.to_vec()).collect();
            let outcome = pend.write(&proposal, &lines).map_err(|e| anyhow::anyhow!("{e}"))?;
            let e = &proposal.evidence;
            println!("{}", toml::to_string(&proposal.definition)?);
            println!("# {} lines, {} used, {} templates, {} unmatched {:?}", e.lines_seen, e.lines_used, e.templates.len(), e.unmatched.count, e.unmatched.by_reason);
            for t in &e.templates {
                println!("# T{:<3} support {:<4} verified {:<4} {}", t.id, t.support, t.verified, t.pattern);
            }
            if decisions {
                for d in &e.decisions {
                    println!("# {d}");
                }
            }
            println!("# pending: {outcome:?} -> {}/{}.toml", pending.display(), crate::pending::Pending::id_for(&name));
            Ok(())
        }
        Cmd::Check { parsers, mappings, pending } => {
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
            let mut problems = p.errors.len() + m.errors.len();
            for e in &p.errors {
                writeln!(out, "ERROR parser  {e}")?;
            }
            for e in &m.errors {
                writeln!(out, "ERROR mapping {e}")?;
            }
            if let Some(dir) = pending {
                let pend = crate::pending::Pending::open(&dir)?;
                for s in pend.list() {
                    writeln!(out, "pending {:<24} source {} ({} lines, {} templates{})", s.id, s.source, s.lines, s.templates, if s.edited { ", edited" } else { "" })?;
                    if let Ok(d) = pend.get(&s.id) {
                        for e in &d.problems {
                            writeln!(out, "ERROR pending {e}")?;
                            problems += 1;
                        }
                    }
                }
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
        Cmd::Pivot { kind, value, output, limit, rebuild, mappings, schema } => {
            if rebuild {
                let mut maps = ulpf_normalize::load_dir(&mappings).with_context(|| format!("mappings directory {}", mappings.display()))?;
                for e in &maps.errors {
                    eprintln!("mapping problem: {e}");
                }
                let idx = match &schema {
                    Some(name) => maps.mappings.iter().position(|m| m.schema_name() == *name).with_context(|| format!("no mapping named `{name}`"))?,
                    None => {
                        anyhow::ensure!(!maps.mappings.is_empty(), "no usable mapping in {}", mappings.display());
                        0
                    }
                };
                let mapping = maps.mappings.swap_remove(idx);
                let report = crate::pivot::rebuild(&output, &mapping, 1024)?;
                eprintln!(
                    "rebuilt {} from {} events, {} postings, {} unreadable lines in {:.3} s",
                    crate::pivot::index_path(&output).display(),
                    report.events,
                    report.postings,
                    report.unreadable_lines,
                    report.elapsed_secs
                );
                return Ok(());
            }
            let (Some(kind), Some(value)) = (kind, value) else { bail!("usage: ulpf pivot KIND VALUE --output out.jsonl, or ulpf pivot --rebuild --output out.jsonl") };
            let kind = ulpf_normalize::EntityKind::from_name(&kind)
                .with_context(|| format!("unknown entity kind `{kind}`; one of src_ip, dst_ip, user, dst_port, device"))?;
            let index = crate::pivot::PivotIndex::open(&output)?;
            let page = index.query(&crate::pivot::PivotQuery {
                kind,
                value: value.as_bytes(),
                limit,
                before: None,
                after: None,
                order: crate::pivot::Order::Desc,
            })?;
            eprintln!(
                "{} {}: {} events on {} device(s), {} .. {}",
                kind.name(),
                value,
                page.total,
                page.devices.len(),
                page.first_time.unwrap_or(0),
                page.last_time.unwrap_or(0)
            );
            let mut out = std::io::stdout().lock();
            for e in &page.events {
                writeln!(out, "{}", serde_json::to_string(&e.line)?)?;
            }
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
