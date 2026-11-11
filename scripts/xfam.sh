#!/usr/bin/env bash
# xfam.sh: run ONE cross-lab reviewer on a prompt file and print its answer only if the run can be trusted.
#
#   bash xfam.sh codex  <name> <prompt_file> [timeout_s]   # OpenAI Codex CLI, read-only sandbox over $XFAM_REPO
#   bash xfam.sh gemini <name> <prompt_file> [timeout_s]   # Google Gemini through an agent CLI, text-only, tool events audited
#
# Required environment:
#   XFAM_OUT          directory for <name>.md, <name>.stdout, <name>.meta
#   codex lane:  XFAM_REPO (directory the reviewer may read; its sandbox root), XFAM_CODEX_MODEL (exact model identifier)
#   gemini lane: XFAM_GEMINI_MODEL (exact model identifier) and the variables of run_text_job.sh
# Optional:
#   XFAM_RULES_FILE   a text file with standing rules that is put in front of every prompt
#
# Behaviour:
# - Names are single-use. <name>.stdout is created with an exclusive create before anything runs, so two runs cannot share
#   a name, and an existing <name>.md, .meta or .stdout aborts the run.
# - The prompt and the rules file are read before anything runs; a read failure or an empty prompt aborts.
# - The meta file is written last, in one step. Its first line carries status=OK or status=FAILED with the reasons.
# - status=OK needs: exit code 0 and a non-blank answer, and
#     codex lane:  the CLI reporting the same model identifier that was requested;
#     gemini lane: an audit that ended with status=OK and zero tool-related events. This lane records the model that was
#                  requested; it does not verify which model the CLI actually used.
# - The answer is printed, and the exit code is 0, only when status=OK and both the meta file and the answer could be
#   written and read back. A failed run exits 4; a failure to record or print the result exits 5.
# This script records and checks a run. It is not a security boundary.
set -u -o pipefail
FAM=${1:-}; NAME=${2:-}; PROMPT=${3:-}; SECS=${4:-1500}
: "${XFAM_OUT:?set XFAM_OUT}"
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
case "$NAME" in ""|*[!a-z0-9_]*) echo "ABORT: name must be [a-z0-9_]+"; exit 2 ;; esac
case "$FAM" in codex|gemini) ;; *) echo "Usage: bash $0 codex|gemini <name> <prompt_file> [timeout_s]"; exit 2 ;; esac
[ -f "$PROMPT" ] || { echo "ABORT: no prompt file"; exit 2; }
ptext=$(cat "$PROMPT") || { echo "ABORT: cannot read the prompt"; exit 2; }
[ -n "$(printf '%s' "$ptext" | tr -d '[:space:]')" ] || { echo "ABORT: the prompt is empty"; exit 2; }
RULE=""
if [ -n "${XFAM_RULES_FILE:-}" ]; then RULE=$(cat "$XFAM_RULES_FILE") || { echo "ABORT: cannot read XFAM_RULES_FILE"; exit 2; }; fi
mkdir -p "$XFAM_OUT" || { echo "ABORT: cannot create XFAM_OUT"; exit 2; }
OUT=$(cd "$XFAM_OUT" && pwd) || exit 2
for f in "$OUT/$NAME.md" "$OUT/$NAME.meta" "$OUT/$NAME.meta.tmp" "$OUT/$NAME.stdout"; do
  { [ -e "$f" ] || [ -L "$f" ]; } && { echo "ABORT: $f exists (names are single-use)"; exit 2; }
done
( set -o noclobber; : > "$OUT/$NAME.stdout" ) 2>/dev/null || { echo "ABORT: $OUT/$NAME.stdout exists (names are single-use)"; exit 2; }
nonblank() { [ -f "$1" ] && [ "$(tr -d '[:space:]' < "$1" | wc -c)" -gt 0 ]; }
status=FAILED; why=""
case "$FAM" in
  codex)
    : "${XFAM_REPO:?set XFAM_REPO}"; : "${XFAM_CODEX_MODEL:?set XFAM_CODEX_MODEL}"
    timeout "$SECS" codex exec --sandbox read-only -C "$XFAM_REPO" --skip-git-repo-check -m "$XFAM_CODEX_MODEL" \
      -o "$OUT/$NAME.md" "${RULE:+$RULE
}$ptext" </dev/null >> "$OUT/$NAME.stdout" 2>&1
    rc=$?
    seen=$(sed -n 's/^model: //p' "$OUT/$NAME.stdout" | head -n 1)
    [ "$rc" = 0 ] || why="$why rc_$rc"
    nonblank "$OUT/$NAME.md" || why="$why no_answer"
    [ "${seen:-}" = "$XFAM_CODEX_MODEL" ] || why="$why model_reported_${seen:-unknown}"
    [ -z "$why" ] && status=OK
    line="family=codex model=$XFAM_CODEX_MODEL model_reported=${seen:-unknown} rc=$rc status=$status${why:+ reasons=$(echo $why | tr ' ' ',')}"
    ;;
  gemini)
    : "${XFAM_GEMINI_MODEL:?set XFAM_GEMINI_MODEL}"; : "${TEXT_JOB_DIR:?set TEXT_JOB_DIR}"
    P=$OUT/$NAME.prompt
    { echo 'TEXT-ONLY TASK: do not call any tool, do not run commands, do not read or write files. Answer in plain text only.'
      [ -n "$RULE" ] && printf '%s\n' "$RULE"
      printf '%s\n' "$ptext"
      echo 'TEXT-ONLY TASK: answer in plain text only; no tools, no commands, no files.'; } > "$P" || { echo "ABORT: cannot write $P"; exit 5; }
    [ "$(stat -c %s "$P")" -le 100000 ] || { echo "ABORT: prompt over 100 KB"; exit 2; }
    bash "$HERE/run_text_job.sh" "xfam_$NAME" "$P" "$XFAM_GEMINI_MODEL" "$SECS" >> "$OUT/$NAME.stdout" 2>&1
    rc=$?
    A=$(cd "$TEXT_JOB_DIR" && pwd) || exit 5
    tools=$(grep -o 'tool_related_events [0-9]*' "$A/xfam_$NAME.audit" 2>/dev/null | awk '{print $2}' | head -n 1)
    [ "$rc" = 0 ] || why="$why rc_$rc"
    [ "$(tail -n 1 "$A/xfam_$NAME.audit" 2>/dev/null)" = "status=OK" ] || why="$why audit_not_ok"
    [ "${tools:-}" = 0 ] || why="$why tool_events_${tools:-unknown}"
    if [ -z "$why" ] && nonblank "$A/xfam_$NAME.md" && cp "$A/xfam_$NAME.md" "$OUT/$NAME.md"; then status=OK; else why="$why no_trusted_answer"; fi
    line="family=gemini model_requested=$XFAM_GEMINI_MODEL rc=$rc tool_events=${tools:-unknown} status=$status${why:+ reasons=$(echo $why | tr ' ' ',')}"
    ;;
esac
printf '%s\n' "$line" > "$OUT/$NAME.meta.tmp" || exit 5
mv "$OUT/$NAME.meta.tmp" "$OUT/$NAME.meta" || exit 5
cat "$OUT/$NAME.meta" || exit 5
if [ "$status" = OK ]; then
  echo "----- ANSWER -----"
  cat "$OUT/$NAME.md" || exit 5
  exit 0
fi
echo "(no trusted answer; see $OUT/$NAME.stdout)"; exit 4
