# xfam-audit

A small Rust tool and library, using only the standard library, that audits an agent CLI's
stream-JSON transcript. It counts event kinds, reports tool-related events, and writes the
last string-valued final answer to a separate UTF-8 file. The package is kept unpublished
with `publish = false`; the library crate is `xfam_audit` and the binary is `xfam-audit`.
The CLI currently uses Unix path-byte handling.

## Input

Supply one JSON object per line. Blank lines are ignored. Malformed JSON and non-object
values count as unparsed lines. Input is decoded as UTF-8 with replacement and accepts
LF, CRLF, and CR line endings. Object key order is retained. The parser also accepts the
reference's nonstandard numeric values `NaN`, `Infinity`, and `-Infinity`.

An invented example:

```json
{"event":"init"}
{"event":"step_update","step_update":{"step_type":"agent_response"}}
{"event":"result","result":{"response":"A square has four sides."}}
```

An event kind is `event` converted to reference-style text, or `?` if absent. A truthy
`step_update` appends `:` and its `step_type` (or `None` if absent). A truthy update whose
type is neither `user_input` nor `agent_response` counts as tool-related, including
`error_message` and unknown types. A truthy update must be an object or the audit fails.
Only a string in `result.response` is an answer; the last such string wins. Empty strings
are found answers in normal mode. Later non-string responses do not reset an earlier answer.

## Usage and output

```sh
cargo run --bin xfam-audit -- /jobs/example.jsonl /jobs/example.md
cargo run --bin xfam-audit -- --strict /jobs/example.jsonl /jobs/example.md
```

`--strict` is recognised only as the optional first argument. Further arguments are ignored.
Normal stdout has this form:

```text
event_kinds {'init': 1, 'step_update:agent_response': 1, 'result': 1} unparsed_lines 0
tool_related_events 0
answer_chars 24 -> /jobs/example.md
```

Kinds appear in first-seen order using reference-style dictionary representation. Between
the second and last lines, up to 20 tool lines appear as `  line N KIND: JSON`, where JSON
uses ASCII escapes and standard spaced separators and is cut at 300 characters. `N` counts
all input lines, including blanks and malformed lines. The tool count includes events
beyond the dump limit. `answer_chars` counts Unicode code points, including combining marks,
not bytes or displayed characters. The answer file contains exactly the answer, without an
added newline. If no answer is found, the last line is
`answer NOT FOUND in known fields; inspect /jobs/example.jsonl` and no answer file is touched.
Fatal errors may leave partial stdout. Encoding failure in an answer leaves an empty file.

Exit statuses:

- `0`: normal completion, including tools, unparsed lines, or a missing/empty answer unless
  strict mode is enabled.
- `1`: fatal input, parsing, encoding, argument, worker, or answer-write error in normal mode.
- `3`: strict rejection, after normal processing and output. Strict mode requires no fatal
  error, zero tool-related events, a found answer containing a non-whitespace character,
  zero unparsed lines, a string in `result.response` on the last event with a `result` key,
  and no logged error. Stderr adds exactly one `strict: FAILED <reasons>` line; reason tokens
  are comma-separated in this order: `fatal_error`, `tool_related_events`, `answer_not_found`
  or `answer_blank`, `unparsed_lines`, `final_result_not_string`, `logged_error`.
  `final_result_not_string` covers a null, missing, or non-string response, including a
  non-object result. No result event produces `answer_not_found` without adding
  `final_result_not_string`. `logged_error` means any parsed object has `event` equal to the
  string "error" or a truthy top-level `error`, using the same truthiness as `step_update`.
  Whitespace follows the reference's Unicode whitespace semantics, including control
  characters U+001C through U+001F.
- `120`: a required stdout/stderr write failed in normal mode. A stdout failure is a fatal
  error for strict mode; strict rejection remains `3` even if its diagnostic cannot be written.

## Limits and tests

This tool reports what the log contains. It is not a sandbox and cannot detect a tool call
that was never logged. It does not establish the truth or correctness of an answer.
The answer file is written before stdout. Input is streamed, but memory depends on the
longest line, distinct event kinds, and the retained answer. Nesting beyond 10,000 containers
is fatal; parsing and formatting deep input require a worker stack of `pyjson::STACK_BYTES`.
The binary allocates that stack; library callers must do so when accepting deeply nested input.

Run `cargo test` from the crate directory. This includes library tests, seven synthetic
byte-exact fixture comparisons, mutation tests, and binary tests for normal and strict modes.
Tests create scratch files under the crate's `.test-output` directory. The broken-output
check uses `/dev/full` when available. An optional `XFAM_AUDIT_JOBS_DIR=/jobs cargo test
full_directory_parity -- --nocapture` checks external triples using the same exact comparisons.
Each triple is a `.jsonl`, an answer `.md`, and an `.audit` whose first line is a shell
header such as `rc=0 model=example-model files_left_in_cwd=0`; remaining bytes are expected
stdout. Inputs without `.audit`, and references from another output format, are skipped.
A missing answer or answer reference is a failure. PARITY.md and REVIEW.md describe the
historical private comparison, which is not reproducible from this public crate.

Further limits:

- Strict mode is a policy about the log (no fatal error, zero tool-related events, a non-blank
  answer, zero unparsed lines, a string response in the last result, and no recognised logged
  error). It rejects a last result with a null, missing, or non-string response (or a non-object
  result), and any parsed object with `event` equal to "error" or a truthy top-level `error`,
  even next to a non-blank answer. The schema is otherwise not validated, and an error recorded
  under another key is not recognised. An `error_message` step is counted as tool-related
  and is rejected.
- The public fixtures are smaller than the private transcripts they replace (22 and 24
  tool-related events instead of about 90; short answers; a 392 KB largest input instead of 1 MB).
  The exact byte comparisons and the mutation tests are unchanged in kind.
- The optional directory test passes when the directory holds no checkable case, and it skips
  references it cannot classify. A pass of that test alone says nothing.
