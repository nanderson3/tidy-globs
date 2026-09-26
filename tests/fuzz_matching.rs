//! Property test: normalizing a pattern must never change which paths it
//! matches. There's no glob matcher elsewhere in this crate to check that
//! against, so this file carries its own small reference matcher (segment
//! splitting, `*`/`?` via the standard wildcard DP, `**` as zero-or-more
//! segments, brace groups expanded into literal alternatives) and a
//! deterministic pattern/path generator, then asserts
//! `glob_match(p, path) == glob_match(normalize(p), path)` across many
//! generated cases. Seeds are fixed so a failure is reproducible.

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn gen_range(&mut self, bound: u32) -> u32 {
        (self.next_u64() % bound as u64) as u32
    }

    fn gen_bool(&mut self, numerator: u32, denominator: u32) -> bool {
        self.gen_range(denominator) < numerator
    }
}

const WORDS: [&str; 6] = ["a", "bb", "ccc", "dir", "file", "x9"];
const EXT_WORDS: [&str; 4] = ["rs", "md", "txt", "toml"];

fn gen_literal(rng: &mut Rng) -> String {
    WORDS[rng.gen_range(WORDS.len() as u32) as usize].to_string()
}

fn gen_wild_literal(rng: &mut Rng) -> String {
    match rng.gen_range(4) {
        0 => format!("*.{}", EXT_WORDS[rng.gen_range(EXT_WORDS.len() as u32) as usize]),
        1 => format!("{}*", gen_literal(rng)),
        2 => format!("{}?", gen_literal(rng)),
        _ => gen_literal(rng),
    }
}

fn gen_brace_alt(rng: &mut Rng, depth: u32) -> String {
    if depth < 2 && rng.gen_bool(1, 6) {
        gen_brace_group(rng, depth + 1)
    } else {
        gen_wild_literal(rng)
    }
}

fn gen_brace_group(rng: &mut Rng, depth: u32) -> String {
    let count = 2 + rng.gen_range(2);
    let mut alts = Vec::new();
    for _ in 0..count {
        alts.push(gen_brace_alt(rng, depth));
    }
    let separator = if rng.gen_bool(1, 2) { ", " } else { "," };
    let mut out = String::from("{");
    if rng.gen_bool(1, 3) {
        out.push(' ');
    }
    out.push_str(&alts.join(separator));
    if rng.gen_bool(1, 3) {
        out.push(' ');
    }
    out.push('}');
    out
}

fn gen_segment(rng: &mut Rng) -> String {
    match rng.gen_range(9) {
        0 => "*".to_string(),
        1 => "?".to_string(),
        2 => "**".to_string(),
        3 => ".".to_string(),
        4 => gen_brace_group(rng, 0),
        _ => gen_wild_literal(rng),
    }
}

/// Builds a pattern with the exact kind of noise `normalize` is meant to
/// remove: doubled slashes, `./` segments, and repeated `**`.
fn gen_pattern(rng: &mut Rng) -> String {
    let seg_count = 1 + rng.gen_range(4);
    let mut segs: Vec<String> = Vec::new();
    for _ in 0..seg_count {
        segs.push(gen_segment(rng));
    }

    let mut with_dupes: Vec<String> = Vec::new();
    for seg in segs {
        let is_double_star = seg == "**";
        with_dupes.push(seg);
        if is_double_star && rng.gen_bool(1, 2) {
            with_dupes.push("**".to_string());
        }
    }
    let segs = with_dupes;

    let mut out = String::new();
    if rng.gen_bool(1, 4) {
        out.push('/');
    }
    if rng.gen_bool(1, 5) {
        out.push_str("./");
    }
    for (i, seg) in segs.iter().enumerate() {
        out.push_str(seg);
        if i + 1 < segs.len() {
            let extra = rng.gen_range(3);
            for _ in 0..=extra {
                out.push('/');
            }
        }
    }
    if rng.gen_bool(1, 4) {
        let extra = rng.gen_range(3);
        for _ in 0..=extra {
            out.push('/');
        }
    }
    out
}

fn gen_path(rng: &mut Rng) -> String {
    let seg_count = rng.gen_range(4);
    let mut segs: Vec<String> = Vec::new();
    for _ in 0..seg_count {
        segs.push(gen_literal(rng));
    }
    let mut out = String::new();
    if rng.gen_bool(1, 3) {
        out.push('/');
    }
    out.push_str(&segs.join("/"));
    if !out.is_empty() && rng.gen_bool(1, 6) {
        out.push('/');
    }
    out
}

// --- reference matcher ---
//
// Mirrors the same structural split `normalize` itself uses (absolute
// marker, trailing-slash marker, segments with empty/"." ones dropped) so
// the two share a definition of what a pattern's "shape" is, but decides
// matches instead of rewriting text.

struct Parsed {
    is_absolute: bool,
    has_trailing_slash: bool,
    segments: Vec<String>,
}

fn parse(s: &str) -> Parsed {
    let is_absolute = s.starts_with('/');
    let has_trailing_slash = s.len() > 1 && s.ends_with('/');
    let mut segments = Vec::new();
    for raw in s.split('/') {
        if raw.is_empty() || raw == "." {
            continue;
        }
        segments.push(raw.to_string());
    }
    Parsed { is_absolute, has_trailing_slash, segments }
}

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

/// Expands the (possibly nested) brace groups in a single path segment into
/// its literal alternatives, trimming whitespace the same way `normalize`
/// does, so "{ a, b}" and "{a,b}" are treated as identical for matching.
/// Left as a single alternative, unchanged, if the braces aren't balanced.
fn expand_segment(seg: &str) -> Vec<String> {
    if !seg.contains('{') || !braces_balanced(seg) {
        return vec![seg.to_string()];
    }
    let chars: Vec<char> = seg.chars().collect();
    let mut pos = 0;
    expand_top(&chars, &mut pos)
}

fn expand_top(chars: &[char], pos: &mut usize) -> Vec<String> {
    let mut pieces: Vec<String> = vec![String::new()];
    while *pos < chars.len() {
        if chars[*pos] == '{' {
            *pos += 1;
            let group = expand_group(chars, pos);
            let mut combined = Vec::new();
            for p in &pieces {
                for g in &group {
                    combined.push(format!("{}{}", p, g));
                }
            }
            pieces = combined;
        } else {
            let c = chars[*pos];
            for p in pieces.iter_mut() {
                p.push(c);
            }
            *pos += 1;
        }
    }
    pieces
}

// Called with `pos` just past the opening "{". Consumes up to and including
// the matching "}" and returns the trimmed alternatives, expanding any
// nested groups first.
fn expand_group(chars: &[char], pos: &mut usize) -> Vec<String> {
    let mut alternatives: Vec<String> = Vec::new();
    let mut pieces: Vec<String> = vec![String::new()];
    loop {
        if *pos >= chars.len() {
            // Unreachable when the segment's braces are balanced, but keeps
            // this total instead of panicking on malformed input.
            break;
        }
        match chars[*pos] {
            '{' => {
                *pos += 1;
                let nested = expand_group(chars, pos);
                let mut combined = Vec::new();
                for p in &pieces {
                    for n in &nested {
                        combined.push(format!("{}{}", p, n));
                    }
                }
                pieces = combined;
            }
            ',' => {
                *pos += 1;
                for p in pieces.drain(..) {
                    alternatives.push(p.trim().to_string());
                }
                pieces = vec![String::new()];
            }
            '}' => {
                *pos += 1;
                for p in pieces.drain(..) {
                    alternatives.push(p.trim().to_string());
                }
                return alternatives;
            }
            c => {
                for p in pieces.iter_mut() {
                    p.push(c);
                }
                *pos += 1;
            }
        }
    }
    for p in pieces.drain(..) {
        alternatives.push(p.trim().to_string());
    }
    alternatives
}

/// Standard `*`/`?` wildcard matching within a single path segment (no
/// separators involved), via the usual O(pattern * text) DP.
fn segment_glob_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == t[j - 1],
            };
        }
    }
    dp[p.len()][t.len()]
}

fn match_segments(pat: &[String], path: &[String]) -> bool {
    if pat.is_empty() {
        return path.is_empty();
    }
    if pat[0] == "**" {
        if match_segments(&pat[1..], path) {
            return true;
        }
        if !path.is_empty() {
            return match_segments(pat, &path[1..]);
        }
        return false;
    }
    if path.is_empty() {
        return false;
    }
    for alt in expand_segment(&pat[0]) {
        if segment_glob_match(&alt, &path[0]) && match_segments(&pat[1..], &path[1..]) {
            return true;
        }
    }
    false
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let p = parse(pattern);
    let t = parse(path);
    if p.is_absolute != t.is_absolute {
        return false;
    }
    if p.has_trailing_slash && !t.has_trailing_slash {
        return false;
    }
    match_segments(&p.segments, &t.segments)
}

/// Builds a concrete path out of a pattern's own segments (wildcards
/// replaced with plain characters, `**` dropped, brace groups resolved to
/// their first alternative), giving the fuzzer a path that's likely to
/// exercise the "still matches" side of the property, not just the
/// "still doesn't match" side that random paths mostly hit.
fn derive_matching_path(pattern: &str) -> String {
    let parsed = parse(pattern);
    let mut segs: Vec<String> = Vec::new();
    for seg in &parsed.segments {
        if seg == "**" {
            continue;
        }
        let alt = expand_segment(seg).into_iter().next().unwrap_or_default();
        let mut concrete = alt.replace('*', "x").replace('?', "y");
        if concrete.is_empty() {
            concrete = "x".to_string();
        }
        segs.push(concrete);
    }
    let mut out = String::new();
    if parsed.is_absolute {
        out.push('/');
    }
    out.push_str(&segs.join("/"));
    if parsed.has_trailing_slash && !out.ends_with('/') {
        out.push('/');
    }
    out
}

#[test]
fn normalize_preserves_match_semantics() {
    let seeds: [u64; 6] = [
        0x9E3779B97F4A7C15,
        0xD1B54A32D192ED03,
        0x2545F4914F6CDD1D,
        0x27220A97D4A1B213,
        0xA24BAED4963EE407,
        0x9EEBB5C1F5A1D3A1,
    ];

    for seed in seeds {
        let mut rng = Rng::new(seed);
        for _ in 0..200 {
            let pattern = gen_pattern(&mut rng);
            let normalized = tidy_globs::normalize(&pattern);

            let mut paths = vec![derive_matching_path(&pattern)];
            for _ in 0..4 {
                paths.push(gen_path(&mut rng));
            }

            for path in &paths {
                let before = glob_match(&pattern, path);
                let after = glob_match(&normalized, path);
                assert_eq!(
                    before, after,
                    "normalize changed match semantics: {:?} -> {:?} against {:?}",
                    pattern, normalized, path
                );
            }
        }
    }
}
