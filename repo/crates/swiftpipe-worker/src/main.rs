//! `swiftpipe-worker`: started by the engine (`holon-http/1`); see the crate docs.
//!
//! Schemas: `SWIFTPIPE_SCHEMAS`, else `./schemas` (the worker dir, which is inside the digest),
//! else the first `schemas` dir above the executable (`<worker>/bin/<triple>/swiftpipe-worker`).

use std::path::PathBuf;

use swiftpipe_worker::Worker;

fn schema_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("SWIFTPIPE_SCHEMAS") {
        return Some(PathBuf::from(d));
    }
    let cwd = PathBuf::from("schemas");
    if cwd.is_dir() {
        return Some(cwd);
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).map(|a| a.join("schemas")).find(|d| d.is_dir())
}

fn main() {
    let Some(dir) = schema_dir() else {
        eprintln!("swiftpipe-worker: no schemas dir (set SWIFTPIPE_SCHEMAS or ship ./schemas)");
        std::process::exit(2);
    };
    let worker = match Worker::new(&dir) {
        Ok(w) if w.schemas_loaded() > 0 => w,
        Ok(_) => {
            eprintln!("swiftpipe-worker: no valid mt<nnn>.yaml in {}", dir.display());
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("swiftpipe-worker: {e:#}");
            std::process::exit(2);
        }
    };
    eprintln!("swiftpipe-worker: {} MT schemas from {}", worker.schemas_loaded(), dir.display());
    let engine = std::env::var("HOLON_ENGINE").unwrap_or_default();
    let token = std::env::var("HOLON_TOKEN").unwrap_or_default();
    if let Err(e) = rae_holon_worker::serve_http(move |req| worker.handle(req, &engine, &token)) {
        eprintln!("swiftpipe-worker: {e}");
        std::process::exit(1);
    }
}
