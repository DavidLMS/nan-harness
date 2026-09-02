use std::process::Output;

pub(super) fn first_non_empty_output_line(output: &Output) -> String {
    for stream in [&output.stdout, &output.stderr] {
        if let Some(line) = stream
            .split(|byte| *byte == b'\n' || *byte == b'\r')
            .map(|line| String::from_utf8_lossy(line).trim().to_owned())
            .find(|line| !line.is_empty())
        {
            return line;
        }
    }
    String::new()
}

pub(super) fn summarize_output(output: &Output) -> String {
    let mut summary = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if summary.is_empty() {
        summary.push_str(String::from_utf8_lossy(&output.stdout).trim());
    }
    if summary.chars().count() > 2_000 {
        summary = summary.chars().take(2_000).collect();
        summary.push('…');
    }
    summary
}
