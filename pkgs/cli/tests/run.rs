use std::process::{Command, Output};

fn run(source: &str) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("script.csh");
    std::fs::write(&script, source).unwrap();
    Command::new(env!("CARGO_BIN_EXE_cssh"))
        .arg("run")
        .arg(&script)
        .current_dir(dir.path())
        .output()
        .unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bundled_utilities_are_used_without_system_path() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("script.csh");
    std::fs::write(
        &script,
        "cat --version\nprintf 'b\\na\\nb\\n' | sort | uniq",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cssh"))
        .args(["run", script.to_str().unwrap()])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("uutils"), "{stdout}");
    assert!(stdout.ends_with("a\nb\n"), "{stdout}");
}

#[test]
fn state_expansion_functions_and_redirection() {
    let output = run(r#"
mkdir work
cd work
greet() { printf '%s\n' "$1"; }
for word in 'hello world' second; do greet "$word"; done > words
value=$(cat words)
if test -n "$value"; then printf '%s\n' "$value"; fi
(cd ..; touch outside)
test -f ../outside && cat < words | wc -l
"#);
    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "hello world\nsecond\n2\n"
    );
}

#[test]
fn command_failures_and_exit_status_propagate() {
    assert_eq!(run("false").status.code(), Some(1));
    assert_eq!(run("exit 42\necho unreachable").status.code(), Some(42));
    assert_eq!(
        run("crossshell_nonexistent_command_12345").status.code(),
        Some(127)
    );
    let output = run("false && echo wrong; false || echo recovered");
    assert_success(&output);
    assert_eq!(output.stdout, b"recovered\n");
}

#[test]
fn syntax_errors_do_not_execute_partial_scripts() {
    let output = run("echo must-not-run\nif then");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn pipelines_stream_more_than_a_pipe_buffer() {
    let output = run("seq 1 100000 | cat | wc -l");
    assert_success(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "100000");
}

#[test]
fn heredocs_and_background_wait() {
    let output =
        run("value=expanded\ncat <<EOF\n$value\nEOF\nprintf background > out &\nwait\ncat out");
    assert_success(&output);
    assert_eq!(output.stdout, b"expanded\nbackground");
}

#[cfg(unix)]
#[test]
fn external_commands_receive_environment_and_native_fds() {
    let output = run(r#"
VALUE='hello world' /bin/sh -c 'printf "%s\n" "$VALUE"; echo error >&2' > out 2> err
cat out err
/bin/sh -c 'exit 23'
"#);
    assert_eq!(output.status.code(), Some(23));
    assert_eq!(output.stdout, b"hello world\nerror\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn expansions_preserve_quoting_splitting_globs_and_positionals() {
    let output = run(r#"
set -- 'one two' '' three
printf '<%s>\n' pre"$@"post
value='a b'
printf '<%s>\n' "$value" $value ${missing:-"x y"}
touch b.txt a.txt
printf '%s\n' *.txt '*.txt' pre{1,2}{a,b}
"#);
    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "<preone two>\n<>\n<threepost>\n<a b>\n<a>\n<b>\n<x y>\na.txt\nb.txt\n*.txt\npre1a\npre1b\npre2a\npre2b\n"
    );
}

#[test]
fn arithmetic_conditionals_and_loop_control() {
    let output = run(r#"
sum=0
for ((i=0; i<5; i++)); do
  if ((i == 2)); then continue; fi
  ((sum += i))
done
while ((sum < 10)); do ((sum++)); done
until ((sum == 8)); do ((sum--)); done
case "$sum" in 8) printf '%s\n' "${sum}" ;; *) false ;; esac
[[ hello == h* && $((2 + 2)) -eq 4 ]] && echo matched
"#);
    assert_success(&output);
    assert_eq!(output.stdout, b"8\nmatched\n");
}

#[test]
fn parameter_operators_and_function_locals() {
    let output = run(r#"
name=outside
f() { local name=inside; printf '%s:%s\n' "$name" "$1"; return 7; }
f argument || printf '%s:%s\n' "$name" "$?"
value=abcabc
printf '%s\n' "${missing:=default}" "${missing}" "${#value}" "${value:1:3}" "${value#a*}" "${value##a*}" "${value%c*}" "${value//ab/X}"
"#);
    assert_success(&output);
    assert_eq!(
        output.stdout,
        b"inside:argument\noutside:7\ndefault\ndefault\n6\nbca\nbcabc\n\nabcab\nXcXc\n"
    );
}

#[test]
fn errexit_respects_conditional_context_and_pipefail() {
    let output = run(
        "set -e; false && echo wrong; ! true; if false; then echo wrong; fi; echo survived; false; echo wrong",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"survived\n");
    assert_eq!(run("set -o pipefail; false | true").status.code(), Some(1));
    assert_eq!(run("false | true").status.code(), Some(0));
}

#[test]
fn substitutions_stream_and_keep_their_exit_status() {
    let output = run("value=$(seq 1 100000); echo ${#value}; value=$(false); echo $?");
    assert_success(&output);
    assert_eq!(output.stdout, b"588894\n1\n");
}

#[test]
fn outstanding_background_jobs_finish_before_the_cli_returns() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("job.csh");
    std::fs::write(&script, "{ sleep 0.05; printf finished > result; } &").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_cssh"))
        .arg("run")
        .arg(&script)
        .current_dir(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result")).unwrap(),
        "finished"
    );
}

#[cfg(unix)]
#[test]
fn descriptor_duplication_is_ordered_and_native() {
    let output = run(r#"
/bin/sh -c 'echo out; echo err >&2' 2>&1 > out
cat out
/bin/sh -c 'echo extra >&3' 3> extra
cat extra
"#);
    assert_success(&output);
    assert_eq!(output.stdout, b"err\nout\nextra\n");
    assert!(output.stderr.is_empty());
}
