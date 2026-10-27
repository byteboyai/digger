#!/bin/sh
# Deterministic stand-in for the `claude` / `codex` command lines that dozerd's group
# runner spawns for a group-chat turn. It RECORDS its argv and NEVER executes,
# evaluates or writes anything from the prompt. No network, no project writes: the
# only file it writes is its own log (and Codex's --output-last-message file, which
# is the reply itself).
#
# Prompt directives (anywhere in the prompt, which includes the whole group record):
#   FAKE:slow     five chunks, 0.4s apart, then the normal reply
#   FAKE:fail     exit 1 with a fixed error on stderr
#   FAKE:hostile  untrusted-looking markdown (script, javascript: link, remote image,
#                 long unbroken line, RTL / zero-width, unclosed fence)
#   FAKE:long     about 18 000 characters
set -u

name="$(basename "$0")"
log="${FAKE_AGENT_LOG:-${TMPDIR:-/tmp}/digger-fake-agents.log}"

# Record the command line (each argument cut to 120 chars, whitespace flattened).
{
  printf 'FAKE_AGENT %s cwd=%s' "$name" "$(pwd)"
  for a in "$@"; do
    printf ' arg=[%s]' "$(printf '%s' "$a" | head -c 120 | tr '\n\t' '  ')"
  done
  printf '\n'
} >>"$log" 2>/dev/null || true

# The prompt: Claude reads it from stdin, Codex passes it as the last argument.
out_file=""
prompt=""
case "$name" in
  claude)
    prompt="$(cat)"
    ;;
  codex)
    prev=""
    for a in "$@"; do
      if [ "$prev" = "--output-last-message" ]; then out_file="$a"; fi
      prev="$a"
      prompt="$a"
    done
    ;;
  *)
    echo "fake-agent: unsupported name $name" >&2
    exit 64
    ;;
esac

reply() {
  case "$prompt" in
    *FAKE:fail*)
      echo "fake agent failure (FAKE:fail)" >&2
      exit 1
      ;;
  esac
  case "$prompt" in
    *FAKE:slow*)
      for i in 1 2 3 4 5; do
        sleep 0.4
      done
      ;;
  esac
  case "$prompt" in
    *FAKE:hostile*)
      printf '%s\n' '## 不可信内容'
      printf '%s\n' '<script>document.title="pwned"</script> <img src=x onerror=alert(1)>'
      printf '%s\n' '[点我](javascript:alert(1)) ![远程图](https://example.invalid/pixel.png)'
      printf '%s\n' '<link rel="stylesheet" href="https://example.invalid/x.css">'
      printf '%s\n' "$(head -c 15000 /dev/zero | tr '\0' 'a')"
      printf '%s\n' 'RTL: ‮evil‬ 零宽:a​b‌c‍d'
      printf '%s\n' '```js'
      printf '%s\n' 'unclosed fence'
      ;;
    *FAKE:long*)
      i=0
      while [ "$i" -lt 450 ]; do
        printf '第 %03d 行:这是一段用来检验滚动与布局的确定性文字,不含任何随机内容。\n' "$i"
        i=$((i + 1))
      done
      ;;
    *)
      printf '%s\n' '这是假 Agent 的确定性回复。' '' '- 要点一' '- 要点二' '' '```rust' 'fn main() { println!("hi"); }' '```' '' '参考:[文档](https://example.invalid/docs)'
      ;;
  esac
}

text="$(reply)"
status=$?
if [ "$status" -ne 0 ]; then
  exit "$status"
fi
if [ -n "$out_file" ]; then
  printf '%s\n' "$text" >"$out_file"
fi
printf '%s\n' "$text"
