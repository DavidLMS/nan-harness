use std::{
    env, fs,
    io::{self, Read, Write},
    process::{self, Command},
    thread,
    time::Duration,
};

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let executable = env::current_exe().unwrap();
    let mode = fs::read_to_string(executable.with_extension("mode")).ok();
    if mode.is_some() {
        writeln!(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(executable.with_extension("calls"))
                .unwrap(),
            "probe"
        )
        .unwrap();
    }
    let mode = mode
        .as_deref()
        .unwrap_or_else(|| arguments.first().map_or("success", String::as_str));
    if matches!(mode, "stdout-flood" | "stderr-flood" | "combined") {
        if let Some(marker) = arguments.get(1) {
            fs::write(marker, process::id().to_string()).unwrap();
        }
    }
    match mode {
        "success" => println!("codex 0.153.4"),
        "stderr" => eprintln!("codex 0.153.4"),
        "nonzero" => process::exit(17),
        "stdin" => {
            let mut input = String::new();
            assert_eq!(io::stdin().read_to_string(&mut input).unwrap(), 0);
            println!("closed");
        }
        "help-flood" if arguments.first().is_some_and(|arg| arg == "--version") => {
            println!("codex 0.153.4")
        }
        "stdout-flood" | "help-flood" => flood(false),
        "stderr-flood" => flood(true),
        "combined" => {
            io::stdout().write_all(&vec![b'o'; 600_000]).unwrap();
            io::stderr().write_all(&vec![b'e'; 600_000]).unwrap();
        }
        "retain" => {
            fs::write(
                format!("{}.parent", arguments[1]),
                process::id().to_string(),
            )
            .unwrap();
            let child = Command::new(&executable)
                .args(["sleep", &arguments[1]])
                .spawn()
                .unwrap();
            fs::write(
                format!("{}.descendant", arguments[1]),
                child.id().to_string(),
            )
            .unwrap();
            // Deliberately exit without waiting: the descendant retains both probe pipes.
        }
        "sleep" => {
            if let Some(marker) = arguments.get(1) {
                fs::write(marker, process::id().to_string()).unwrap();
            }
            thread::sleep(Duration::from_secs(10));
        }
        "hang" => {
            fs::write(executable.with_extension("pid"), process::id().to_string()).unwrap();
            thread::sleep(Duration::from_secs(60));
        }
        _ => panic!("unknown synthetic probe mode"),
    }
}

fn flood(stderr: bool) {
    let mut stream: Box<dyn Write> = if stderr {
        Box::new(io::stderr())
    } else {
        Box::new(io::stdout())
    };
    for _ in 0..2048 {
        if stream.write_all(&[b'x'; 8192]).is_err() {
            break;
        }
    }
}
