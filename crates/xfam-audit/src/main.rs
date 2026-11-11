//! xfam-audit [--strict] <jsonl> <answer.md>
//!
//! `--strict` is recognised only as the first argument. Extra arguments are ignored.
//! Normal mode exits 0 on completion, 1 on fatal errors, or 120 on output failure.
//! Strict mode preserves normal stdout and answer-file behaviour, then rejects fatal errors,
//! tool-related events, a missing or blank answer, unparsed lines, a last result without a
//! string response, or a logged error. Rejection exits 3 and writes `strict: FAILED <reasons>`
//! to stderr. Reasons are comma-separated in this order: `fatal_error`, `tool_related_events`,
//! `answer_not_found` or `answer_blank`, `unparsed_lines`, `final_result_not_string`, `logged_error`.
//! With no result event, `answer_not_found` applies without `final_result_not_string`.
//! A logged error is an object with `event` equal to the string "error" or a truthy top-level
//! `error`, using the same truthiness as `step_update`.
//! The schema is otherwise not validated; an error under another key is not recognised.
//! Other fatal diagnostics remain one-line messages. A strict pass uses normal exit handling.
//!
//! Input is streamed; memory depends on line length, distinct kinds, and the retained answer.
//! The answer file is written before stdout. This differs from a buffered reference when
//! stdout fails before the reference reaches the answer write.

use std::ffi::OsString;
use std::io::{BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::process::ExitCode;
use xfam_audit::pyjson::STACK_BYTES;
use xfam_audit::{strict_reasons, StrictFacts};

/// Historical exit status for a failed stdout/stderr flush.
const EXIT_FLUSH_FAILED: u8 = 120;

fn write_stdout(bytes: &[u8]) -> bool {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes).and_then(|()| out.flush()).is_ok()
}

fn strict_failed(facts: &StrictFacts) -> bool {
    let reasons = strict_reasons(facts);
    if reasons.is_empty() {
        return false;
    }
    // Rejection remains exit 3 even when the diagnostic sink is broken.
    let _ = writeln!(std::io::stderr().lock(), "strict: FAILED {}", reasons.join(","));
    true
}

fn died(msg: &str, stdout_ok: bool, strict: bool) -> ExitCode {
    let stderr_ok = writeln!(std::io::stderr().lock(), "xfam-audit: {msg}").is_ok();
    if strict {
        strict_failed(&StrictFacts { fatal_error: true, ..StrictFacts::default() });
        return ExitCode::from(3);
    }
    ExitCode::from(if stdout_ok && stderr_ok { 1 } else { EXIT_FLUSH_FAILED })
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let strict = args.get(1).is_some_and(|s| s == "--strict");
    let first = if strict { 2 } else { 1 };
    if args.len() < first + 2 {
        return died("usage: xfam-audit [--strict] <jsonl> <answer.md>", true, strict);
    }
    let (src, dst) = (args[first].clone(), args[first + 1].clone());
    let input = match std::fs::File::open(&src) {
        Ok(f) => BufReader::with_capacity(1 << 16, f),
        Err(e) => return died(&format!("cannot read {}: {e}", src.to_string_lossy()), true, strict),
    };
    // Deep values need room for recursive parsing, formatting, and dropping.
    let worker = std::thread::Builder::new().stack_size(STACK_BYTES).spawn(move || {
        xfam_audit::run(input, src.as_bytes(), dst.as_bytes(), |bytes| std::fs::write(&dst, bytes))
    });
    let run = match worker.map(|h| h.join()) {
        Ok(Ok(run)) => run,
        _ => return died("worker thread failed", true, strict),
    };
    let stdout_ok = run.stdout.is_empty() || write_stdout(&run.stdout);
    if strict {
        if let Some(msg) = &run.error {
            let _ = writeln!(std::io::stderr().lock(), "xfam-audit: {msg}");
        }
        let mut facts = run.strict.clone();
        facts.fatal_error |= !stdout_ok;
        if strict_failed(&facts) {
            return ExitCode::from(3);
        }
    }
    match run.error {
        Some(msg) => died(&msg, stdout_ok, false),
        None if stdout_ok => ExitCode::SUCCESS,
        None => ExitCode::from(EXIT_FLUSH_FAILED),
    }
}
