//! Byte-exact regression tests over seven invented stream-JSON transcripts.
//!
//! Each triple contains input, a hand-derived stdout reference after a synthetic shell
//! header, and exact answer bytes. The pinned case stores only a strict stdout prefix in
//! `.audit`; its complete hand-derived `.stdout` is still compared byte for byte.
//! A synthetic appended-reference test accepts only the listed prior byte length and an
//! exact complete final run. Mutation tests reject changes to either stdout or answer.
//! `full_directory_parity` optionally checks triples in `XFAM_AUDIT_JOBS_DIR`; unrelated
//! audit formats and inputs without a reference are skipped. Binary tests cover normal,
//! strict, fatal, and broken-output behaviour. Historical private data is not included.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use xfam_audit::{run, Run};

/// Neutral argv directory printed by the synthetic references.
const ORACLE_DIR: &str = "/jobs";

const FIXTURES: [&str; 6] = [
    "tool_shell_empty",  // 2 shell events, empty answer
    "tool_lookup_empty", // 2 lookup events, empty answer
    "recipe_answer",     // 1 error-message event, non-empty answer
    "tool_cap",          // 24 tool events: the 20-line cap
    "large_notes",       // 392141 bytes of invented geometry notes
    "unicode_answer",    // accents, CJK, emoji, and a combining mark
];

/// Cases whose stored reference is truncated: the complete expected stdout is the hand-derived pin
/// `<pin dir>/<name>.stdout` (tests/fixtures; provenance in PARITY.md), compared exactly.
const PINNED_STDOUT: [(&str, &str); 1] = [(
    "pinned_geometry",
    "synthetic stored prefix after the first tool line (491 of 6781 bytes)",
)];

/// Synthetic references with an exact-length prior output before the current one.
const PRIOR_RUN_REFS: [(&str, usize); 1] = [("prior_reference", 128)];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Match,
    /// listed in `PRIOR_RUN_REFS`: our stdout equals the reference's complete last run
    RefHasPriorRun { prior_bytes: usize },
    /// listed in `PINNED_STDOUT`: our stdout equals the complete hand-derived pin, and the stored
    /// (truncated) reference is a prefix of that pin
    MatchPinned { oracle_bytes: usize, pinned_bytes: usize },
    /// not the expected stream-auditor output format
    NotOracle,
    /// `.jsonl` without `.audit`: the job is still running
    InProgress,
    Mismatch(String),
}
fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

/// The directory the oracle got in argv, from the last "answer_chars … -> DIR/<name>.md" or
/// "answer NOT FOUND …; inspect DIR/<name>.jsonl" line of the reference.
fn argv_dir(expected: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(expected);
    text.lines().rev().find_map(|l| {
        let path = if l.starts_with("answer_chars ") {
            l.split_once(" -> ")?.1.strip_suffix(&format!("/{name}.md"))?
        } else {
            l.strip_prefix("answer NOT FOUND in known fields; inspect ")?
                .strip_suffix(&format!("/{name}.jsonl"))?
        };
        Some(path.to_string())
    })
}

fn check_case(dir: &Path, default_argv_dir: &str, name: &str) -> Verdict {
    check_case_with(dir, &fixtures_dir(), default_argv_dir, name, |_, _| {})
}

/// Compare one triple in `dir`; pins (`PINNED_STDOUT`) are read from `pin_dir`; argv paths
/// default to `default_argv_dir`. `tamper` may alter the port's run (stdout, exit status) and its
/// answer bytes before judging, to inject regressions.
fn check_case_with(
    dir: &Path,
    pin_dir: &Path,
    default_argv_dir: &str,
    name: &str,
    tamper: impl FnOnce(&mut Run, &mut Option<Vec<u8>>),
) -> Verdict {
    let read = |ext: &str| std::fs::read(dir.join(format!("{name}.{ext}")));
    let Ok(input) = read("jsonl") else {
        return Verdict::Mismatch("missing .jsonl".into());
    };
    let Ok(audit) = read("audit") else {
        return Verdict::InProgress;
    };
    let stored = match audit.iter().position(|&b| b == b'\n') {
        Some(nl) => &audit[nl + 1..],
        None => &[][..],
    };
    if !stored.is_empty() && !stored.starts_with(b"event_kinds ") {
        return Verdict::NotOracle;
    }
    // the expected stdout: the stored reference, or for a pinned case the complete pin, which
    // must extend the (truncated) stored reference
    let pin = if PINNED_STDOUT.iter().any(|(n, _)| *n == name) {
        let Ok(pin) = std::fs::read(pin_dir.join(format!("{name}.stdout"))) else {
            return Verdict::Mismatch(format!("pinned stdout {name}.stdout missing in {}", pin_dir.display()));
        };
        if stored.is_empty() || stored.len() >= pin.len() || !pin.starts_with(stored) {
            return Verdict::Mismatch(format!(
                "pin does not extend the stored reference (differs at byte {} of {})",
                first_diff(&pin, stored),
                stored.len()
            ));
        }
        Some(pin)
    } else {
        None
    };
    let expected = pin.as_deref().unwrap_or(stored);
    let argv = argv_dir(expected, name).unwrap_or_else(|| default_argv_dir.to_string());
    let src = format!("{argv}/{name}.jsonl");
    let dst = format!("{argv}/{name}.md");
    let mut answer = None;
    let mut r = run(&input[..], src.as_bytes(), dst.as_bytes(), |b| {
        answer = Some(b.to_vec());
        Ok(())
    });
    tamper(&mut r, &mut answer);

    if let Some(e) = &r.error {
        return Verdict::Mismatch(format!("port exits 1 ({e}); every case must exit 0"));
    }
    let Some(ours_md) = answer else {
        return Verdict::Mismatch("no answer file written".into());
    };
    match read("md") {
        Ok(md) if md == ours_md => {}
        Ok(md) => {
            return Verdict::Mismatch(format!(
                "answer differs from .md at byte {} (ours {} bytes, .md {} bytes)",
                first_diff(&ours_md, &md),
                ours_md.len(),
                md.len()
            ))
        }
        Err(_) => return Verdict::Mismatch("answer written but .md missing".into()),
    }

    let ours = &r.stdout[..];
    if ours == expected {
        return match &pin {
            Some(p) => Verdict::MatchPinned { oracle_bytes: stored.len(), pinned_bytes: p.len() },
            None => Verdict::Match,
        };
    }
    if let Some(&(_, prior)) = PRIOR_RUN_REFS.iter().find(|(n, _)| *n == name) {
        if expected.len() == prior + ours.len() && &expected[prior..] == ours && expected[..prior].ends_with(b"\n") {
            return Verdict::RefHasPriorRun { prior_bytes: prior };
        }
    }
    Verdict::Mismatch(format!(
        "stdout differs at byte {} (ours {} bytes, {} {} bytes)",
        first_diff(ours, expected),
        ours.len(),
        if pin.is_some() { "pin" } else { "reference" },
        expected.len()
    ))
}

#[test]
fn fixtures_match_oracle() {
    for name in FIXTURES {
        assert_eq!(check_case(&fixtures_dir(), ORACLE_DIR, name), Verdict::Match, "{name}");
    }
    assert_eq!(
        check_case(&fixtures_dir(), ORACLE_DIR, "pinned_geometry"),
        Verdict::MatchPinned { oracle_bytes: 491, pinned_bytes: 6781 }
    );
}

#[test]
fn synthetic_fixture_coverage() {
    let cases = [
        ("tool_shell_empty", 2, 0, 0),
        ("tool_lookup_empty", 2, 0, 0),
        ("recipe_answer", 1, 10, 10),
        ("tool_cap", 24, 9, 9),
        ("large_notes", 0, 15, 15),
        ("unicode_answer", 0, 11, 18),
        ("pinned_geometry", 22, 12, 12),
    ];
    let fx = fixtures_dir();
    let header = b"rc=0 model=example-model files_left_in_cwd=0\n";
    for (name, tools, chars, bytes) in cases {
        let input = std::fs::read(fx.join(format!("{name}.jsonl"))).unwrap();
        let audit = xfam_audit::audit(&input[..]);
        assert_eq!(audit.fatal, None, "{name}");
        assert_eq!(audit.tool_related_events, tools, "{name}");
        assert_eq!(audit.unparsed_lines, 0, "{name}");
        assert_eq!(audit.answer.unwrap().len(), chars, "{name}");
        let md = std::fs::read(fx.join(format!("{name}.md"))).unwrap();
        assert_eq!(md.len(), bytes, "{name}");
        let reference = std::fs::read(fx.join(format!("{name}.audit"))).unwrap();
        assert!(reference.starts_with(header), "{name}");
    }
    assert_eq!(std::fs::metadata(fx.join("large_notes.jsonl")).unwrap().len(), 392141);
    assert!(std::fs::metadata(fx.join("large_notes.jsonl")).unwrap().len() >= 150_000);
}

#[test]
fn pinned_stdout_shape() {
    // The stored prefix, then 19 more tool lines (20-line cap of 22 events), then the answer.
    // Every ASCII dump starts with a fixed object prefix followed by repeated "dot " filler.
    let pin = std::fs::read(fixtures_dir().join("pinned_geometry.stdout")).unwrap();
    let audit = std::fs::read(fixtures_dir().join("pinned_geometry.audit")).unwrap();
    let stored = &audit[audit.iter().position(|&b| b == b'\n').unwrap() + 1..];
    assert_eq!((stored.len(), pin.len()), (491, 6781));
    assert!(pin.starts_with(stored));
    let text = String::from_utf8(pin).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 23);
    assert_eq!(lines[1], "tool_related_events 22");
    let numbers: Vec<&str> = lines[2..22]
        .iter()
        .map(|l| l.strip_prefix("  line ").unwrap().split_once(" step_update:tool: ").unwrap().0)
        .collect();
    assert_eq!(numbers, ["4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23"]);
    for l in &lines[2..22] {
        let dump = l.split_once(" step_update:tool: ").unwrap().1;
        assert_eq!(dump.len(), 300, "every dump is longer than 300 chars and cut at 300");
    }
    assert_eq!(lines[22], format!("answer_chars 12 -> {ORACLE_DIR}/pinned_geometry.md"));
}

#[test]
fn full_directory_parity() {
    let Ok(dir) = std::env::var("XFAM_AUDIT_JOBS_DIR") else {
        eprintln!("full_directory_parity: skipped (XFAM_AUDIT_JOBS_DIR unset)");
        return;
    };
    let dir = dir.trim_end_matches('/').to_string();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read XFAM_AUDIT_JOBS_DIR")
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".jsonl").map(str::to_string))
        .collect();
    names.sort();
    let (mut total, mut matched, mut prior, mut pinned, mut skipped, mut real) = (0, 0, 0, 0, 0, 0);
    let mut report = Vec::new();
    for name in &names {
        let v = check_case(Path::new(&dir), &dir, name);
        match v {
            Verdict::Match => matched += 1,
            Verdict::RefHasPriorRun { .. } => prior += 1,
            Verdict::MatchPinned { .. } => pinned += 1,
            Verdict::NotOracle | Verdict::InProgress => skipped += 1,
            Verdict::Mismatch(_) => real += 1,
        }
        if !matches!(v, Verdict::NotOracle | Verdict::InProgress) {
            total += 1;
        }
        if v != Verdict::Match {
            report.push(format!("  {name}: {v:?}"));
        }
    }
    println!(
        "parity: {matched}/{total} oracle triples byte-identical, {prior} matched against the last run of a two-run reference, {pinned} matched exactly against a hand-derived pinned stdout extending a truncated reference, {real} mismatches; {skipped} of {} .jsonl files skipped; in {dir}",
        names.len()
    );
    for line in &report {
        println!("{line}");
    }
    assert_eq!(real, 0, "port/oracle mismatches:\n{}", report.join("\n"));
}

fn tmp(tag: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".test-output")
        .join(format!("xfam-audit-test-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

/// Byte offset just past the end of the `k`-th (1-based) printed tool-event line.
fn after_tool_line(stdout: &[u8], k: usize) -> usize {
    let mut seen = 0;
    let mut pos = 0;
    for line in stdout.split_inclusive(|&b| b == b'\n') {
        pos += line.len();
        if line.starts_with(b"  line ") {
            seen += 1;
            if seen == k {
                return pos;
            }
        }
    }
    panic!("fewer than {k} tool lines");
}

/// Same-length change to one byte in the middle of the `k`-th printed tool-event line.
fn flip_in_tool_line(stdout: &mut [u8], k: usize) {
    let end = after_tool_line(stdout, k);
    let start = stdout[..end - 1].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    let i = start + (end - start) / 2;
    stdout[i] = if stdout[i] == b'x' { b'y' } else { b'x' };
}

#[test]
fn injected_regressions_fail() {
    // Reject both a same-length dump change and a crash after a complete tool line.
    let fx = fixtures_dir();
    assert_eq!(check_case(&fx, ORACLE_DIR, "tool_cap"), Verdict::Match);
    let is_mismatch = |v: Verdict| matches!(v, Verdict::Mismatch(_));

    // 1. same-length change in the second tool-event dump of a covered case
    let v = check_case_with(&fx, &fx, ORACLE_DIR, "tool_cap", |r, _| flip_in_tool_line(&mut r.stdout, 2));
    assert!(is_mismatch(v));
    // 2. crash after the first tool line, before the answer is written
    let crash = |r: &mut Run, a: &mut Option<Vec<u8>>| {
        let cut = after_tool_line(&r.stdout, 1);
        r.stdout.truncate(cut);
        r.error = Some("injected".into());
        *a = None;
    };
    assert!(is_mismatch(check_case_with(&fx, &fx, ORACLE_DIR, "tool_cap", crash)));
    // exit 0 but no answer written; and a changed answer
    assert!(is_mismatch(check_case_with(&fx, &fx, ORACLE_DIR, "tool_cap", |_, a| *a = None)));
    assert!(is_mismatch(check_case_with(&fx, &fx, ORACLE_DIR, "tool_cap", |_, a| a.as_mut().unwrap().push(b' '))));

    // Changes beyond the stored prefix must also be rejected.
    let name = "pinned_geometry";
    let pinned = Verdict::MatchPinned { oracle_bytes: 491, pinned_bytes: 6781 };
    assert_eq!(check_case(&fx, ORACLE_DIR, name), pinned);
    let check = |tamper: &dyn Fn(&mut Run, &mut Option<Vec<u8>>)| check_case_with(&fx, &fx, ORACLE_DIR, name, tamper);
    // same-length change in the second tool line (after the stored 491 B)
    assert!(is_mismatch(check(&|r, _| {
        assert_eq!(after_tool_line(&r.stdout, 1), 491);
        flip_in_tool_line(&mut r.stdout, 2)
    })));
    // truncated to exactly the stored prefix, exit 0 and answer intact
    assert!(is_mismatch(check(&|r, _| r.stdout.truncate(491))));
    // an extra trailing line
    assert!(is_mismatch(check(&|r, _| r.stdout.extend_from_slice(b"extra\n"))));
    // the answer_chars line changed; the last byte dropped
    assert!(is_mismatch(check(&|r, _| {
        let n = r.stdout.len();
        r.stdout[n - 5] ^= 1
    })));
    assert!(is_mismatch(check(&|r, _| {
        r.stdout.pop();
    })));
    // a change inside the stored prefix; a crash after the first tool line; no answer
    assert!(is_mismatch(check(&|r, _| flip_in_tool_line(&mut r.stdout, 1))));
    assert!(is_mismatch(check(&|r, a| crash(r, a))));
    assert!(is_mismatch(check(&|_, a| *a = None)));

    // the pin itself: missing, not extending the stored reference, or a stored reference emptied
    let dir = tmp("inject");
    let d = dir.to_str().unwrap();
    for ext in ["jsonl", "audit", "md"] {
        std::fs::copy(fx.join(format!("{name}.{ext}")), dir.join(format!("{name}.{ext}"))).unwrap();
    }
    let pin_dir = dir.join("pins");
    std::fs::create_dir_all(&pin_dir).unwrap();
    let by_pin = || check_case_with(&dir, &pin_dir, ORACLE_DIR, name, |_, _| {});
    assert!(is_mismatch(by_pin()));
    let mut pin = std::fs::read(fx.join(format!("{name}.stdout"))).unwrap();
    std::fs::write(pin_dir.join(format!("{name}.stdout")), &pin).unwrap();
    assert_eq!(by_pin(), pinned);
    pin[100] ^= 1;
    std::fs::write(pin_dir.join(format!("{name}.stdout")), &pin).unwrap();
    assert!(is_mismatch(by_pin()));
    pin[100] ^= 1;
    pin[1000] ^= 1;
    std::fs::write(pin_dir.join(format!("{name}.stdout")), &pin).unwrap();
    assert!(is_mismatch(by_pin()));
    // A pin must strictly extend the stored bytes, not merely equal them.
    let reference = std::fs::read(fx.join(format!("{name}.audit"))).unwrap();
    let stored = &reference[reference.iter().position(|&b| b == b'\n').unwrap() + 1..];
    std::fs::write(pin_dir.join(format!("{name}.stdout")), stored).unwrap();
    assert!(is_mismatch(by_pin()));
    std::fs::write(dir.join(format!("{name}.audit")), b"rc=0 model=m files_left_in_cwd=0\n").unwrap();
    assert!(is_mismatch(check_case(&dir, d, name)));
    // a missing .md fails: restore the stored .audit (check_case reads the hand-derived pin from
    // tests/fixtures), confirm the triple passes, then drop only the .md, so the answer-file
    // check is the one that rejects it
    std::fs::copy(fx.join(format!("{name}.audit")), dir.join(format!("{name}.audit"))).unwrap();
    assert_eq!(check_case(&dir, d, name), pinned);
    std::fs::remove_file(dir.join(format!("{name}.md"))).unwrap();
    assert_eq!(check_case(&dir, d, name), Verdict::Mismatch("answer written but .md missing".into()));
    std::fs::remove_file(pin_dir.join(format!("{name}.stdout"))).ok();
    std::fs::remove_dir(&pin_dir).ok();

    // the two-run reference: only an exact match of the complete last run with the listed prior length
    let name = "prior_reference";
    let input = std::fs::read(fx.join("recipe_answer.jsonl")).unwrap();
    let md = std::fs::read(fx.join("recipe_answer.md")).unwrap();
    let reference = std::fs::read_to_string(fx.join("recipe_answer.audit")).unwrap();
    let current = reference.split_once('\n').unwrap().1
        .replace("/jobs/recipe_answer.md", "/jobs/prior_reference.md");
    let prior_output = concat!(
        "event_kinds {'init': 1} unparsed_lines 0\n",
        "tool_related_events 0\n",
        "answer NOT FOUND in known fields; inspect /jobs/prior_case.jsonl\n",
    ).as_bytes();
    assert_eq!(prior_output.len(), 128);
    let with_prior = |prior: usize| {
        let mut a = b"rc=0 model=example-model files_left_in_cwd=0\n".to_vec();
        a.extend_from_slice(&prior_output[..prior]);
        a.extend_from_slice(current.as_bytes());
        a
    };
    std::fs::write(dir.join(format!("{name}.jsonl")), &input).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), &md).unwrap();
    std::fs::write(dir.join(format!("{name}.audit")), with_prior(128)).unwrap();
    assert_eq!(check_case(&dir, d, name), Verdict::RefHasPriorRun { prior_bytes: 128 });
    assert!(is_mismatch(check_case_with(&dir, &fx, d, name, |r, _| flip_in_tool_line(&mut r.stdout, 1))));
    assert!(is_mismatch(check_case_with(&dir, &fx, d, name, crash)));
    std::fs::write(dir.join(format!("{name}.audit")), with_prior(127)).unwrap();
    assert!(is_mismatch(check_case(&dir, d, name)));

    for n in ["pinned_geometry", "prior_reference"] {
        for ext in ["jsonl", "audit", "md"] {
            std::fs::remove_file(dir.join(format!("{n}.{ext}"))).ok();
        }
    }
    std::fs::remove_dir(&dir).ok();
}

#[test]
fn parity_classification() {
    let dir = tmp("classify");
    let d = dir.to_str().unwrap();
    let put = |file: &str, bytes: &[u8]| std::fs::write(dir.join(file), bytes).unwrap();
    let verdict = |name: &str| check_case(&dir, d, name);

    // a running job is skipped
    put("running.jsonl", b"{\"event\":\"a\"}\n");
    assert_eq!(verdict("running"), Verdict::InProgress);

    let jsonl = b"{\"event\":\"a\",\"step_update\":{\"step_type\":\"t\"}}\n{\"result\":{\"response\":\"x\"}}\n";
    let head = "event_kinds {'a:t': 1, '?': 1} unparsed_lines 0\ntool_related_events 1\n";
    let tool = "  line 1 a:t: {\"event\": \"a\", \"step_update\": {\"step_type\": \"t\"}}\n";
    for name in ["whole", "cut", "twice"] {
        put(&format!("{name}.jsonl"), jsonl);
        put(&format!("{name}.md"), b"x");
    }
    let full = |name: &str| format!("{head}{tool}answer_chars 1 -> {ORACLE_DIR}/{name}.md\n");
    put("whole.audit", format!("rc=0\n{}", full("whole")).as_bytes());
    assert_eq!(verdict("whole"), Verdict::Match);
    // unlisted truncated or two-run references are mismatches
    put("cut.audit", format!("rc=0\n{head}{tool}").as_bytes());
    assert!(matches!(verdict("cut"), Verdict::Mismatch(_)));
    put("twice.audit", format!("rc=0\n{head}{}", full("twice")).as_bytes());
    assert!(matches!(verdict("twice"), Verdict::Mismatch(_)));

    // the oracle died before printing: a failure even though the port dies the same way
    put("died.jsonl", b"{\"event\":\"e\",\"step_update\":[1]}\n");
    put("died.audit", b"rc=0 model=m files_left_in_cwd=0\n");
    assert!(matches!(verdict("died"), Verdict::Mismatch(_)));
    // no answer found: identical stdout, but no answer file is a failure
    put("noans.jsonl", b"{}\n");
    let noans = format!("event_kinds {{'?': 1}} unparsed_lines 0\ntool_related_events 0\nanswer NOT FOUND in known fields; inspect {ORACLE_DIR}/noans.jsonl\n");
    put("noans.audit", format!("rc=0\n{noans}").as_bytes());
    assert!(matches!(verdict("noans"), Verdict::Mismatch(_)));

    for name in ["running", "whole", "cut", "twice", "died", "noans"] {
        for ext in ["jsonl", "audit", "md"] {
            std::fs::remove_file(dir.join(format!("{name}.{ext}"))).ok();
        }
    }
    std::fs::remove_dir(&dir).ok();
}

#[test]
fn binary_end_to_end() {
    let bin = env!("CARGO_BIN_EXE_xfam-audit");
    let dir = tmp("e2e");

    // a fixture through the real CLI: stdout and the written answer file
    let md = dir.join("recipe_answer.md");
    let src = fixtures_dir().join("recipe_answer.jsonl");
    let out = Command::new(bin).arg(&src).arg(&md).arg("ignored-extra").output().expect("run binary");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let audit = std::fs::read(fixtures_dir().join("recipe_answer.audit")).unwrap();
    let expected = String::from_utf8(audit).unwrap();
    let expected = expected
        .split_once('\n')
        .unwrap()
        .1
        .replace(&format!("{ORACLE_DIR}/recipe_answer.md"), md.to_str().unwrap());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), expected);
    assert_eq!(std::fs::read(&md).unwrap(), std::fs::read(fixtures_dir().join("recipe_answer.md")).unwrap());

    // surrogate in the answer: exit 1, stale answer file truncated to empty, no answer_chars line
    let bad = dir.join("bad.jsonl");
    std::fs::write(&bad, "{\"result\":{\"response\":\"x\\ud800\"}}\n").unwrap();
    std::fs::write(&md, "stale").unwrap();
    let out = Command::new(bin).arg(&bad).arg(&md).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"event_kinds {'?': 1} unparsed_lines 0\ntool_related_events 0\n");
    assert_eq!(std::fs::read(&md).unwrap(), b"");

    // no answer: the answer file is left alone
    std::fs::write(&bad, "{}\n").unwrap();
    std::fs::write(&md, "stale").unwrap();
    let out = Command::new(bin).arg(&bad).arg(&md).output().unwrap();
    assert!(out.status.success());
    let want = format!(
        "event_kinds {{'?': 1}} unparsed_lines 0\ntool_related_events 0\nanswer NOT FOUND in known fields; inspect {}\n",
        bad.display()
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), want);
    assert_eq!(std::fs::read(&md).unwrap(), b"stale");

    // missing input or arguments: exit 1, nothing on stdout
    let out = Command::new(bin).arg(dir.join("absent.jsonl")).arg(&md).output().unwrap();
    assert_eq!((out.status.code(), out.stdout.len()), (Some(1), 0));
    let out = Command::new(bin).arg(&bad).output().unwrap();
    assert_eq!((out.status.code(), out.stdout.len()), (Some(1), 0));

    // answer path in a missing directory: earlier lines are printed, then exit 1
    std::fs::write(&bad, "{\"result\":{\"response\":\"a\"}}\n").unwrap();
    let out = Command::new(bin).arg(&bad).arg(dir.join("no/such/dir.md")).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"event_kinds {'?': 1} unparsed_lines 0\ntool_related_events 0\n");

    for f in [md, bad] {
        std::fs::remove_file(f).ok();
    }
    std::fs::remove_dir(&dir).ok();
}

const LATER_NULL_INPUT: &[u8] = b"{\"result\":{\"response\":\"ok\"}}\n{\"result\":{\"response\":null}}\n";
const ERROR_EVENT_INPUT: &[u8] = b"{\"event\":\"error\"}\n{\"result\":{\"response\":\"ok\"}}\n";
// Hand-derived: absent event keys count as '?'; neither input has tools or unparsed lines.
// The retained answer is two code points, and the destination argument is relative.
const LATER_NULL_STDOUT: &[u8] = b"event_kinds {'?': 2} unparsed_lines 0\ntool_related_events 0\nanswer_chars 2 -> answer.md\n";
const ERROR_EVENT_STDOUT: &[u8] = b"event_kinds {'error': 1, '?': 1} unparsed_lines 0\ntool_related_events 0\nanswer_chars 2 -> answer.md\n";

fn assert_tiny_log_output(
    tag: &str, input: &[u8], strict: bool, stdout: &[u8], stderr: &[u8],
) {
    let dir = tmp(tag);
    let src = dir.join("input.jsonl");
    let dst = dir.join("answer.md");
    std::fs::write(&src, input).unwrap();
    std::fs::write(&dst, b"stale").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_xfam-audit"));
    command.current_dir(&dir);
    if strict { command.arg("--strict"); }
    let out = command.arg("input.jsonl").arg("answer.md").output().unwrap();
    assert_eq!(out.status.code(), Some(if strict { 3 } else { 0 }));
    assert_eq!(out.stdout, stdout);
    assert_eq!(out.stderr, stderr);
    assert_eq!(std::fs::read(&dst).unwrap(), b"ok");
    std::fs::remove_file(&src).unwrap();
    std::fs::remove_file(&dst).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn binary_strict_rejects_later_null_result() {
    assert_tiny_log_output(
        "strict-later-null", LATER_NULL_INPUT, true, LATER_NULL_STDOUT,
        b"strict: FAILED final_result_not_string\n",
    );
}

#[test]
fn binary_strict_rejects_logged_error() {
    assert_tiny_log_output(
        "strict-logged-error", ERROR_EVENT_INPUT, true, ERROR_EVENT_STDOUT,
        b"strict: FAILED logged_error\n",
    );
}

#[test]
fn binary_normal_preserves_later_null_and_logged_error() {
    assert_tiny_log_output("normal-later-null", LATER_NULL_INPUT, false, LATER_NULL_STDOUT, b"");
    assert_tiny_log_output("normal-logged-error", ERROR_EVENT_INPUT, false, ERROR_EVENT_STDOUT, b"");
}

#[test]
fn binary_strict_fixtures_preserve_output() {
    let bin = env!("CARGO_BIN_EXE_xfam-audit");
    let dir = tmp("strict-fixtures");
    for (name, reasons) in [
        ("tool_shell_empty", Some("tool_related_events,answer_blank")),
        ("tool_lookup_empty", Some("tool_related_events,answer_blank")),
        ("recipe_answer", Some("tool_related_events")),
        ("tool_cap", Some("tool_related_events")),
        ("large_notes", None),
        ("unicode_answer", None),
        ("pinned_geometry", Some("tool_related_events")),
    ] {
        let src = fixtures_dir().join(format!("{name}.jsonl"));
        let md = dir.join(format!("{name}.md"));
        let normal = Command::new(bin).arg(&src).arg(&md).output().unwrap();
        assert_eq!(normal.status.code(), Some(0), "{name}");
        assert_eq!(normal.stderr, b"", "{name}");
        let strict = Command::new(bin).arg("--strict").arg(&src).arg(&md).arg("ignored-extra").output().unwrap();
        assert_eq!(strict.stdout, normal.stdout, "{name}");
        assert_eq!(std::fs::read(&md).unwrap(), std::fs::read(fixtures_dir().join(format!("{name}.md"))).unwrap(), "{name}");
        assert_eq!(strict.status.code(), Some(if reasons.is_some() { 3 } else { 0 }), "{name}");
        let stderr = reasons.map_or_else(String::new, |r| format!("strict: FAILED {r}\n"));
        assert_eq!(strict.stderr, stderr.as_bytes(), "{name}");
        std::fs::remove_file(&md).unwrap();
    }
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn binary_strict_failures() {
    let bin = env!("CARGO_BIN_EXE_xfam-audit");
    let dir = tmp("strict-failures");
    let src = dir.join("input.jsonl");
    let md = dir.join("answer.md");
    let run = |strict: bool, dst: &Path| {
        let mut command = Command::new(bin);
        if strict { command.arg("--strict"); }
        command.arg(&src).arg(dst).output().unwrap()
    };
    for (input, reasons, normal_code, writes_answer) in [
        ("{}\n", "answer_not_found", 0, false),
        ("{\"result\":{\"response\":\"\"}}\n", "answer_blank", 0, true),
        ("{\"result\":{\"response\":\" \\t\\n\\u3000\\u001c\"}}\n", "answer_blank", 0, true),
        ("not json\n[]\n{\"result\":{\"response\":\"ok\"}}\n", "unparsed_lines", 0, true),
        ("{\"step_update\":[1]}\n", "fatal_error,answer_not_found", 1, false),
        ("{\"result\":{\"response\":\"x\\ud800\"}}\n", "fatal_error", 1, true),
    ] {
        std::fs::write(&src, input).unwrap();
        std::fs::write(&md, b"stale").unwrap();
        let normal = run(false, &md);
        assert_eq!(normal.status.code(), Some(normal_code));
        let answer = std::fs::read(&md).unwrap();
        if !writes_answer { assert_eq!(answer, b"stale"); }
        std::fs::write(&md, b"stale").unwrap();
        let strict = run(true, &md);
        assert_eq!(strict.status.code(), Some(3));
        assert_eq!(strict.stdout, normal.stdout);
        let mut stderr = normal.stderr;
        stderr.extend_from_slice(format!("strict: FAILED {reasons}\n").as_bytes());
        assert_eq!(strict.stderr, stderr);
        assert_eq!(std::fs::read(&md).unwrap(), answer);
    }
    // Fatal answer-write errors use the structured fatal fact too.
    std::fs::write(&src, "{\"result\":{\"response\":\"ok\"}}\n").unwrap();
    let absent_dst = dir.join("absent/answer.md");
    let normal = run(false, &absent_dst);
    let strict = run(true, &absent_dst);
    assert_eq!(normal.status.code(), Some(1));
    assert_eq!(strict.status.code(), Some(3));
    assert_eq!(strict.stdout, normal.stdout);
    let mut stderr = normal.stderr;
    stderr.extend_from_slice(b"strict: FAILED fatal_error\n");
    assert_eq!(strict.stderr, stderr);

    // Missing input and missing arguments fail closed before a library run exists.
    std::fs::remove_file(&src).unwrap();
    let strict = run(true, &md);
    assert_eq!(strict.status.code(), Some(3));
    assert_eq!(strict.stdout, b"");
    assert!(strict.stderr.starts_with(b"xfam-audit: cannot read "));
    assert!(strict.stderr.ends_with(b"strict: FAILED fatal_error,answer_not_found\n"));
    let strict = Command::new(bin).arg("--strict").output().unwrap();
    assert_eq!(strict.status.code(), Some(3));
    assert_eq!(strict.stdout, b"");
    assert_eq!(strict.stderr, b"xfam-audit: usage: xfam-audit [--strict] <jsonl> <answer.md>\nstrict: FAILED fatal_error,answer_not_found\n");
    // A later --strict remains an ignored extra argument.
    std::fs::write(&src, "{}\n").unwrap();
    let normal = Command::new(bin).arg(&src).arg(&md).arg("--strict").output().unwrap();
    assert_eq!(normal.status.code(), Some(0));
    assert_eq!(normal.stderr, b"");
    std::fs::remove_file(&src).unwrap();
    std::fs::remove_file(&md).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn binary_exit_status_with_broken_stdio() {
    // CPython exits 120 when it cannot flush stdout/stderr at exit, and
    // a broken stderr must not panic (exit 101)
    let full = Path::new("/dev/full");
    if !full.exists() {
        eprintln!("binary_exit_status_with_broken_stdio: skipped (no /dev/full)");
        return;
    }
    let sink = || Stdio::from(std::fs::OpenOptions::new().write(true).open(full).expect("open /dev/full"));
    let bin = env!("CARGO_BIN_EXE_xfam-audit");
    let dir = tmp("stdio");
    let (good, died, md) = (dir.join("good.jsonl"), dir.join("died.jsonl"), dir.join("a.md"));
    std::fs::write(&good, "{\"result\":{\"response\":\"ok\"}}\n").unwrap();
    std::fs::write(&died, "{\"event\":\"e\",\"step_update\":[1]}\n").unwrap();
    let status = |src: &Path, stdout: Stdio, stderr: Stdio| {
        Command::new(bin).arg(src).arg(&md).stdout(stdout).stderr(stderr).status().expect("run binary").code()
    };

    // output cannot be written: 120; the answer file is still written first
    std::fs::remove_file(&md).ok();
    assert_eq!(status(&good, sink(), Stdio::null()), Some(120));
    assert_eq!(std::fs::read(&md).unwrap(), b"ok");
    // died before printing anything: nothing to flush, so 1
    assert_eq!(status(&died, sink(), Stdio::null()), Some(1));
    // died, and its message cannot be written: 120, not a panic
    assert_eq!(status(&died, Stdio::null(), sink()), Some(120));
    // stderr broken but unused: 0
    assert_eq!(status(&good, Stdio::null(), sink()), Some(0));
    // missing arguments with a broken stderr: 120, not a panic
    let code = Command::new(bin).stdout(Stdio::null()).stderr(sink()).status().unwrap().code();
    assert_eq!(code, Some(120));

    let strict_status = |src: &Path, stdout: Stdio, stderr: Stdio| {
        Command::new(bin).arg("--strict").arg(src).arg(&md)
            .stdout(stdout).stderr(stderr).status().unwrap().code()
    };
    let out = Command::new(bin).arg("--strict").arg(&good).arg(&md)
        .stdout(sink()).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.stdout, b"");
    assert_eq!(out.stderr, b"strict: FAILED fatal_error\n");
    assert_eq!(std::fs::read(&md).unwrap(), b"ok");
    assert_eq!(strict_status(&died, Stdio::null(), sink()), Some(3));
    assert_eq!(strict_status(&good, Stdio::null(), sink()), Some(0));
    let code = Command::new(bin).arg("--strict").stdout(Stdio::null()).stderr(sink()).status().unwrap().code();
    assert_eq!(code, Some(3));

    for f in [good, died, md] {
        std::fs::remove_file(f).ok();
    }
    std::fs::remove_dir(&dir).ok();
}
