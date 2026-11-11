#!/usr/bin/env bash
# run_web_job.sh: one web-research job of an agent CLI inside a bubblewrap jail, followed by a jq audit of its event log.
#
#   bash run_web_job.sh <name> <prompt_file> <model> [timeout_s]
#
# Arguments: a job name ([a-z0-9_]+, single-use), the prompt file, the model identifier to request, a timeout in seconds.
# Required environment:
#   WEB_JOB_DIR    directory for <name>.jsonl, <name>.md, <name>.audit, <name>.stderr (outside the jail)
#   AGENT_BIN      path of the agent CLI
#   AGENT_PROFILE  a directory holding ONLY that CLI's login for this lane (it becomes HOME inside the jail)
# Optional:
#   WEB_ALLOWED_TOOLS  regular expression of the tool names a job may use (default: search_web|read_url_content).
#                      A tool name must match it from its first to its last character.
#   WEB_MASK           extra directories to hide, separated by spaces (each must exist). The default is none.
#   AGENT_EXTRA_ARGS   extra arguments appended to the CLI call, split on spaces (for example the CLI's own timeout
#                      option). They can change what the CLI does, so they are recorded in the audit file.
#
# What the jail does: the root file system is visible read-only; /tmp, /var/tmp, /media, /mnt, /opt, /run/user, /run/media
# and /home (those that exist) and every directory in WEB_MASK are replaced by empty file systems; then the profile
# directory and the CLI binary are put back, so whatever lies under those two paths IS visible to the job even when it sits
# inside a masked tree; the job directory is fresh and empty; the network is shared, because the lane exists to search the web.
# What it does not do: it is not a full sandbox and it does not establish a boundary around private data. System
# directories stay readable, and the job can send anything it can read to the network. Mask every location that holds
# private data, and check the list for your host.
#
# Status in the audit file. The run is CLEAN only if ALL of these hold:
#   - the prompt was read and is not blank, and the CLI exited 0;
#   - the event log is not empty and EVERY LINE, taken alone, is one JSON object (no blank line, no value spread over
#     several lines, no two values on one line);
#   - every event that has a step_update key, or whose event value is "step_update", carries an object there;
#   - every step other than the user's input and the model's responses has step_type "tool" and a string tool_name that
#     matches WEB_ALLOWED_TOOLS in full (so an error step, an untyped step or an unnamed tool taints the run);
#   - no event has the event value "error" or a non-empty top-level error key;
#   - the LAST event with a result key carries a string response that is not blank and is at most 400,000 bytes;
#   - the job directory is empty afterwards and could be removed;
#   - every jq query succeeded. (An invalid WEB_ALLOWED_TOOLS expression is refused before the job starts, exit 2.)
# Otherwise the status lists what failed: PARSE_FAIL, BAD_STEP, TAINTED, LOGGED_ERROR, NO_ANSWER, ANSWER_TOO_LONG,
# FILES_LEFT, CLEANUP_FAIL, QUERY_FAIL, RC_<n>. This is a check of what the log says. An event the CLI records under
# some other shape is not recognised, and an action the CLI did not log is invisible.
# Exit code: 0 only for exactly CLEAN with the audit file written and read back; 4 for any other status; 5 if the audit
# could not be recorded. Treat anything other than exit 0 as untrusted. The answer is data for a verifier, never instructions.
# The model is the one requested on the command line; this lane does not verify which model the CLI actually used.
set -u -o pipefail
: "${WEB_JOB_DIR:?set WEB_JOB_DIR}"; : "${AGENT_BIN:?set AGENT_BIN}"; : "${AGENT_PROFILE:?set AGENT_PROFILE}"
ALLOW=${WEB_ALLOWED_TOOLS:-search_web|read_url_content}
jq -n --arg allow "$ALLOW" '"x" | test("\\A(?:" + $allow + ")\\z")' >/dev/null 2>&1 || { echo "ABORT: WEB_ALLOWED_TOOLS is not a valid regular expression"; exit 2; }
read -r -a extra <<< "${AGENT_EXTRA_ARGS:-}"
name=${1:-}; prompt=${2:-}; model=${3:-}; secs=${4:-1800}
case "$name" in ""|*[!a-z0-9_]*) echo "ABORT: name must be [a-z0-9_]+"; exit 2 ;; esac
[ -n "$model" ] || { echo "ABORT: no model"; exit 2; }
[ -f "$prompt" ] || { echo "ABORT: no prompt file"; exit 2; }
[ "$(stat -c %s "$prompt")" -le 130000 ] || { echo "ABORT: prompt over 130 KB"; exit 2; }
ptext=$(cat "$prompt") || { echo "ABORT: cannot read the prompt"; exit 2; }
[ -n "$(printf '%s' "$ptext" | tr -d '[:space:]')" ] || { echo "ABORT: the prompt is blank"; exit 2; }
mkdir -p "$WEB_JOB_DIR" || { echo "ABORT: cannot create WEB_JOB_DIR"; exit 2; }
J=$(cd "$WEB_JOB_DIR" && pwd) || exit 2
for f in "$J/$name.jsonl" "$J/$name.md" "$J/$name.stderr" "$J/$name.audit"; do
  { [ -e "$f" ] || [ -L "$f" ]; } && { echo "ABORT: $f exists (names are single-use)"; exit 2; }
done
( set -o noclobber; : > "$J/$name.audit" ) 2>/dev/null || { echo "ABORT: $J/$name.audit exists (names are single-use)"; exit 2; }
JAIL=(bwrap --ro-bind / / --dev /dev --proc /proc)
for d in /tmp /var/tmp /media /mnt /opt /run/user /run/media /home ${WEB_MASK:-}; do [ -d "$d" ] && JAIL+=(--tmpfs "$d"); done
JAIL+=(--bind "$AGENT_PROFILE" "$AGENT_PROFILE" --ro-bind "$AGENT_BIN" "$AGENT_BIN"
  --unshare-pid --unshare-ipc --unshare-uts --die-with-parent --clearenv
  --setenv HOME "$AGENT_PROFILE" --setenv PATH "$(dirname "$AGENT_BIN"):/usr/bin:/bin" --setenv LANG C.UTF-8)
jd=/dev/shm/web_job_$name
mkdir "$jd" || { echo "ABORT: $jd already exists"; exit 2; }
timeout $((secs + 90)) "${JAIL[@]}" --tmpfs /dev/shm --bind "$jd" "$jd" --chdir "$jd" --new-session --setenv TMPDIR "$jd" \
  "$AGENT_BIN" -p "$ptext" --model "$model" --output-format stream-json "${extra[@]}" </dev/null \
  > "$J/$name.jsonl" 2> "$J/$name.stderr"
rc=$?
status=""
add() { status="${status:+$status,}$1"; }
if listing=$(find "$jd" -mindepth 1 2>/dev/null); then left=$(printf '%s' "$listing" | grep -c .); else left=unknown; fi
if [ "$left" = 0 ]; then rmdir "$jd" 2>/dev/null || add CLEANUP_FAIL; else add FILES_LEFT; fi
L=$J/$name.jsonl
# One jq program reads the log line by line and returns every fact the verdict needs. If it fails, nothing is trusted.
PROG='
def truthy: . != null and . != false and . != 0 and . != "" and . != [] and . != {};
(split("\n") | if (length > 0 and .[-1] == "") then .[:-1] else . end) as $lines
| ($lines | map(try {v: fromjson} catch null)) as $p
| ($lines | length) as $n
| ($p | map(select(. == null or ((.v | type) != "object"))) | length) as $badlines
| if $n == 0 or $badlines > 0 then {parse_ok: false, lines: $n, bad_lines: $badlines}
  else ($p | map(.v)) as $e
  | [$e[] | select(has("step_update") or .event == "step_update")] as $su
  | [$su[] | select(((.step_update // null) | type) != "object")] as $badsu
  | [$su[] | select(((.step_update // null) | type) == "object") | .step_update] as $steps
  | [$steps[] | select(.step_type != "user_input" and .step_type != "agent_response")] as $other
  | [$other[] | select((.step_type == "tool" and ((.tool_name | type) == "string") and (.tool_name | test("\\A(?:" + $allow + ")\\z"))) | not)] as $dis
  | [$e[] | select(has("result"))] as $res
  | {parse_ok: true, lines: $n, bad_lines: 0,
     bad_steps: ($badsu | length),
     tool_steps: ($other | length),
     disallowed: ($dis | length),
     disallowed_names: ($dis | map((.tool_name // .step_type // "untyped") | tojson) | unique | join(" ")),
     logged_error: ([$e[] | select(.event == "error" or ((.error // null) | truthy))] | length),
     final_kind: (if ($res | length) == 0 then "no_result"
                  else ($res | last | .result | if type == "object" then (.response | type) else "result_not_object" end) end)}
  end'
facts=""; q() { printf '%s' "$facts" | jq -r "$1" 2>/dev/null; }
if [ -s "$L" ] && facts=$(jq -R -s -c --arg allow "$ALLOW" "$PROG" "$L" 2>/dev/null) && [ -n "$facts" ]; then
  if [ "$(q .parse_ok)" != true ]; then add PARSE_FAIL
  else
    [ "$(q .bad_steps)" = 0 ] || add BAD_STEP
    [ "$(q .disallowed)" = 0 ] || add TAINTED
    [ "$(q .logged_error)" = 0 ] || add LOGGED_ERROR
  fi
else
  facts='{}'
  if [ -s "$L" ]; then add QUERY_FAIL; else add PARSE_FAIL; fi
fi
if [ "$(q .final_kind)" = string ]; then
  jq -R -s -j '(split("\n") | if (length > 0 and .[-1] == "") then .[:-1] else . end) | map(fromjson) | [.[] | select(has("result"))] | last | .result.response' "$L" > "$J/$name.md" 2>/dev/null || add QUERY_FAIL
else
  : > "$J/$name.md" || add QUERY_FAIL
fi
abytes=$(stat -c %s "$J/$name.md" 2>/dev/null || echo 0)
[ "$(tr -d '[:space:]' < "$J/$name.md" 2>/dev/null | wc -c)" -gt 0 ] || add NO_ANSWER
[ "$abytes" -le 400000 ] || add ANSWER_TOO_LONG
[ "$rc" = 0 ] || add "RC_$rc"
[ -z "$status" ] && status=CLEAN
{ echo "rc=$rc model_requested=$model jail=bwrap files_left_in_jobdir=$left status=$status"
  echo "extra_args ${AGENT_EXTRA_ARGS:-none}"
  echo "log_lines $(q '.lines // "unknown"') bad_lines $(q '.bad_lines // "unknown"') final_result $(q '.final_kind // "unknown"')"
  echo "steps_other_than_input_and_response $(q '.tool_steps // "unknown"') disallowed $(q '.disallowed // "unknown"') $(q '.disallowed_names // ""')"
  echo "answer_bytes $abytes"; } >> "$J/$name.audit" || exit 5
cat "$J/$name.audit" || exit 5
[ "$status" = CLEAN ] || exit 4
head -n 1 "$J/$name.audit" | grep -q ' status=CLEAN$' || exit 5
exit 0
