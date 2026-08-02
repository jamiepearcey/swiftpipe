//! `ingest` — run the ingestion wedge end to end from the command line.
//!
//! Detects the input format (SWIFT MT940 / MT securities, ISO 20022 camt.053,
//! custodian CSV), routes it through the matching source adapter into the
//! normalized model, writes columnar **Parquet** (the data-plane seam), and
//! prints a **Tier-1 P&L / positions** summary — no market data required.
//!
//! ```text
//! ingest <input> [--type auto|mt940|camt053|mt-securities|csv]
//!                [--schema-dir DIR] [--out-dir DIR]
//! ```

mod csdr;
mod serve;
mod snapshot;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use ingest_core::{CashStatement, SecurityEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    CashMt940,
    CashCamt,
    PositionsCsv,
    PositionsMt,
}

fn main() -> Result<()> {
    // Subcommand dispatch. Bare `ingest <input>` runs the single-file wedge as
    // before; the named subcommands build the read model + serve it:
    //   snapshot  → static recon-snapshot.json (UI offline fallback)
    //   store     → materialise the Parquet read-model store (write side)
    //   serve     → query the store via DuckDB over HTTP (read side)
    match std::env::args().nth(1).as_deref() {
        Some("snapshot") => return run_snapshot(),
        Some("store") => return run_store(),
        Some("serve") => return run_serve_cmd(),
        _ => {}
    }

    let mut input: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from(".");
    let mut schema_dir = PathBuf::from("examples/schemas");
    let mut forced: Option<Route> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out-dir" => {
                out_dir = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--out-dir needs a value"))?
            }
            "--schema-dir" => {
                schema_dir = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--schema-dir needs a value"))?
            }
            "--type" => {
                forced = Some(parse_route(
                    &args.next().ok_or_else(|| anyhow!("--type needs a value"))?,
                )?);
            }
            "-h" | "--help" => {
                eprintln!("usage: ingest <input> [--type auto|mt940|camt053|mt-securities|csv] [--schema-dir DIR] [--out-dir DIR]");
                return Ok(());
            }
            other => {
                if input.is_none() {
                    input = Some(PathBuf::from(other));
                } else {
                    bail!("unexpected argument: {other}");
                }
            }
        }
    }

    let input = input.ok_or_else(|| anyhow!("missing input file (see --help)"))?;
    let content =
        std::fs::read_to_string(&input).with_context(|| format!("reading {}", input.display()))?;
    std::fs::create_dir_all(&out_dir).ok();

    let route = match forced {
        Some(r) => r,
        None => detect(&input, &content)?,
    };

    match route {
        Route::CashMt940 | Route::CashCamt => run_cash(route, &content, &out_dir),
        Route::PositionsCsv | Route::PositionsMt => {
            run_positions(route, &content, &schema_dir, &out_dir)
        }
    }
}

/// Parse one cash input (MT940 / camt.053) into normalized statements.
fn parse_cash(route: Route, content: &str) -> Vec<CashStatement> {
    match route {
        // Empty id → the parser adopts the statement's own `:20:` reference.
        Route::CashMt940 => swift_mt940::parse_mt940(content, "").into_iter().collect(),
        Route::CashCamt => mx_camt::parse_camt053(content, ""),
        _ => unreachable!(),
    }
}

/// Parse one positions input (custodian CSV / MT securities) into events.
fn parse_positions(route: Route, content: &str, schema_dir: &Path) -> Result<Vec<SecurityEvent>> {
    Ok(match route {
        Route::PositionsCsv => ingest_tabular::parse_csv(content),
        Route::PositionsMt => parse_mt_securities(content, schema_dir)?,
        _ => unreachable!(),
    })
}

fn run_cash(route: Route, content: &str, out_dir: &Path) -> Result<()> {
    let statements = parse_cash(route, content);
    if statements.is_empty() {
        bail!("no cash statements parsed from the input");
    }
    let out = out_dir.join("cash.parquet");
    ingest_parquet::write_cash_entries(&statements, &out)?;
    let pnl = ingest_pnl::compute_cash_pnl(&statements);

    println!("cash statements : {}", pnl.statements);
    println!("entries         : {}", pnl.entries);
    println!("inflows         : {:>14.2}", pnl.inflows);
    println!("outflows        : {:>14.2}", pnl.outflows);
    println!("net movement    : {:>14.2}", pnl.net_movement);
    println!("reconciles      : {}", pnl.all_reconciled);
    if !pnl.by_type.is_empty() {
        println!("by transaction type:");
        for (t, v) in &pnl.by_type {
            println!("  {t:<6} {v:>14.2}");
        }
    }
    println!("→ wrote {}", out.display());
    Ok(())
}

fn run_positions(route: Route, content: &str, schema_dir: &Path, out_dir: &Path) -> Result<()> {
    let events = parse_positions(route, content, schema_dir)?;
    if events.is_empty() {
        bail!("no securities events parsed from the input");
    }
    let out = out_dir.join("positions.parquet");
    ingest_parquet::write_security_events(&events, &out)?;
    let summary = ingest_pnl::summarize_positions(&events);

    println!("securities events : {}", summary.positions);
    println!("distinct ISINs    : {}", summary.by_isin.len());
    if summary.missing_isin > 0 {
        println!("missing ISIN      : {}", summary.missing_isin);
    }
    println!("quantity by ISIN:");
    for (isin, qty) in &summary.by_isin {
        println!("  {isin:<14} {qty:>14.2}");
    }
    println!("→ wrote {}", out.display());
    Ok(())
}

/// `ingest snapshot <files...> [--out FILE] [--schema-dir DIR]` — ingest any
/// mix of cash + positions inputs and emit one combined `recon-snapshot.json`
/// in the UI contract (the presentation-plane seam for the Reconciliation desk).
fn run_snapshot() -> Result<()> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut out = PathBuf::from("recon-snapshot.json");
    let mut schema_dir = PathBuf::from("examples/schemas");

    let mut args = std::env::args().skip(2); // skip argv0 + "snapshot"
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--out needs a value"))?
            }
            "--schema-dir" => {
                schema_dir = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--schema-dir needs a value"))?
            }
            "-h" | "--help" => {
                eprintln!("usage: ingest snapshot <files...> [--out recon-snapshot.json] [--schema-dir DIR]");
                return Ok(());
            }
            other => inputs.push(PathBuf::from(other)),
        }
    }
    if inputs.is_empty() {
        bail!("snapshot: no input files (see --help)");
    }

    let mut statements: Vec<CashStatement> = Vec::new();
    let mut events: Vec<SecurityEvent> = Vec::new();
    for path in &inputs {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let route = detect(path, &content)
            .with_context(|| format!("detecting type of {}", path.display()))?;
        match route {
            Route::CashMt940 | Route::CashCamt => statements.extend(parse_cash(route, &content)),
            Route::PositionsCsv | Route::PositionsMt => {
                events.extend(parse_positions(route, &content, &schema_dir)?)
            }
        }
    }

    let snap = snapshot::ReconSnapshot::build(&statements, &events);
    let json = serde_json::to_string_pretty(&snap)?;
    std::fs::write(&out, &json).with_context(|| format!("writing {}", out.display()))?;

    println!("statements : {}", snap.statements.len());
    println!("positions  : {}", snap.positions.len());
    println!("→ wrote {}", out.display());
    Ok(())
}

/// `ingest store <files...> [--store DIR] [--schema-dir DIR]` — the write side
/// of the read model: ingest a mix of cash + positions inputs and materialise
/// them as the Parquet store `serve` queries (statement headers + entry lines +
/// positions). Overwrites the store's canonical set.
fn run_store() -> Result<()> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut store = PathBuf::from("recon-store");
    let mut schema_dir = PathBuf::from("examples/schemas");
    let mut penalty_statement: Option<PathBuf> = None;
    let mut default_currency = String::from("EUR");

    let mut args = std::env::args().skip(2);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => {
                store = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--store needs a value"))?
            }
            "--schema-dir" => {
                schema_dir = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--schema-dir needs a value"))?
            }
            "--penalty-statement" => {
                penalty_statement = Some(
                    args.next()
                        .map(PathBuf::from)
                        .ok_or_else(|| anyhow!("--penalty-statement needs a value"))?,
                )
            }
            "--currency" => {
                default_currency = args
                    .next()
                    .ok_or_else(|| anyhow!("--currency needs a value"))?
            }
            "-h" | "--help" => {
                eprintln!("usage: ingest store <files...> [--store recon-store] [--schema-dir DIR] [--penalty-statement FILE.csv] [--currency EUR]");
                return Ok(());
            }
            other => inputs.push(PathBuf::from(other)),
        }
    }
    if inputs.is_empty() {
        bail!("store: no input files (see --help)");
    }

    let mut statements: Vec<CashStatement> = Vec::new();
    let mut events: Vec<SecurityEvent> = Vec::new();
    for path in &inputs {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let route = detect(path, &content)
            .with_context(|| format!("detecting type of {}", path.display()))?;
        match route {
            Route::CashMt940 | Route::CashCamt => statements.extend(parse_cash(route, &content)),
            Route::PositionsCsv | Route::PositionsMt => {
                // `SecurityEvent::transaction_ref` (MT537 `:20C::RELA`, else
                // `:20C::SEME`) is the real per-transaction reference penalty
                // accruals and recon keys use — no filename scaffolding needed.
                events.extend(parse_positions(route, &content, &schema_dir)?);
            }
        }
    }

    // CSDR expected penalties, computed from the failing/pending events.
    let accruals = ingest_penalty::compute_penalty_accruals(
        &events,
        &ingest_penalty::PenaltyRateTable::starter(),
        &default_currency,
    );
    // The CSD monthly penalty statement (reported figures), when provided.
    let reported = match &penalty_statement {
        Some(path) => {
            let csv = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            ingest_penalty::parse_penalty_statement_csv(&csv)
        }
        None => Vec::new(),
    };

    std::fs::create_dir_all(&store)
        .with_context(|| format!("creating store {}", store.display()))?;
    ingest_parquet::write_cash_statements(&statements, &store.join("cash_statements.parquet"))?;
    ingest_parquet::write_cash_entries(&statements, &store.join("cash_entries.parquet"))?;
    ingest_parquet::write_security_events(&events, &store.join("positions.parquet"))?;
    ingest_parquet::write_penalty_accruals(&accruals, &store.join("penalty_accruals.parquet"))?;
    ingest_parquet::write_penalty_statements(&reported, &store.join("penalty_statements.parquet"))?;

    println!("statements : {}", statements.len());
    println!(
        "entries    : {}",
        statements.iter().map(|s| s.entries.len()).sum::<usize>()
    );
    println!(
        "positions  : {}",
        events.iter().filter(|e| e.isin.is_some()).count()
    );
    println!(
        "penalties  : {} computed, {} reported",
        accruals.len(),
        reported.len()
    );
    println!("→ store {}", store.display());
    Ok(())
}

/// `ingest serve [--store DIR] [--bind ADDR]` — the read side: serve
/// `GET /recon/snapshot` by querying the Parquet store via DuckDB.
fn run_serve_cmd() -> Result<()> {
    let mut store = PathBuf::from("recon-store");
    let mut bind: SocketAddr = "127.0.0.1:7390".parse().unwrap();

    let mut args = std::env::args().skip(2);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => {
                store = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--store needs a value"))?
            }
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| anyhow!("--bind needs a value"))?
                    .parse()
                    .context("parsing --bind address")?;
            }
            "-h" | "--help" => {
                eprintln!("usage: ingest serve [--store recon-store] [--bind 127.0.0.1:7390]");
                return Ok(());
            }
            other => bail!("unexpected argument: {other}"),
        }
    }
    serve::run_serve(store, bind)
}

fn parse_mt_securities(
    content: &str,
    schema_dir: &Path,
) -> Result<Vec<ingest_core::SecurityEvent>> {
    let mtype = detect_mt_type(content)
        .context("could not detect the MT message type from the FIN header")?;
    let schema_path = schema_dir.join(format!("{}.yaml", mtype.to_lowercase()));
    let yaml = std::fs::read_to_string(&schema_path).with_context(|| {
        format!(
            "reading schema {} (pass --schema-dir)",
            schema_path.display()
        )
    })?;
    let catalog = swift_schema::SchemaCatalog::from_yaml_str(&yaml)
        .map_err(|e| anyhow!("schema parse: {e:?}"))?;
    catalog
        .validate()
        .map_err(|e| anyhow!("invalid schema: {e:?}"))?;
    let inbound = swift_db::InboundMessage {
        id: "cli".into(),
        message_type: mtype,
        body: content.to_string(),
    };
    let parsed = swift_core::parse_message(inbound.body.as_bytes());
    let batch = swift_db::materialize_message(&catalog, &inbound, &parsed)?;
    Ok(swift_normalize::normalize(&batch))
}

fn parse_route(s: &str) -> Result<Route> {
    Ok(match s {
        "mt940" => Route::CashMt940,
        "camt053" => Route::CashCamt,
        "csv" => Route::PositionsCsv,
        "mt-securities" => Route::PositionsMt,
        "auto" => bail!("--type auto: omit --type to auto-detect"),
        other => bail!("unknown --type {other}"),
    })
}

fn detect(path: &Path, content: &str) -> Result<Route> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "xml" || content.trim_start().starts_with("<?xml") || content.contains("camt.053") {
        return Ok(Route::CashCamt);
    }
    if ext == "csv" {
        return Ok(Route::PositionsCsv);
    }
    if content.contains(":61:") {
        return Ok(Route::CashMt940);
    }
    if content.contains(":35B:") {
        return Ok(Route::PositionsMt);
    }
    if content
        .lines()
        .next()
        .map(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("isin") && l.contains(',')
        })
        .unwrap_or(false)
    {
        return Ok(Route::PositionsCsv);
    }
    bail!("could not detect input type; pass --type mt940|camt053|mt-securities|csv")
}

/// Message type from the FIN application header `{2:I535…}` / `{2:O940…}`.
fn detect_mt_type(s: &str) -> Option<String> {
    let i = s.find("{2:")? + 3;
    let rest = s.get(i..)?;
    let b = rest.as_bytes();
    if b.len() >= 4 && (b[0] == b'I' || b[0] == b'O') && b[1..4].iter().all(u8::is_ascii_digit) {
        Some(format!("MT{}", &rest[1..4]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mt_type_from_header() {
        assert_eq!(
            detect_mt_type("{1:F01BANKBEBB}{2:I535BANKDEFFXXXXN}{4:").as_deref(),
            Some("MT535")
        );
        assert_eq!(detect_mt_type("{2:O940BANK}").as_deref(), Some("MT940"));
        assert_eq!(detect_mt_type("no header"), None);
    }

    #[test]
    fn routes_by_content() {
        assert_eq!(
            detect(Path::new("x.fin"), ":20:R\n:61:2605...").unwrap(),
            Route::CashMt940
        );
        assert_eq!(
            detect(Path::new("x.fin"), ":35B:ISIN GB00...").unwrap(),
            Route::PositionsMt
        );
        assert_eq!(
            detect(Path::new("x.xml"), "<?xml version=\"1.0\"?>").unwrap(),
            Route::CashCamt
        );
        assert_eq!(
            detect(Path::new("x.csv"), "isin,quantity\n").unwrap(),
            Route::PositionsCsv
        );
    }
}
