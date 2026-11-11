//! Stream auditing with the historical reference's text and JSON semantics.
//!
//! For each line (numbered from 1, universal newlines, `str.strip()`, blanks skipped):
//! `json.loads`; ValueError or a non-dict counts as unparsed. kind = `str(ev.get("event", "?"))`,
//! plus ":" + `str(step.get("step_type"))` when `ev.get("step_update")` is truthy. A tool event is
//! a truthy step_update whose step_type is not "user_input"/"agent_response"; the first 20 print
//! as `  line {n} {kind}: ` + `json.dumps(ev)[:300]`. The answer is the last `result.response`
//! that is a str. stdout:
//!   event_kinds {Counter as dict repr, first-seen order} unparsed_lines {bad}
//!   tool_related_events {count}
//!     line … (up to 20)
//!   answer_chars {len in code points} -> {dst}      or      answer NOT FOUND in known fields; inspect {src}
//!
//! Places where the oracle dies with an uncaught exception (exit 1), reproduced as `error`:
//! - the jsonl cannot be read (OSError), before anything is printed;
//! - step_update truthy but not a dict: AttributeError on `.get`, before anything is printed;
//! - nesting deeper than `pyjson::MAX_DEPTH`: CPython raises RecursionError only when its C
//!   stack runs out, at an unmeasured depth assumed to be above `MAX_DEPTH`. The port exits 1
//!   before anything is printed: the one case where it may refuse input the oracle accepts;
//! - a printed kind with a surrogate outside U+DC80..U+DCFF: UnicodeEncodeError on stdout; the
//!   lines before it are still flushed, the answer file is not touched;
//! - an answer with any surrogate: UnicodeEncodeError in `out.write`; `open(dst, "w")` already
//!   truncated the file, so it is left empty and "answer_chars" is not printed.
//!
//! The unused helper `strings()` in the oracle is dead code and is not ported.

use crate::pyjson::{dumps_dict, loads, py_str, repr_str, LoadError, Val};
use crate::pytext::{encode_strict, encode_surrogateescape, escape_cp, lines, py, py_strip, PyStr};
use std::collections::HashMap;
use std::io::BufRead;

/// Tool events printed in full (`tool_events[:20]`).
pub const MAX_TOOL_LINES: usize = 20;
/// `json.dumps(ev)[:300]`.
pub const TOOL_TEXT_CHARS: usize = 300;

/// What the oracle does up to (not including) touching the answer file.
#[derive(Debug, Default)]
pub struct Audit {
    /// stdout bytes written so far.
    pub stdout: Vec<u8>,
    /// Some(message): the oracle dies right after `stdout` (exit 1, answer file untouched).
    pub fatal: Option<String>,
    /// The last `result.response` str, if any.
    pub answer: Option<PyStr>,
    /// Number of tool-related events seen.
    pub tool_related_events: usize,
    /// Number of malformed or non-object lines seen.
    pub unparsed_lines: u64,
    /// Whether the last event with a result key lacks a string response.
    pub final_result_not_string: bool,
    /// Whether any parsed object records a recognised top-level error.
    pub logged_error: bool,
}

fn fatal(
    stdout: Vec<u8>, msg: String, tool_related_events: usize, unparsed_lines: u64,
    final_result_not_string: bool, logged_error: bool,
) -> Audit {
    Audit {
        stdout, fatal: Some(msg), answer: None, tool_related_events, unparsed_lines,
        final_result_not_string, logged_error,
    }
}

/// Run the oracle's loop and its first prints over the jsonl file's bytes, streamed line by
/// line. Memory also includes distinct kinds and the retained answer. Nesting up to
/// `pyjson::MAX_DEPTH` needs a big stack: call this on a thread of `pyjson::STACK_BYTES`.
pub fn audit(input: impl BufRead) -> Audit {
    let mut kinds: Vec<(PyStr, u64)> = Vec::new();
    let mut kind_index: HashMap<PyStr, usize> = HashMap::new();
    let mut tool_lines: Vec<(usize, PyStr, String)> = Vec::new();
    let mut tool_count = 0usize;
    let mut answer: Option<PyStr> = None;
    let mut bad = 0u64;
    let mut final_result_not_string = false;
    let mut logged_error = false;

    for (n0, raw) in lines(input).enumerate() {
        let n = n0 + 1;
        let raw = match raw {
            Ok(raw) => raw,
            Err(e) => return fatal(
                Vec::new(), format!("OSError reading line {n}: {e}"), tool_count, bad,
                final_result_not_string, logged_error,
            ),
        };
        let line = py_strip(&raw);
        if line.is_empty() {
            continue;
        }
        let ev = match loads(line) {
            Ok(Val::Dict(d)) => d,
            Ok(_) | Err(LoadError::Value) => {
                bad += 1;
                continue;
            }
            Err(LoadError::TooDeep) => {
                return fatal(Vec::new(), format!(
                    "line {n}: JSON nesting deeper than {} (CPython raises RecursionError at a larger, stack-dependent depth; not reproduced)",
                    crate::pyjson::MAX_DEPTH
                ), tool_count, bad, final_result_not_string, logged_error);
            }
        };
        if let Some(result) = ev.get("result") {
            final_result_not_string = !matches!(
                result, Val::Dict(result) if matches!(result.get("response"), Some(Val::Str(_)))
            );
        }
        logged_error |= matches!(ev.get("event"), Some(Val::Str(s)) if *s == py("error"))
            || ev.get("error").is_some_and(Val::truthy);
        let mut kind = ev.get("event").map_or_else(|| py("?"), py_str);
        let mut is_tool = false;
        if let Some(step) = ev.get("step_update").filter(|v| v.truthy()) {
            let Val::Dict(step) = step else {
                return fatal(Vec::new(), format!(
                    "line {n}: AttributeError: '{}' object has no attribute 'get'",
                    step.type_name()
                ), tool_count, bad, final_result_not_string, logged_error);
            };
            let step_type = step.get("step_type");
            kind.push(u32::from(b':'));
            kind.extend(step_type.map_or_else(|| py("None"), py_str));
            is_tool = !matches!(step_type, Some(Val::Str(s)) if *s == py("user_input") || *s == py("agent_response"));
        }
        match kind_index.get(&kind) {
            Some(&i) => kinds[i].1 += 1,
            None => {
                kind_index.insert(kind.clone(), kinds.len());
                kinds.push((kind.clone(), 1));
            }
        }
        if is_tool {
            tool_count += 1;
            if tool_lines.len() < MAX_TOOL_LINES {
                let mut text = String::new();
                dumps_dict(&ev, &mut text);
                text.truncate(TOOL_TEXT_CHARS); // ASCII, so bytes == code points
                tool_lines.push((n, kind, text));
            }
        }
        if let Some(Val::Dict(result)) = ev.get("result") {
            if let Some(Val::Str(s)) = result.get("response") {
                answer = Some(s.clone());
            }
        }
    }

    // print("event_kinds", dict(kinds), "unparsed_lines", bad)
    let mut first = py("event_kinds {");
    for (i, (k, count)) in kinds.iter().enumerate() {
        if i > 0 {
            first.extend(py(", "));
        }
        repr_str(k, &mut first);
        first.extend(py(&format!(": {count}")));
    }
    first.extend(py(&format!("}} unparsed_lines {bad}\ntool_related_events {tool_count}\n")));
    // repr() escapes every surrogate, so this cannot fail
    let mut stdout = encode_surrogateescape(&first).expect("repr output is encodable");

    for (n, kind, text) in tool_lines {
        let mut line = py(&format!("  line {n} "));
        line.extend(kind);
        line.extend(py(": "));
        line.extend(py(&text));
        match encode_surrogateescape(&line) {
            Ok(bytes) => {
                stdout.extend(bytes);
                stdout.push(b'\n');
            }
            Err(i) => {
                return fatal(stdout, format!(
                    "UnicodeEncodeError: 'utf-8' codec can't encode character '{}' in position {i}: surrogates not allowed",
                    escape_cp(line[i])
                ), tool_count, bad, final_result_not_string, logged_error);
            }
        }
    }
    Audit {
        stdout, fatal: None, answer, tool_related_events: tool_count, unparsed_lines: bad,
        final_result_not_string, logged_error,
    }
}

/// Facts used by the opt-in strict policy. No stdout parsing or I/O is needed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrictFacts {
    pub fatal_error: bool,
    pub tool_related_events: usize,
    pub answer_found: bool,
    pub answer_non_whitespace: bool,
    pub unparsed_lines: u64,
    /// False when no event has a result key, or the last such result has a string response.
    pub final_result_not_string: bool,
    pub logged_error: bool,
}

/// Stable, ordered reasons for rejection; an empty list means strict mode passes.
/// Blank means empty or entirely whitespace under the reference's text semantics.
pub fn strict_reasons(facts: &StrictFacts) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if facts.fatal_error {
        reasons.push("fatal_error");
    }
    if facts.tool_related_events != 0 {
        reasons.push("tool_related_events");
    }
    if !facts.answer_found {
        reasons.push("answer_not_found");
    } else if !facts.answer_non_whitespace {
        reasons.push("answer_blank");
    }
    if facts.unparsed_lines != 0 {
        reasons.push("unparsed_lines");
    }
    if facts.final_result_not_string {
        reasons.push("final_result_not_string");
    }
    if facts.logged_error {
        reasons.push("logged_error");
    }
    reasons
}

/// Everything observable from one oracle run except stderr text.
#[derive(Debug)]
pub struct Run {
    /// stdout bytes.
    pub stdout: Vec<u8>,
    /// Some(message): exit status 1 (uncaught exception in the oracle); None: exit status 0.
    pub error: Option<String>,
    /// Structured facts for strict policy evaluation.
    pub strict: StrictFacts,
}

impl Run {
    fn finish(stdout: Vec<u8>, error: Option<String>, mut strict: StrictFacts) -> Self {
        strict.fatal_error |= error.is_some();
        Self { stdout, error, strict }
    }
}

/// The oracle end to end over the jsonl file's bytes (`&[u8]` or a buffered file).
/// `write_answer` must create/truncate the answer file and write the given bytes; it is called
/// with no bytes when the answer cannot be encoded, because Python has already truncated the
/// file by then.
pub fn run(
    input: impl BufRead,
    src: &[u8],
    dst: &[u8],
    write_answer: impl FnOnce(&[u8]) -> std::io::Result<()>,
) -> Run {
    let Audit {
        mut stdout, fatal, answer, tool_related_events, unparsed_lines,
        final_result_not_string, logged_error,
    } = audit(input);
    let strict = StrictFacts {
        fatal_error: fatal.is_some(),
        tool_related_events,
        answer_found: answer.is_some(),
        answer_non_whitespace: answer.as_ref().is_some_and(|s| s.iter().any(|&cp| {
            !char::from_u32(cp).is_some_and(crate::pytext::is_py_space)
        })),
        unparsed_lines,
        final_result_not_string,
        logged_error,
    };
    if let Some(msg) = fatal {
        return Run::finish(stdout, Some(msg), strict);
    }
    let Some(answer) = answer else {
        stdout.extend_from_slice(b"answer NOT FOUND in known fields; inspect ");
        stdout.extend_from_slice(src);
        stdout.push(b'\n');
        return Run::finish(stdout, None, strict);
    };
    let (bytes, encode_error) = match encode_strict(&answer) {
        Ok(b) => (b, None),
        Err(i) => (Vec::new(), Some(format!(
            "UnicodeEncodeError: 'utf-8' codec can't encode character '{}' in position {i}: surrogates not allowed",
            escape_cp(answer[i])
        ))),
    };
    if let Err(e) = write_answer(&bytes) {
        return Run::finish(stdout, Some(format!("cannot write answer file: {e}")), strict);
    }
    if let Some(msg) = encode_error {
        return Run::finish(stdout, Some(msg), strict);
    }
    stdout.extend_from_slice(format!("answer_chars {} -> ", answer.len()).as_bytes());
    stdout.extend_from_slice(dst);
    stdout.push(b'\n');
    Run::finish(stdout, None, strict)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing_facts() -> StrictFacts {
        StrictFacts { answer_found: true, answer_non_whitespace: true, ..StrictFacts::default() }
    }

    #[test]
    fn strict_policy_passes() {
        assert_eq!(strict_reasons(&passing_facts()), Vec::<&str>::new());
    }

    #[test]
    fn strict_policy_rejects_fatal_error() {
        let facts = StrictFacts { fatal_error: true, ..passing_facts() };
        assert_eq!(strict_reasons(&facts), ["fatal_error"]);
    }

    #[test]
    fn strict_policy_rejects_tools() {
        let facts = StrictFacts { tool_related_events: 1, ..passing_facts() };
        assert_eq!(strict_reasons(&facts), ["tool_related_events"]);
    }

    #[test]
    fn strict_policy_rejects_missing_answer() {
        assert_eq!(strict_reasons(&StrictFacts::default()), ["answer_not_found"]);
    }

    #[test]
    fn strict_policy_rejects_blank_answer() {
        let facts = StrictFacts { answer_non_whitespace: false, ..passing_facts() };
        assert_eq!(strict_reasons(&facts), ["answer_blank"]);
    }

    #[test]
    fn strict_policy_rejects_unparsed_lines() {
        let facts = StrictFacts { unparsed_lines: 1, ..passing_facts() };
        assert_eq!(strict_reasons(&facts), ["unparsed_lines"]);
    }

    #[test]
    fn strict_policy_reports_all_reasons_in_order() {
        let facts = StrictFacts {
            fatal_error: true, tool_related_events: 2, unparsed_lines: 3,
            ..StrictFacts::default()
        };
        assert_eq!(strict_reasons(&facts), ["fatal_error", "tool_related_events", "answer_not_found", "unparsed_lines"]);
    }

    #[test]
    fn strict_rejects_final_result_not_string() {
        for result in [
            r#"{"response":null}"#, "{}", r#"{"response":0}"#,
            r#"{"response":false}"#, r#"{"response":[]}"#, r#"{"response":{}}"#,
            "null", "false", "0", r#""x""#, "[]",
        ] {
            let input = format!(
                "{{\"result\":{{\"response\":\"ok\"}}}}\n{{\"result\":{result}}}\n{{}}\n"
            );
            let mut written = Vec::new();
            let r = run(input.as_bytes(), b"s", b"d", |bytes| {
                written.extend_from_slice(bytes);
                Ok(())
            });
            assert_eq!(r.error, None, "{result}");
            assert_eq!(written, b"ok", "{result}");
            assert_eq!(strict_reasons(&r.strict), ["final_result_not_string"], "{result}");
        }
    }

    #[test]
    fn strict_rejects_logged_error() {
        for event in [
            r#"{"event":"error","error":false}"#, r#"{"error":true}"#,
            r#"{"error":1}"#, r#"{"error":-0.5}"#, r#"{"error":"x"}"#,
            r#"{"error":[0]}"#, r#"{"error":{"x":0}}"#, r#"{"error":NaN}"#,
            r#"{"error":Infinity}"#,
        ] {
            let input = format!("{event}\n{{\"result\":{{\"response\":\"ok\"}}}}\n{{\"error\":false}}\n");
            let r = run(input.as_bytes(), b"s", b"d", |_| Ok(()));
            assert_eq!(r.error, None, "{event}");
            assert_eq!(strict_reasons(&r.strict), ["logged_error"], "{event}");
        }
    }

    #[test]
    fn strict_accepts_final_string_without_logged_error() {
        for event in [
            "{}", r#"{"error":null}"#, r#"{"error":false}"#, r#"{"error":0}"#,
            r#"{"error":-0.0}"#, r#"{"error":""}"#, r#"{"error":[]}"#,
            r#"{"error":{}}"#, r#"{"event":"errors"}"#,
            r#"{"event":{"error":true}}"#, r#"{"other":{"error":true}}"#,
            r#"{"other_error":true}"#,
        ] {
            let input = format!(
                "{{\"result\":null}}\n{{\"result\":{{\"response\":\"ok\"}}}}\n{event}\n"
            );
            let r = run(input.as_bytes(), b"s", b"d", |_| Ok(()));
            assert_eq!(r.error, None, "{event}");
            assert!(strict_reasons(&r.strict).is_empty(), "{event}");
        }
        let r = run(&b"{}\n"[..], b"s", b"d", |_| panic!("no answer"));
        assert_eq!(strict_reasons(&r.strict), ["answer_not_found"]);
    }

    #[test]
    fn strict_reports_new_reasons_after_existing_reasons() {
        for answer_found in [false, true] {
            let facts = StrictFacts {
                fatal_error: true, tool_related_events: 1, answer_found,
                answer_non_whitespace: false, unparsed_lines: 1,
                final_result_not_string: true, logged_error: true,
            };
            assert_eq!(strict_reasons(&facts), [
                "fatal_error", "tool_related_events",
                if answer_found { "answer_blank" } else { "answer_not_found" },
                "unparsed_lines", "final_result_not_string", "logged_error",
            ]);
        }
    }

    #[test]
    fn strict_new_facts_survive_fatal_paths() {
        for (input, write_fails) in [
            ("{\"event\":\"error\",\"result\":null,\"step_update\":[1]}\n", false),
            (concat!(
                "{\"event\":\"error\",\"result\":null}\n",
                "{\"event\":\"\\ud800\",\"step_update\":{\"step_type\":\"t\"}}\n",
            ), false),
            (concat!(
                "{\"result\":{\"response\":\"\\ud800\"}}\n",
                "{\"event\":\"error\",\"result\":null}\n",
            ), false),
            (concat!(
                "{\"result\":{\"response\":\"ok\"}}\n",
                "{\"event\":\"error\",\"result\":null}\n",
            ), true),
        ] {
            let r = run(input.as_bytes(), b"s", b"d", |_| {
                if write_fails { Err(std::io::Error::other("injected")) } else { Ok(()) }
            });
            assert!(r.error.is_some());
            assert!(r.strict.fatal_error);
            assert!(r.strict.final_result_not_string);
            assert!(r.strict.logged_error);
            assert!(strict_reasons(&r.strict).ends_with(&["final_result_not_string", "logged_error"]));
        }
    }

    #[test]
    fn strict_facts_follow_parsing_and_answer_writes() {
        for answer in ["", " \\t\\n\\u3000\\u001c", "x"] {
            let input = format!("{{\"result\":{{\"response\":\"{answer}\"}}}}\n");
            let r = run(input.as_bytes(), b"s", b"d", |_| Ok(()));
            assert_eq!(r.error, None);
            assert!(r.strict.answer_found);
            assert_eq!(r.strict.answer_non_whitespace, answer == "x");
        }
        let r = run(&b"{}\nnot json\n[]\n"[..], b"s", b"d", |_| Ok(()));
        assert_eq!(r.strict.unparsed_lines, 2);
        assert!(!r.strict.answer_found);
        let input = &b"{\"result\":{\"response\":\"x\"}}\n"[..];
        let r = run(input, b"s", b"d", |_| Err(std::io::Error::other("injected")));
        assert_eq!(strict_reasons(&r.strict), ["fatal_error"]);
        let r = run(&b"{\"step_update\":[1]}\n"[..], b"s", b"d", |_| Ok(()));
        assert!(r.strict.fatal_error);
    }

    /// (stdout, answer file bytes if written, exit error)
    fn go(input: &str) -> (String, Option<Vec<u8>>, Option<String>) {
        go_bytes(input.as_bytes())
    }

    fn go_bytes(input: &[u8]) -> (String, Option<Vec<u8>>, Option<String>) {
        let mut file = None;
        let r = run(input, b"/jobs/x.jsonl", b"/jobs/x.md", |b| {
            file = Some(b.to_vec());
            Ok(())
        });
        (String::from_utf8_lossy(&r.stdout).into_owned(), file, r.error)
    }

    #[test]
    fn typical_transcript() {
        let t = concat!(
            "{\"event\":\"init\",\"init\":{\"tools\":[\"a\"]}}\n",
            "{\"event\":\"step_update\",\"step_update\":{\"step_type\":\"user_input\"}}\n",
            "\n   \n",
            "{\"event\":\"step_update\",\"step_update\":{\"step_type\":\"tool\",\"x\":1.5e-7,\"y\":\"\\u00e9\"}}\n",
            "not json\n",
            "[1,2]\n",
            "{\"event\":\"result\",\"result\":{\"response\":\"caf\\u00e9 \\ud83d\\ude00\"}}\n",
        );
        let (out, file, err) = go(t);
        assert_eq!(err, None);
        assert_eq!(out, concat!(
            "event_kinds {'init': 1, 'step_update:user_input': 1, 'step_update:tool': 1, 'result': 1} unparsed_lines 2\n",
            "tool_related_events 1\n",
            "  line 5 step_update:tool: {\"event\": \"step_update\", \"step_update\": {\"step_type\": \"tool\", \"x\": 1.5e-07, \"y\": \"\\u00e9\"}}\n",
            "answer_chars 6 -> /jobs/x.md\n",
        ));
        assert_eq!(file.unwrap(), "café 😀".as_bytes());
    }

    #[test]
    fn no_answer_and_empty_input() {
        let (out, file, err) = go("");
        assert_eq!(out, "event_kinds {} unparsed_lines 0\ntool_related_events 0\nanswer NOT FOUND in known fields; inspect /jobs/x.jsonl\n");
        assert_eq!((file, err), (None, None));
        // result.response must be a str; the last str wins, later non-str values do not reset it
        let (out, file, _) = go("{\"result\":{\"response\":\"a\"}}\n{\"result\":{\"response\":\"bb\"}}\n{\"result\":{\"response\":3}}\n{\"result\":\"x\"}");
        assert!(out.ends_with("answer_chars 2 -> /jobs/x.md\n"), "{out}");
        assert_eq!(file.unwrap(), b"bb");
        let (out, _, _) = go("{\"result\":{\"response\":null}}");
        assert!(out.ends_with("answer NOT FOUND in known fields; inspect /jobs/x.jsonl\n"));
    }

    #[test]
    fn kinds_use_python_str_and_repr() {
        let t = concat!(
            "{\"x\":1}\n",
            "{\"event\":null}\n",
            "{\"event\":true}\n",
            "{\"event\":1e16}\n",
            "{\"event\":-0}\n",
            "{\"event\":{\"k\":[1,\"it's\",NaN]}}\n",
            "{\"event\":\"a\",\"step_update\":{\"no_type\":1}}\n",
            "{\"event\":\"a\",\"step_update\":{}}\n",
            "{\"event\":\"a\",\"step_update\":0}\n",
            "{\"event\":\"a\",\"step_update\":{\"step_type\":[\"t\"]}}\n",
            "{\"event\":\"tab\\tq'\",\"step_update\":{\"step_type\":\"agent_response\"}}\n",
            "{\"event\":\"a\",\"event\":\"b\"}\n",
            "{\"event\":\"x\"}\n",
        );
        let (out, _, err) = go(t);
        assert_eq!(err, None);
        let first = out.lines().next().unwrap();
        assert_eq!(first, concat!(
            "event_kinds {'?': 1, 'None': 1, 'True': 1, '1e+16': 1, '0': 1, ",
            // the kind holds both quote kinds, so repr uses ' and escapes it
            "'{\\'k\\': [1, \"it\\'s\", nan]}': 1, 'a:None': 1, 'a': 2, \"a:['t']\": 1, ",
            "\"tab\\tq':agent_response\": 1, 'b': 1, 'x': 1} unparsed_lines 0",
        ));
        // tool events: a step_update without step_type and one with a list step_type
        assert!(out.contains("tool_related_events 2\n  line 7 a:None: {\"event\": \"a\", \"step_update\": {\"no_type\": 1}}\n"), "{out}");
        assert!(out.contains("  line 10 a:['t']: "), "{out}");
    }

    #[test]
    fn tool_lines_capped_and_truncated() {
        let long = "z".repeat(400);
        let mut t = String::new();
        for _ in 0..25 {
            t.push_str(&format!("{{\"event\":\"s\",\"step_update\":{{\"step_type\":\"tool\",\"p\":\"{long}\"}}}}\r\n"));
        }
        let (out, _, _) = go(&t);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1], "tool_related_events 25");
        assert_eq!(lines.len(), 2 + 20 + 1);
        let prefix = "  line 20 s:tool: ";
        assert!(lines[21].starts_with(prefix));
        assert_eq!(lines[21].len(), prefix.len() + TOOL_TEXT_CHARS);
    }

    #[test]
    fn python_whitespace_and_line_numbers() {
        // \x1c..\x1f and U+3000 are stripped by str.strip(); \r alone ends a line
        let t = "\u{3000}\x1c{\"event\":\"a\",\"step_update\":{\"step_type\":\"t\"}}\x1f\r{\"event\":\"b\",\"step_update\":{\"step_type\":\"t\"}}";
        let (out, _, _) = go(t);
        assert!(out.contains("  line 1 a:t: "), "{out}");
        assert!(out.contains("  line 2 b:t: "), "{out}");
        // invalid UTF-8 is replaced, so the line is still JSON
        let (out, _, _) = go_bytes(b"{\"event\":\"\xff\",\"step_update\":{\"step_type\":\"t\"}}\n");
        assert!(out.starts_with("event_kinds {'\u{fffd}:t': 1}"), "{out}");
        assert!(out.contains("  line 1 \u{fffd}:t: {\"event\": \"\\ufffd\""), "{out}");
    }

    #[test]
    fn oracle_crashes_are_reproduced() {
        // AttributeError before any print
        let (out, file, err) = go("{\"result\":{\"response\":\"a\"}}\n{\"event\":\"e\",\"step_update\":[1]}\n");
        assert_eq!((out.as_str(), file), ("", None));
        assert!(err.unwrap().contains("'list' object has no attribute 'get'"));
        // surrogate in a printed kind: earlier lines survive, no answer file
        let r = run(
            &b"{\"event\":\"a\",\"step_update\":{\"step_type\":\"t\"}}\n{\"event\":\"\\ud800\",\"step_update\":{\"step_type\":\"t\"}}\n{\"result\":{\"response\":\"x\"}}"[..],
            b"s", b"d", |_| panic!("answer must not be written"),
        );
        assert_eq!(String::from_utf8(r.stdout).unwrap(), concat!(
            "event_kinds {'a:t': 1, '\\ud800:t': 1, '?': 1} unparsed_lines 0\n",
            "tool_related_events 2\n",
            "  line 1 a:t: {\"event\": \"a\", \"step_update\": {\"step_type\": \"t\"}}\n",
        ));
        assert!(r.error.unwrap().contains("surrogates not allowed"));
        // U+DC80..U+DCFF go out as raw bytes through surrogateescape
        let r = run(&b"{\"event\":\"\\udcff\",\"step_update\":{\"step_type\":\"t\"}}"[..], b"s", b"d", |_| Ok(()));
        assert_eq!(r.error, None);
        assert!(r.stdout.windows(4).any(|w| w == b"\xff:t:"));
        // surrogate in the answer: empty file, no answer_chars line, exit 1
        let (out, file, err) = go("{\"result\":{\"response\":\"ok\\udc80\"}}");
        assert_eq!(out, "event_kinds {'?': 1} unparsed_lines 0\ntool_related_events 0\n");
        assert_eq!(file, Some(Vec::new()));
        assert!(err.unwrap().contains("surrogates not allowed"));
    }

    #[test]
    fn answer_length_counts_code_points() {
        let (out, file, _) = go("{\"result\":{\"response\":\"\\u00e9\\ud83d\\ude00\\n\\r\\n\"}}");
        assert!(out.ends_with("answer_chars 5 -> /jobs/x.md\n"), "{out}");
        // text mode on Linux writes "\n" and "\r" unchanged
        assert_eq!(file.unwrap(), "é😀\n\r\n".as_bytes());
    }

    #[test]
    fn big_int_line_is_unparsed() {
        let t = format!("{{\"event\":{}}}\n{{\"event\":{}}}\n", "7".repeat(4301), "7".repeat(4300));
        let (out, _, _) = go(&t);
        assert!(out.starts_with(&format!("event_kinds {{'{}': 1}} unparsed_lines 1\n", "7".repeat(4300))), "{}", &out[..80]);
    }

    #[test]
    fn float_ties_print_as_cpython() {
        // Regression for CPython's expected stdout
        let (out, _, err) = go("{\"event\":1.00000762939453125,\"step_update\":{\"step_type\":\"t\",\"d\":562949953421312.25}}\n");
        assert_eq!(err, None);
        assert_eq!(out, concat!(
            "event_kinds {'1.0000076293945312:t': 1} unparsed_lines 0\n",
            "tool_related_events 1\n",
            "  line 1 1.0000076293945312:t: {\"event\": 1.0000076293945312, \"step_update\": {\"step_type\": \"t\", \"d\": 562949953421312.2}}\n",
            "answer NOT FOUND in known fields; inspect /jobs/x.jsonl\n",
        ));
    }

    #[test]
    fn deep_nesting_up_to_max_depth_matches_the_oracle() {
        use crate::pyjson::{MAX_DEPTH, STACK_BYTES};
        std::thread::Builder::new()
            .stack_size(STACK_BYTES)
            .spawn(|| {
                // Depth regression: a malformed line 1001 levels deep is only unparsed
                let (out, file, err) = go(&format!("{{\"result\":{{\"response\":\"ok\"}}}}\n{}\n", "[".repeat(1001)));
                assert_eq!(err, None);
                assert_eq!(out, "event_kinds {'?': 1} unparsed_lines 1\ntool_related_events 0\nanswer_chars 2 -> /jobs/x.md\n");
                assert_eq!(file.unwrap(), b"ok");
                // a valid tool event MAX_DEPTH deep (the dict is level 1): repr in the kind, dumps cut at 300
                let k = MAX_DEPTH - 1;
                let line = format!("{{\"event\":{}0{},\"step_update\":{{\"step_type\":\"t\"}}}}", "[".repeat(k), "]".repeat(k));
                let (out, _, err) = go(&line);
                assert_eq!(err, None);
                let kind = format!("{}0{}:t", "[".repeat(k), "]".repeat(k));
                let tool = format!("{{\"event\": {}", "[".repeat(300 - 10)); // `{"event": ` is 10 chars
                assert_eq!(out, format!(
                    "event_kinds {{'{kind}': 1}} unparsed_lines 0\ntool_related_events 1\n  line 1 {kind}: {tool}\nanswer NOT FOUND in known fields; inspect /jobs/x.jsonl\n"
                ));
                // one level more: the port refuses loudly, nothing printed, answer untouched
                let line = format!("{{\"result\":{{\"response\":\"ok\"}},\"event\":{}0{}}}", "[".repeat(k + 1), "]".repeat(k + 1));
                let (out, file, err) = go(&line);
                assert_eq!((out.as_str(), file), ("", None));
                assert!(err.unwrap().contains("nesting deeper than"));
            })
            .expect("spawn")
            .join()
            .expect("deep nesting thread");
    }

    #[test]
    fn read_error_is_fatal_before_any_output() {
        // Streaming regression: input is streamed, so a read error can come after good lines;
        // like the oracle's OSError it ends the run before anything is printed
        struct Broken;
        impl std::io::Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk gone"))
            }
        }
        let input = std::io::Read::chain(
            &b"{\"result\":{\"response\":\"a\"}}\n{\"event\":\"error\",\"result\":null}\n{\"x\":1}"[..], Broken,
        );
        let r = run(std::io::BufReader::new(input), b"s", b"d", |_| panic!("answer must not be written"));
        assert_eq!(r.stdout, b"");
        assert!(r.error.unwrap().contains("disk gone"));
        assert!(r.strict.final_result_not_string);
        assert!(r.strict.logged_error);
    }
}
