#!/usr/bin/env bash
# run_text_job.sh: one text-only job of an agent CLI that prints a stream-JSON event log, followed by an audit of that log.
#
#   bash run_text_job.sh <name> <prompt_file> <model> [timeout_s]
#
# Required environment:
#   TEXT_JOB_DIR    directory for <name>.jsonl (event log), <name>.md (answer), <name>.audit, <name>.stderr
#   AGENT_BIN       path of the agent CLI (it must accept: -p <prompt> --model <m> --output-format stream-json)
#   XFAM_AUDIT_BIN  path of the xfam-audit binary built from crates/xfam-audit (jq is also required)
# Optional:
#   AGENT_HOME      HOME given to the agent CLI. The default is the caller's HOME, which gives the CLI everything in it:
#                   point this at a directory that holds only the CLI's login if that matters to you.
#   SCRATCH         a RAM-backed directory for the job's working directory (default: /dev/shm)
#   AGENT_EXTRA_ARGS extra arguments appended to the CLI call, split on spaces (for example the CLI's own timeout
#                   option). They can change what the CLI does, so they are recorded in the audit file.
#
# The prompt must carry all its text inline. The job runs in a fresh, empty working directory, and any file left there is
# counted. The audit file ends with "status=OK" only when: the prompt was read and is not empty, the CLI exited 0, the
# auditor in --strict mode exited 0 (no fatal error, zero tool-related events, no logged error, a final result with a
# non-blank string answer, no unparsed line), a jq check found every log line to be one JSON object with well-formed
# step updates, and
# nothing was left in the working directory. Otherwise it ends with "status=FAILED reasons=…" and the script exits 4.
# If the audit file itself cannot be written or read back, the script exits 5.
# Names are single-use: the audit file is created with an exclusive create, so two runs cannot share a name.
# The model is the one requested on the command line; this lane does not verify which model the CLI actually used.
#
# This wrapper records what the model did and refuses to vouch for a run it cannot read. It is NOT a sandbox: it does not
# stop the agent CLI from using whatever tools that CLI gives the model.
set -u -o pipefail
: "${TEXT_JOB_DIR:?set TEXT_JOB_DIR}"; : "${AGENT_BIN:?set AGENT_BIN}"; : "${XFAM_AUDIT_BIN:?set XFAM_AUDIT_BIN}"
name=${1:-}; prompt=${2:-}; model=${3:-}; secs=${4:-1800}
case "$name" in ""|*[!a-z0-9_]*) echo "ABORT: name must be [a-z0-9_]+"; exit 2 ;; esac
[ -n "$model" ] || { echo "ABORT: no model"; exit 2; }
[ -f "$prompt" ] || { echo "ABORT: no prompt file"; exit 2; }
ptext=$(cat "$prompt") || { echo "ABORT: cannot read the prompt"; exit 2; }
[ -n "$(printf '%s' "$ptext" | tr -d '[:space:]')" ] || { echo "ABORT: the prompt is empty"; exit 2; }
S=${SCRATCH:-/dev/shm}
read -r -a extra <<< "${AGENT_EXTRA_ARGS:-}"
mkdir -p "$TEXT_JOB_DIR" || { echo "ABORT: cannot create TEXT_JOB_DIR"; exit 2; }
J=$(cd "$TEXT_JOB_DIR" && pwd) || exit 2
for f in "$J/$name.jsonl" "$J/$name.md" "$J/$name.stderr" "$J/$name.audit"; do
  { [ -e "$f" ] || [ -L "$f" ]; } && { echo "ABORT: $f exists (names are single-use)"; exit 2; }
done
( set -o noclobber; : > "$J/$name.audit" ) 2>/dev/null || { echo "ABORT: $J/$name.audit exists (names are single-use)"; exit 2; }
cwd=$S/text_job_$name
mkdir "$cwd" || { echo "ABORT: $cwd already exists"; exit 2; }
cd "$cwd" || exit 2
timeout $((secs + 60)) env HOME="${AGENT_HOME:-$HOME}" TMPDIR="$S" \
  "$AGENT_BIN" -p "$ptext" --model "$model" --output-format stream-json "${extra[@]}" </dev/null \
  > "$J/$name.jsonl" 2> "$J/$name.stderr"
rc=$?
why=""
if listing=$(find "$cwd" -mindepth 1); then left=$(printf '%s' "$listing" | grep -c .); else left=unknown; why="$why cannot_list_cwd"; fi
echo "rc=$rc model_requested=$model files_left_in_cwd=$left extra_args=${AGENT_EXTRA_ARGS:-none}" >> "$J/$name.audit" || exit 5
[ "$left" = 0 ] && rmdir "$cwd"
cd "$S" || exit 5
env -i PATH=/usr/bin:/bin HOME=/nonexistent TMPDIR="$S" \
  "$XFAM_AUDIT_BIN" --strict "$J/$name.jsonl" "$J/$name.md" >> "$J/$name.audit" 2>> "$J/$name.stderr"
arc=$?
# Structure of the log, checked independently of the auditor: every line alone is one JSON object (no blank line),
# and every event that has a step_update key, or whose event value is "step_update", carries an object there.
jq -e -R -s '(split("\n") | if (length > 0 and .[-1] == "") then .[:-1] else . end) as $l
  | ($l | map(try {v: fromjson} catch null)) as $p
  | ($l | length) > 0 and all($p[]; . != null and ((.v | type) == "object"))
    and all($p[] | .v | select(has("step_update") or .event == "step_update"); ((.step_update // null) | type) == "object")' \
  "$J/$name.jsonl" >/dev/null 2>&1 || why="$why log_structure"
[ "$rc" = 0 ] || why="$why agent_rc_$rc"
[ "$arc" = 0 ] || why="$why audit_rc_$arc"
[ "$left" = 0 ] || why="$why files_left_$left"
grep -q '^tool_related_events 0$' "$J/$name.audit" || why="$why tool_events_not_zero_or_missing"
{ [ -f "$J/$name.md" ] && [ "$(tr -d '[:space:]' < "$J/$name.md" | wc -c)" -gt 0 ]; } || why="$why no_answer"
if [ -z "$why" ]; then
  echo "status=OK" >> "$J/$name.audit" || exit 5
  cat "$J/$name.audit" || exit 5
  [ "$(tail -n 1 "$J/$name.audit")" = "status=OK" ] || exit 5
  exit 0
fi
echo "status=FAILED reasons=$(echo $why | tr ' ' ',')" >> "$J/$name.audit" || exit 5
cat "$J/$name.audit" || exit 5
exit 4
