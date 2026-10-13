# Each line starts with a leading space to leave these commands out of history.
#
# command -p resolves the given command with the system default PATH, ensuring the shell
# can find them even if the user has a clobbered PATH value.
 unsetopt ZLE
 WARP_SESSION_ID=@@WARP_SESSION_ID@@
 # zsh already knows the host ($HOST, from gethostname like `hostname`) and the user
 # ($USERNAME, like `whoami`), and printf can hex-encode the message, so this starts no
 # processes. A zsh without `printf -v` hex-encodes with od instead.
 _msg="{\"hook\": \"InitShell\", \"value\": {\"session_id\": $WARP_SESSION_ID, \"shell\": \"zsh\", \"user\": \"$USERNAME\", \"hostname\": \"$HOST\"}}"
 () { local LC_ALL=C; local -a _bytes; _bytes=("${(@s::)_msg}"); builtin printf -v _msg "%02x" "\"${^_bytes[@]}" } 2>/dev/null || _msg=$(printf "%s" "$_msg" | command -p od -An -v -tx1 | command -p tr -d " \n")
 WARP_USING_WINDOWS_CON_PTY=@@USING_CON_PTY_BOOLEAN@@
 # We send the InitShell hook via OSCs when on Windows and via DCSs otherwise.
 if [ "$WARP_USING_WINDOWS_CON_PTY" = true ]; then printf '\e]9278;d;%s\x07' "$_msg"; else printf '\x1b\x50\x24\x64%s\x1b\x5c' "$_msg"; fi
 unset _msg
