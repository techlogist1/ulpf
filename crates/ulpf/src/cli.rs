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
    /// Mapping schema name (default: `ocsf` when present, else the first loaded).
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
    /// Fix the receipt time of every event (RFC 3339 or epoch seconds/millis) for a
    /// reproducible run; the fixture harness uses 2026-09-04T12:00:00Z.
    #[arg(long)]
    receipt: Option<String>,
    /// Build the entity index beside the output: `on` or `off`. Default `off` for `run`
    /// (bulk throughput; `ulpf pivot --rebuild` builds it afterwards) and `on` for `serve`
    /// (the UI pivots live). Measured 2026-09-05: the index thread caps a run at ~27k
    /// events/s on this machine, one tenth of the pipeline without it.
    #[arg(long, value_name = "on|off", value_parser = clap::builder::BoolishValueParser::new(), hide_possible_values = true)]
    pivot: Option<bool>,
    /// Also write the normalized events to this Parquet file (an additional sink; the
    /// JSON Lines output is always written). A Parquet file is unreadable until closed.
    #[arg(long)]
    parquet: Option<PathBuf>,
    /// Only ingest files under an input directory whose relative path matches (repeatable).
    /// Shell patterns: `*` (not across `/`), `**`, `?`. A pattern with no `/` also matches
    /// the file's name at any depth, so `--include '*.log'` takes nested logs; one with a
    /// `/` is anchored at the input directory. Case-sensitive. Default: every file.
    #[arg(long, value_name = "PATTERN")]
    include: Vec<String>,
    /// Never ingest files under an input directory whose relative path (or, for a pattern
    /// with no `/`, name at any depth) matches; a directory that matches is not descended
    /// (repeatable). The defaults `*.md`, `README*`, `.*`, `*.truth.tsv`, `*.expected.jsonl`
    /// stay in force under any --exclude; `--exclude ''` drops them and ingests everything.
    /// A file named on the command line itself is always taken.
    #[arg(long, value_name = "PATTERN")]
    exclude: Vec<String>,
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
        /// Listen for syslog over UDP (one datagram is one event), e.g. 127.0.0.1:5514.
        #[arg(long)]
        syslog_udp: Option<SocketAddr>,
        /// Listen for syslog over TCP (RFC 6587 octet counting or newline framing).
        #[arg(long)]
        syslog_tcp: Option<SocketAddr>,
        /// Close the current Parquet file after this many rows (`--parquet` only).
        #[arg(long, default_value_t = 1_000_000)]
        parquet_roll_rows: u64,
        /// Close the current Parquet file after this many seconds (`--parquet` only).
        #[arg(long, default_value_t = 300)]
        parquet_roll_secs: u64,
        /// Stop when this process, the one that started the engine, is gone (the desktop shell passes its own pid).
        #[arg(long, value_name = "PID")]
        exit_with_parent: Option<u32>,
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
    /// Re-run every stored record through the current parsers and mappings into a new
    /// output version, and diff it against the previous version.
    Replay {
        #[command(flatten)]
        engine: EngineArgs,
        /// Also write the replay report as JSON to this path.
        #[arg(long)]
        report_json: Option<PathBuf>,
    },
    /// Recompute every digest and chain value in a raw store.
    Verify {
        #[arg(long, default_value = "ulpf.ulpf-store")]
        store: PathBuf,
        /// Also check every checkpoint of an attestation taken earlier (`ulpf attest`).
        #[arg(long)]
        attestation: Option<PathBuf>,
    },
    /// Write the store's attestation document: store id, genesis, head and checkpoints.
    Attest {
        #[arg(long, default_value = "ulpf.ulpf-store")]
        store: PathBuf,
        /// Write the JSON here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
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
    /// Play the demo of PROGRESS.md: existing subcommands, the watch directory and the API only.
    Demo {
        /// Fixed pauses instead of waiting for Enter, then stop and reset.
        #[arg(long)]
        auto: bool,
        /// Check the inputs, the ports and that every title and command still matches PROGRESS.md; start nothing.
        #[arg(long)]
        check: bool,
        /// Stop a leftover demo server and remove the demo directory.
        #[arg(long)]
        reset: bool,
        /// Scratch directory the demo owns (removed before and after; the server's parsers and pending live here).
        #[arg(long, default_value = "demo")]
        dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        /// Syslog address the demo's server listens on, UDP and TCP.
        #[arg(long, default_value = "127.0.0.1:5514")]
        syslog: SocketAddr,
        /// Repository root: samples/, heldout/, parsers/, mappings/ and PROGRESS.md come from here.
        #[arg(long, default_value = ".")]
        repo: PathBuf,
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
    anyhow::ensure!(h <= 14 && m < 60, "timezone offset out of range: `{s}` (at most 14:00)");
    Ok(sign * (h * 3600 + m * 60))
}

impl EngineArgs {
    fn config(&self, inputs: Vec<PathBuf>, tail_capacity: usize, parquet_roll: Option<(u64, Duration)>) -> Result<engine::Config> {
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
            syslog_udp: None,
            syslog_tcp: None,
            pivot_index: self.pivot.unwrap_or(false),
            receipt_nanos: match &self.receipt {
                Some(text) => {
                    let ctx = ulpf_time::Context { receipt_epoch_nanos: engine::now_nanos(), default_offset_secs: 0 };
                    Some(ulpf_time::parse(text.as_bytes(), &ulpf_time::Format::Auto, &ctx).map_err(|e| anyhow::anyhow!("--receipt `{text}`: {e:?}"))?.epoch_nanos)
                }
                None => None,
            },
            parquet: self.parquet.clone(),
            parquet_roll: self.parquet.as_ref().and(parquet_roll),
            filter: engine::Filter {
                include: self.include.iter().filter(|p| !p.is_empty()).cloned().collect(),
                exclude: excludes(&self.exclude),
            },
        })
    }
}

/// The exclude list for the `--exclude` patterns given: the defaults stay under any of them,
/// since one more pattern must not open `.git` to an append-only store; the empty pattern is
/// the one way to drop them.
fn excludes(given: &[String]) -> Vec<String> {
    let mut out = if given.iter().any(String::is_empty) { Vec::new() } else { engine::Filter::default().exclude };
    out.extend(given.iter().filter(|p| !p.is_empty()).cloned());
    out
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
    if report.recovered > 0 {
        writeln!(err, "recovered: {} stored records an interrupted run had not written to the output", report.recovered)?;
    }
    writeln!(err, "{}", report.snapshot)?;
    for e in &report.excluded {
        writeln!(err, "excluded: {e}")?;
    }
    if report.snapshot.files_excluded >= engine::EXCLUDED_CAP {
        writeln!(err, "excluded: the {} name cap was reached; the count and the list stop there", engine::EXCLUDED_CAP)?;
    }
    let unlisted = report.snapshot.files_excluded.saturating_sub(report.excluded.len() as u64);
    if unlisted > 0 {
        writeln!(err, "excluded: {unlisted} more not listed")?;
    }
    if report.inference_secs > 0.0 || !report.pending.is_empty() {
        writeln!(err, "pending: {} proposals awaiting review (final inference pass {:.3} s)", report.pending.len(), report.inference_secs)?;
        for p in &report.pending {
            writeln!(err, "  {}  source {}  {} lines  {} templates  {} unmatched{}{}", p.id, p.source, p.lines, p.templates, p.unmatched, if p.edited { "  edited" } else { "" }, if p.problems > 0 { "  PROBLEMS" } else { "" })?;
        }
    }
    Ok(())
}

fn print_replay(r: &crate::replay::ReplayReport) -> Result<()> {
    let mut err = std::io::stderr().lock();
    let against = r.previous_version.map(|v| format!(" against v{v}")).unwrap_or_default();
    writeln!(err, "replay v{}{}: {} events in {:.3} s ({:.0} events/s), schema {}, parsers generation {}", r.version, against, r.events, r.elapsed_secs, r.events_per_sec, r.schema, r.parsers_generation)?;
    writeln!(err, "  counts: detected {}  no_parser {}  parsed {}  parse_failed {}  class_unknown {}", r.counts.detected, r.counts.no_parser, r.counts.parsed, r.counts.parse_failed, r.counts.class_unknown)?;
    let s = &r.summary;
    writeln!(err, "  events: unchanged {}  changed {}  only_in_new {}  only_in_old {}", s.unchanged, s.changed, s.only_in_new, s.only_in_old)?;
    writeln!(err, "  fields: added {}  lost {}  changed {}", s.fields_added, s.fields_lost, s.fields_changed)?;
    for p in s.parser_changes.iter().take(10) {
        writeln!(err, "  parser: {} -> {}  ({} events)", p.from.as_deref().unwrap_or("none"), p.to.as_deref().unwrap_or("none"), p.events)?;
    }
    for f in s.by_field.iter().take(15) {
        writeln!(err, "  field {:<40} added {:<6} lost {:<6} changed {}", f.path, f.added, f.lost, f.changed)?;
    }
    for w in &r.why {
        writeln!(err, "  why: {w}")?;
    }
    writeln!(err, "  output: {}", r.output.display())?;
    if let Some(d) = &r.diff {
        writeln!(err, "  diff:   {}", d.display())?;
    }
    Ok(())
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { inputs, engine: args, report_json } => {
            // batch mode writes one Parquet file: there is nothing to roll for
            let cfg = args.config(inputs, 16, None)?;
            let report = engine::run(&cfg)?;
            print_report(&report)?;
            if let Some(path) = report_json {
                std::fs::write(&path, serde_json::to_string_pretty(&report.snapshot)?).with_context(|| format!("writing {}", path.display()))?;
            }
            Ok(())
        }
        Cmd::Serve { watch, engine: args, listen, tail, ui_dir, poll_ms, syslog_udp, syslog_tcp, parquet_roll_rows, parquet_roll_secs, exit_with_parent } => {
            let roll = (parquet_roll_rows.max(1), Duration::from_secs(parquet_roll_secs.max(1)));
            let mut cfg = args.config(watch, tail, Some(roll))?;
            cfg.syslog_udp = syslog_udp;
            cfg.syslog_tcp = syslog_tcp;
            cfg.pivot_index = args.pivot.unwrap_or(true);
            let live = engine::Live::open(&cfg, true)?;
            for p in live.load_problems.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                eprintln!("load problem: {p}");
            }
            let server = crate::server::Server::start(std::sync::Arc::clone(&live), listen, ui_dir)?;
            server.install_ctrl_c(std::sync::Arc::clone(&live));
            if let Some(pid) = exit_with_parent {
                engine::stop_with_parent(std::sync::Arc::clone(&live), pid);
            }
            let listeners: Vec<String> = [("udp", cfg.syslog_udp), ("tcp", cfg.syslog_tcp)].iter().filter_map(|(k, a)| a.map(|a| format!("syslog {k} {a}"))).collect();
            eprintln!("ulpf: serving http://{} ; watching {} ; {}{} parsers loaded ; ctrl-c to stop", server.addr, cfg.inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "), if listeners.is_empty() { String::new() } else { format!("{} ; ", listeners.join(", ")) }, live.pipeline().registry.len());
            let report = engine::serve(&live, Duration::from_millis(poll_ms.max(50)));
            server.shutdown();
            print_report(&report?)
        }
        Cmd::Replay { engine: args, report_json } => {
            anyhow::ensure!(!engine::output_is_sink(&args.output), "replay needs a file output (--output), not stdout or a device");
            let reader = ulpf_store::RawReader::open(&args.store).with_context(|| format!("opening store {}", args.store.display()))?;
            let names = match reader.source_names() {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("ulpf: replay: {e} (a `serve` holds this store; replay it from the server with POST /api/replay)");
                    std::process::exit(2);
                }
            };
            let (pipeline, problems) = crate::pipeline::Pipeline::load(&args.parsers, &args.mappings, args.schema.as_deref(), parse_tz(&args.tz)?)?;
            for p in problems {
                eprintln!("load problem: {p}");
            }
            let versions = crate::replay::Versions::new(&args.output);
            let version = versions.next();
            let total = reader.len();
            let threads = args.threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(1));
            let job = crate::replay::Job { versions, version, pipeline: std::sync::Arc::new(pipeline), threads, batch: args.batch, parsers_generation: 0, names, reader, total };
            let progress = std::sync::atomic::AtomicU64::new(0);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let report = crate::replay::run(job, &progress, &cancel)?;
            print_replay(&report)?;
            if let Some(path) = report_json {
                std::fs::write(&path, serde_json::to_string_pretty(&report)?).with_context(|| format!("writing {}", path.display()))?;
            }
            Ok(())
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
        Cmd::Verify { store, attestation } => {
            // The index header is checked first and named by field: a rewritten magic,
            // version or store id used to surface as "predates the integrity chain" from
            // the open, or as a shorter store that verified clean (finding 19).
            let mut header = ulpf_store::index_header(&store).with_context(|| format!("opening store {}", store.display()))?;
            for line in header.lines() {
                println!("{line}");
            }
            let reader = match ulpf_store::RawReader::open(&store) {
                Ok(reader) => reader,
                // The header lines above say which field; the open error names the store.
                Err(e) if !header.problems.is_empty() => {
                    println!("{e}");
                    std::process::exit(1)
                }
                Err(e) => return Err(e).with_context(|| format!("opening store {}", store.display())),
            };
            let against_store = ulpf_store::index_header_against_store(&reader);
            for line in against_store.lines() {
                println!("{line}");
            }
            header.problems.extend(against_store.problems);
            let attestation = match &attestation {
                Some(path) => {
                    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
                    Some(serde_json::from_str::<ulpf_store::Attestation>(&text).with_context(|| format!("{} is not an attestation document", path.display()))?)
                }
                None => None,
            };
            let report = match &attestation {
                Some(att) => reader.verify_against(att),
                None => reader.verify(),
            };
            println!("store {} genesis {}", ulpf_store::hex(&reader.store_id()), ulpf_store::hex(&reader.genesis()));
            println!("verified {} records, {} corrupt", report.checked, report.corrupt.len());
            for id in report.corrupt.iter().take(20) {
                println!("corrupt: raw id {}", id.0);
            }
            match report.first_bad {
                None => println!("chain ok (head {})", reader.head().map(|h| ulpf_store::hex(&h)).unwrap_or_else(|| "-".into())),
                Some((id, reason)) => println!("chain broken at id {} ({})", id.0, reason.as_str()),
            }
            if let Some(att) = &attestation {
                if let Some(problem) = &report.attestation_problem {
                    println!("attestation: {problem}");
                }
                match report.bad_checkpoint {
                    Some(id) => println!("attestation: checkpoint at id {} disagrees with the store (generated {})", id.0, att.generated),
                    None => println!("attestation: {} of {} checkpoints agree ({} records attested, generated {})", report.checkpoints, att.checkpoints.len(), att.records, att.generated),
                }
            }
            if !report.ok() || !header.problems.is_empty() {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Attest { store, out } => {
            let reader = ulpf_store::RawReader::open(&store).with_context(|| format!("opening store {}", store.display()))?;
            let json = serde_json::to_string_pretty(&reader.attest())?;
            match out {
                Some(path) => {
                    std::fs::write(&path, format!("{json}\n")).with_context(|| format!("writing {}", path.display()))?;
                    eprintln!("attested {} records to {}", reader.len(), path.display());
                }
                None => println!("{json}"),
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
                before: None, before_id: None,
                after: None, after_id: None,
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
        Cmd::Demo { auto, check, reset, dir, listen, syslog, repo } => {
            let code = crate::demo::main(crate::demo::Args { auto, check, reset, dir, listen, syslog, repo })?;
            if code != 0 {
                std::process::exit(code);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exclude_adds_to_the_defaults_and_only_the_empty_pattern_drops_them() {
        assert_eq!(excludes(&[]), engine::Filter::default().exclude);
        let one = excludes(&["*.gz".to_string()]);
        assert!(one.iter().any(|p| p == ".*"), "--exclude '*.gz' keeps the dotfile guard: {one:?}");
        assert_eq!(one.last().map(String::as_str), Some("*.gz"));
        assert!(excludes(&[String::new()]).is_empty(), "--exclude '' ingests everything");
        assert_eq!(excludes(&[String::new(), "*.gz".to_string()]), vec!["*.gz".to_string()]);
    }
}
