# Historical compatibility record

In the private project, the recorded comparison covered 65 private transcripts against
outputs from a Python reference implementation using CPython 3.14 in UTF-8 mode.
There were no reported mismatches under the recorded comparison rules: answer files
matched byte for byte, and stdout was compared byte for byte. Of these, 63 matched
the complete stored stdout directly. One stored reference contained an appended prior
run; only the complete final run matched. One stored stdout was truncated; its complete
expected stdout was derived independently and pinned, with the stored bytes required
to be a prefix. That pin was partly derived evidence, not a complete reference capture.

This is recorded history, not a reproducible public experiment. Neither the reference
implementation nor the private transcripts are included. The seven public fixtures are
invented, and their expected bytes were derived by hand from the source semantics.
They do not provide independent evidence for the historical claim.

Documented differences and limits:

- Fatal diagnostics are one line instead of a reference traceback. Normal fatal exit
  status is 1; failed stdout/stderr writes use 120. Strict mode is a new opt-in policy.
- The answer file is written before stdout is emitted. With a broken stdout and enough
  buffered output, the reference could fail before touching the answer file instead.
- Nesting beyond 10,000 containers is fatal here. The reference's stack-dependent
  recursion limit was not measured, for valid or malformed input beyond that depth.
- An unused string-walking helper was omitted. Input is streamed, but memory still
  depends on line length, distinct event kinds, and the retained answer.
- Float ties use the even final digit. String representation uses Unicode 16 tables;
  file and stdout encoding preserve the reference's distinct surrogate rules.

The synthetic pin retains a useful harness invariant: the 491 stored stdout bytes must
strictly prefix the full 6,781-byte pin, and actual stdout must equal the whole pin.
Its 22 tool events produce 20 dumps, each cut at exactly 300 ASCII characters.
The appended-reference regression is generated synthetically with a 128-byte prior run.
