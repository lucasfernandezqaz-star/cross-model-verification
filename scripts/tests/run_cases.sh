#!/usr/bin/env bash
# run_cases.sh: runs the web lane and the text lane against fake_agent.sh, which prints canned event logs, and checks
# the exit code of every case: only the good logs may be accepted. No network and no model are used. Needs bwrap and jq;
# the text lane also needs the xfam-audit binary:
#   XFAM_AUDIT_BIN=/path/to/xfam-audit bash scripts/tests/run_cases.sh
# What it covers: the shape of the event log (cases named in fake_agent.sh), an invalid allow-list expression with and
# without tool steps, a blank prompt, an unreadable prompt file (skipped when run as root) and a repeated job name.
# What it does not cover: a result file that cannot be written, a failure to list or remove the job directory, and the
# Codex lane. It checks exit codes, not the reasons printed.
# The jail hides /home and /tmp but puts the agent binary and the profile directory back, so the fake agent is copied to
# a new scratch directory under HOME. Everything this script removes at the end was created by it: two mktemp directories
# and job directories whose names carry this run's process id.
# Exit code 0 only if every case behaved as expected.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
S=$(dirname "$HERE")
W=$(mktemp -d "$HOME/.xfam_cases.XXXXXX") || exit 2
T=$(mktemp -d /dev/shm/xfam_cases.XXXXXX) || { rmdir "$W"; exit 2; }
mkdir "$W/profile" && cp "$HERE/fake_agent.sh" "$W/agent.sh" && chmod +x "$W/agent.sh" || exit 2
printf 'Say READY.\n' > "$T/p.txt"
R=$$
GOOD="good oktool"
BAD="numeric nullupd nopayload emptylast nulllast badtool emptytool nltool nltail errstep toperror errkey empty notobject garbage multiline twoperline blankline leftfile leftdir fail"
for m in leftfile leftdir; do
  for d in "/dev/shm/web_job_w_${m}_$R" "/dev/shm/text_job_t_${m}_$R"; do
    { [ -e "$d" ] || [ -L "$d" ]; } && { echo "ABORT: $d already exists; not created by this run"; rm -rf "$T" "$W"; exit 2; }
  done
done
fails=0
check() { # lane case exit expected
  if [ "$3" = "$4" ]; then printf 'ok    %-5s %-18s exit=%s\n' "$1" "$2" "$3"; else printf 'WRONG %-5s %-18s exit=%s expected=%s\n' "$1" "$2" "$3" "$4"; fails=$((fails + 1)); fi
}
web() { WEB_JOB_DIR=$T/web AGENT_BIN=$W/agent.sh AGENT_PROFILE=$W/profile bash "$S/run_web_job.sh" "$@" </dev/null > /dev/null 2>&1; }
text() { TEXT_JOB_DIR=$T/text AGENT_BIN=$W/agent.sh AGENT_HOME=$W/profile bash "$S/run_text_job.sh" "$@" </dev/null > /dev/null 2>&1; }
: > "$T/blank.txt"
printf 'Say READY.\n' > "$T/unreadable.txt"; chmod 000 "$T/unreadable.txt"
cd /dev/shm || exit 2
for m in $GOOD $BAD; do
  web "w_${m}_$R" "$T/p.txt" "$m" 20; rc=$?
  case " $GOOD " in *" $m "*) check web "$m" "$rc" 0 ;; *) check web "$m" "$rc" 4 ;; esac
done
WEB_ALLOWED_TOOLS='(' web "w_badre_tools_$R" "$T/p.txt" oktool 20;  check web bad_regex_tools "$?" 2
WEB_ALLOWED_TOOLS='(' web "w_badre_plain_$R" "$T/p.txt" good 20;    check web bad_regex_no_tools "$?" 2
web "w_blank_$R" "$T/blank.txt" good 20;                             check web blank_prompt "$?" 2
web "w_good_$R" "$T/p.txt" good 20;                                  check web repeated_name "$?" 2
if [ "$(id -u)" != 0 ]; then web "w_unread_$R" "$T/unreadable.txt" good 20; check web unreadable_prompt "$?" 2; fi
if [ -n "${XFAM_AUDIT_BIN:-}" ]; then
  # In the text lane every step that is not input or response is rejected, so "oktool" is a bad case here.
  for m in good oktool $BAD; do
    text "t_${m}_$R" "$T/p.txt" "$m" 20; rc=$?
    if [ "$m" = good ]; then check text "$m" "$rc" 0; else check text "$m" "$rc" 4; fi
  done
  text "t_blank_$R" "$T/blank.txt" good 20;                          check text blank_prompt "$?" 2
  text "t_good_$R" "$T/p.txt" good 20;                               check text repeated_name "$?" 2
  if [ "$(id -u)" != 0 ]; then text "t_unread_$R" "$T/unreadable.txt" good 20; check text unreadable_prompt "$?" 2; fi
else
  echo "text lane skipped: set XFAM_AUDIT_BIN"
fi
for m in leftfile leftdir; do rm -rf "/dev/shm/web_job_w_${m}_$R" "/dev/shm/text_job_t_${m}_$R"; done
chmod 600 "$T/unreadable.txt"; rm -rf "$T" "$W"
echo "cases with a wrong outcome: $fails"
[ "$fails" = 0 ]
