//! A stand-in for yt-dlp or ffmpeg in the unit tests, on every platform.
//!
//! `testing::stand_in` compiles this file once per test run and copies it
//! into `bin/` under the tool's name. What it does comes from the file next
//! to it with the extension `.script` (`bin/yt-dlp.script`), one command
//! per line, run in order:
//!
//! ```text
//! out <text>             a line on stdout
//! err <text>             a line on stderr
//! out-raw <text>         a line on stdout, with \xNN written as that byte
//! repeat <n> out <text>  the same line n times (also with err)
//! args-to <file>         the arguments it was given, one per line
//! stdin-to <file>        everything it reads on stdin
//! write <file>|<text>    a file holding <text> and a newline
//! append <file>|<text>   <text> and a newline added at the end of <file>
//! write-last-arg <text>  the same, into the file named by its last argument
//! spawn-sleeper <file>   starts a copy of itself that sleeps for a minute,
//!                        and writes that copy's process id to <file>
//! sleep <seconds>
//! exit <code>            (without one it exits with 0 at the end)
//! ```
//!
//! Not part of the crate: it is built on its own with plain `rustc`.

use std::io::{Read, Write};
use std::time::Duration;
use std::{env, fs, process, thread};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--sleeper") {
        thread::sleep(Duration::from_secs(60));
        return;
    }
    let me = env::current_exe().expect("own path");
    let script = fs::read_to_string(me.with_extension("script")).expect("the .script file");
    for line in script.lines() {
        run(line, &args, &me);
    }
}

fn run(line: &str, args: &[String], me: &std::path::Path) {
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    match command {
        "" => {}
        "out" => println!("{rest}"),
        "err" => eprintln!("{rest}"),
        "out-raw" => {
            let mut bytes = unescape(rest);
            bytes.push(b'\n');
            std::io::stdout().write_all(&bytes).unwrap();
        }
        "repeat" => {
            let (n, line) = rest.split_once(' ').expect("repeat <n> <command>");
            for _ in 0..n.parse::<u32>().expect("a count") {
                run(line, args, me);
            }
        }
        "args-to" => fs::write(rest, args.join("\n")).unwrap(),
        "stdin-to" => {
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input).unwrap();
            fs::write(rest, input).unwrap();
        }
        "write" => {
            let (file, text) = rest.split_once('|').expect("write <file>|<text>");
            fs::write(file, format!("{text}\n")).unwrap();
        }
        "append" => {
            let (file, text) = rest.split_once('|').expect("append <file>|<text>");
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
                .unwrap();
            writeln!(file, "{text}").unwrap();
        }
        "write-last-arg" => {
            let file = args.last().expect("an argument");
            fs::write(file, format!("{rest}\n")).unwrap();
        }
        "spawn-sleeper" => {
            let child = process::Command::new(me)
                .arg("--sleeper")
                .stdin(process::Stdio::null())
                .stdout(process::Stdio::null())
                .stderr(process::Stdio::null())
                .spawn()
                .unwrap();
            fs::write(rest, child.id().to_string()).unwrap();
        }
        "sleep" => thread::sleep(Duration::from_secs_f64(rest.parse().expect("seconds"))),
        "exit" => {
            std::io::stdout().flush().unwrap();
            process::exit(rest.parse().expect("an exit code"));
        }
        other => panic!("unknown command {other:?}"),
    }
}

/// `Jos\xe9` as the bytes `J o s 0xE9`.
fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = text.as_bytes();
    while let Some((&first, tail)) = rest.split_first() {
        if first == b'\\' && tail.first() == Some(&b'x') && tail.len() >= 3 {
            let hex = std::str::from_utf8(&tail[1..3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).expect("two hex digits"));
            rest = &tail[3..];
        } else {
            out.push(first);
            rest = tail;
        }
    }
    out
}
