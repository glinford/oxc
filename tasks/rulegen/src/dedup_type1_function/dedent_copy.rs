pub fn dedent(s: &str) -> String {
    // find the min whitespace count
    let min_indent = s
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or_default();

    s.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| if line.len() >= min_indent { &line[min_indent..] } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}
