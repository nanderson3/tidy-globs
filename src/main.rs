use std::env;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

/// True if every "}" in `s` closes a "{" that came before it, and every
/// "{" is eventually closed. Brace normalization bails out and returns the
/// input untouched when this doesn't hold, rather than guess at intent.
fn braces_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Trims whitespace around the alternatives of each brace group, including
/// nested ones: "{ a, b }" -> "{a,b}". Left alone entirely if the pattern's
/// braces aren't balanced.
fn normalize_brace_spacing(pattern: &str) -> String {
    if !braces_balanced(pattern) {
        return pattern.to_string();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut pos = 0;
    normalize_braces(&chars, &mut pos)
}

fn normalize_braces(chars: &[char], pos: &mut usize) -> String {
    let mut out = String::new();
    while *pos < chars.len() {
        if chars[*pos] == '{' {
            *pos += 1;
            out.push('{');
            out.push_str(&normalize_brace_group(chars, pos));
            out.push('}');
        } else {
            out.push(chars[*pos]);
            *pos += 1;
        }
    }
    out
}

// Called with `pos` just past the opening "{". Consumes up to and including
// the matching "}" and returns the trimmed, comma-joined alternatives.
fn normalize_brace_group(chars: &[char], pos: &mut usize) -> String {
    let mut alternatives: Vec<String> = Vec::new();
    let mut current = String::new();
    while *pos < chars.len() {
        match chars[*pos] {
            '{' => {
                *pos += 1;
                current.push('{');
                current.push_str(&normalize_brace_group(chars, pos));
                current.push('}');
            }
            ',' => {
                alternatives.push(current.trim().to_string());
                current.clear();
                *pos += 1;
            }
            '}' => {
                *pos += 1;
                alternatives.push(current.trim().to_string());
                return alternatives.join(",");
            }
            c => {
                current.push(c);
                *pos += 1;
            }
        }
    }
    // Unreachable when braces_balanced held, but keeps this total.
    alternatives.push(current.trim().to_string());
    alternatives.join(",")
}

/// Rewrites a glob pattern into a canonical form without changing what it
/// matches:
///   - runs of slashes collapse to one ("a//b" -> "a/b")
///   - "./" segments are dropped ("./src/*.rs" -> "src/*.rs")
///   - repeated "**" segments collapse to one ("**/**/x" -> "**/x")
///   - brace-group spacing is trimmed ("{ a, b }" -> "{a,b}")
/// A leading slash and a meaningful trailing slash (directory-only match,
/// as gitignore-style tools use it) are both preserved.
fn normalize(pattern: &str) -> String {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return String::new();
    }
    let pattern = normalize_brace_spacing(pattern);
    let pattern = pattern.as_str();

    let is_absolute = pattern.starts_with('/');
    // A single "/" is the root itself, not a trailing-slash marker on it.
    let has_trailing_slash = pattern.len() > 1 && pattern.ends_with('/');

    let mut segments: Vec<&str> = Vec::new();
    for raw in pattern.split('/') {
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == "**" && segments.last() == Some(&"**") {
            continue;
        }
        segments.push(raw);
    }

    let mut out = String::new();
    if is_absolute {
        out.push('/');
    }
    out.push_str(&segments.join("/"));
    if out.is_empty() {
        out.push('/');
    } else if has_trailing_slash {
        out.push('/');
    }
    out
}

fn run<R: BufRead, W: Write>(patterns: &[String], input: R, mut output: W) -> io::Result<()> {
    if patterns.is_empty() {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            writeln!(output, "{}", normalize(&line))?;
        }
    } else {
        for pattern in patterns {
            writeln!(output, "{}", normalize(pattern))?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let patterns: Vec<String> = env::args().skip(1).collect();
    let stdin = io::stdin();
    let stdout = io::stdout();

    match run(&patterns, stdin.lock(), stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("tidy-globs: {}", err);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn collapses_repeated_slashes() {
        assert_eq!(normalize("src//**///*.rs"), "src/**/*.rs");
    }

    #[test]
    fn drops_leading_dot_slash() {
        assert_eq!(normalize("./src/*.rs"), "src/*.rs");
    }

    #[test]
    fn drops_dot_segments_in_the_middle() {
        assert_eq!(normalize("src/./lib/./mod.rs"), "src/lib/mod.rs");
    }

    #[test]
    fn collapses_repeated_double_star() {
        assert_eq!(normalize("**/**/target"), "**/target");
        assert_eq!(normalize("a/**/**/**/b"), "a/**/b");
    }

    #[test]
    fn preserves_leading_slash() {
        assert_eq!(normalize("/usr/local/*.so"), "/usr/local/*.so");
    }

    #[test]
    fn preserves_meaningful_trailing_slash() {
        assert_eq!(normalize("build/output/"), "build/output/");
        assert_eq!(normalize("build///output//"), "build/output/");
    }

    #[test]
    fn root_stays_root() {
        assert_eq!(normalize("/"), "/");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(normalize("  src/*.rs  \n"), "src/*.rs");
    }

    #[test]
    fn leaves_already_clean_patterns_alone() {
        assert_eq!(normalize("src/**/*.rs"), "src/**/*.rs");
    }

    #[test]
    fn trims_brace_group_spacing() {
        assert_eq!(normalize("*.{ txt, md }"), "*.{txt,md}");
        assert_eq!(normalize("src/{  a  ,b,  c}/*.rs"), "src/{a,b,c}/*.rs");
    }

    #[test]
    fn trims_nested_brace_group_spacing() {
        assert_eq!(normalize("{ a, { b, c } }"), "{a,{b,c}}");
    }

    #[test]
    fn leaves_unbalanced_braces_untouched() {
        assert_eq!(normalize("{a, b/*.rs"), "{a, b/*.rs");
        assert_eq!(normalize("a, b}/*.rs"), "a, b}/*.rs");
    }

    #[test]
    fn run_reads_stdin_when_no_args_given() {
        let input = b"./a//b\n\nc/./d\n" as &[u8];
        let mut output = Vec::new();
        super::run(&[], input, &mut output).unwrap();
        assert_eq!(output, b"a/b\nc/d\n");
    }

    #[test]
    fn run_prefers_args_over_stdin() {
        let input = b"" as &[u8];
        let mut output = Vec::new();
        let args = vec!["./x//y".to_string()];
        super::run(&args, input, &mut output).unwrap();
        assert_eq!(output, b"x/y\n");
    }
}
