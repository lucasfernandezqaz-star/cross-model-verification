#!/bin/sh
# Test double for an agent CLI: prints a canned event log chosen by the --model argument.
m=""; while [ $# -gt 0 ]; do [ "$1" = "--model" ] && m=$2; shift; done
ok='{"event":"init"}
{"event":"step_update","step_update":{"step_type":"user_input"}}
{"event":"step_update","step_update":{"step_type":"agent_response"}}'
fin='{"event":"result","result":{"response":"READY"}}'
case "$m" in
  good)       printf '%s\n%s\n' "$ok" "$fin" ;;
  oktool)     printf '%s\n{"event":"step_update","step_update":{"step_type":"tool","tool_name":"search_web","state":"DONE"}}\n%s\n' "$ok" "$fin" ;;
  numeric)    printf '%s\n{"event":"step_update","step_update":7}\n%s\n' "$ok" "$fin" ;;
  nullupd)    printf '%s\n{"event":"step_update","step_update":null}\n%s\n' "$ok" "$fin" ;;
  nopayload)  printf '%s\n{"event":"step_update"}\n%s\n' "$ok" "$fin" ;;
  emptylast)  printf '%s\n{"event":"result","result":{"response":"EARLIER"}}\n{"event":"result","result":{"response":""}}\n' "$ok" ;;
  nulllast)   printf '%s\n{"event":"result","result":{"response":"EARLIER"}}\n{"event":"result","result":{"response":null}}\n' "$ok" ;;
  badtool)    printf '%s\n{"event":"step_update","step_update":{"step_type":"tool","tool_name":"run_command"}}\n%s\n' "$ok" "$fin" ;;
  emptytool)  printf '%s\n{"event":"step_update","step_update":{"step_type":"tool","tool_name":""}}\n%s\n' "$ok" "$fin" ;;
  nltool)     printf '%s\n{"event":"step_update","step_update":{"step_type":"tool","tool_name":"search_web\\nread_url_content"}}\n%s\n' "$ok" "$fin" ;;
  nltail)     printf '%s\n{"event":"step_update","step_update":{"step_type":"tool","tool_name":"search_web\\n"}}\n%s\n' "$ok" "$fin" ;;
  errstep)    printf '%s\n{"event":"step_update","step_update":{"step_type":"error_message","tool_name":"search_web"}}\n%s\n' "$ok" "$fin" ;;
  toperror)   printf '%s\n{"event":"error","message":"quota"}\n%s\n' "$ok" "$fin" ;;
  errkey)     printf '%s\n{"event":"note","error":{"code":7}}\n%s\n' "$ok" "$fin" ;;
  empty)      : ;;
  notobject)  printf '%s\n[1,2]\n%s\n' "$ok" "$fin" ;;
  garbage)    printf '%s\nnot json at all\n%s\n' "$ok" "$fin" ;;
  multiline)  printf '%s\n{"event":\n"note"}\n%s\n' "$ok" "$fin" ;;
  twoperline) printf '%s\n{"event":"a"} {"event":"b"}\n%s\n' "$ok" "$fin" ;;
  blankline)  printf '%s\n\n%s\n' "$ok" "$fin" ;;
  leftfile)   printf '%s\n%s\n' "$ok" "$fin"; echo x > ./left_behind.txt ;;
  leftdir)    printf '%s\n%s\n' "$ok" "$fin"; mkdir ./prompt.txt ;;
  fail)       printf '%s\n%s\n' "$ok" "$fin"; exit 7 ;;
esac
