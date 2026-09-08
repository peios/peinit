use super::*;

/// Strip SGR escapes so a test can assert on layout without embedding escape
/// bytes in an expected value — which is the readability win the whole
/// render-at-the-sink split was for.
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn every_tag_occupies_the_same_column() {
    let tags = [
        ConsoleTag::None,
        ConsoleTag::Ok,
        ConsoleTag::Skip,
        ConsoleTag::Warn,
        ConsoleTag::Failed,
        ConsoleTag::Crit,
    ];
    let widths: Vec<usize> = tags
        .iter()
        .map(|tag| {
            let line = render_with(*tag, "peinit: x\n", false);
            line.find("peinit").expect("component follows the tag")
        })
        .collect();
    assert!(
        widths.windows(2).all(|w| w[0] == w[1]),
        "tags must align: {widths:?}"
    );
}

#[test]
fn tag_words_render_inside_a_fixed_bracket() {
    for (tag, expected) in [
        (ConsoleTag::None, "[      ] "),
        (ConsoleTag::Ok, "[  OK  ] "),
        (ConsoleTag::Skip, "[ SKIP ] "),
        (ConsoleTag::Warn, "[ WARN ] "),
        (ConsoleTag::Failed, "[FAILED] "),
        (ConsoleTag::Crit, "[ CRIT ] "),
    ] {
        let line = render_with(tag, "peinit: x\n", false);
        assert_eq!(line, format!("{expected}peinit: x\n"), "{tag:?}");
    }
}

/// Colour must change only the bytes, never the layout — otherwise a serial
/// log and a VT would disagree about where the message starts.
#[test]
fn colour_does_not_move_the_column() {
    for tag in [ConsoleTag::Ok, ConsoleTag::Failed, ConsoleTag::Crit] {
        let coloured = render_with(tag, "peinit: x\n", true);
        let plainly = render_with(tag, "peinit: x\n", false);
        assert_ne!(coloured, plainly, "{tag:?} should be coloured");
        assert_eq!(plain(&coloured), plainly, "{tag:?} layout changed");
    }
}

#[test]
fn untagged_progress_is_never_coloured() {
    let coloured = render_with(ConsoleTag::None, "peinit: x\n", true);
    assert!(!coloured.contains('\x1b'));
}

#[test]
fn the_message_keeps_its_own_newline_and_prefix() {
    let line = render_with(ConsoleTag::Ok, "peinit: service authd started\n", false);
    assert_eq!(line, "[  OK  ] peinit: service authd started\n");
}

/// The only test that touches the process-wide flag, which is why every other
/// test here takes colour as an argument instead. It restores what it found so
/// it cannot colour a later test's expectations.
#[test]
fn set_colour_drives_the_global() {
    let previous = COLOUR.load(Ordering::Relaxed);

    set_colour(false);
    assert!(!colour());
    set_colour(true);
    assert!(colour());

    COLOUR.store(previous, Ordering::Relaxed);
}

/// `TERM=dumb` is parsed off the kernel command line, not sniffed from an
/// environment peinit does not have. Last occurrence wins, as the kernel
/// resolves any repeated parameter.
#[test]
fn term_dumb_is_read_from_the_kernel_command_line() {
    use crate::init::KernelCommandLine;
    assert!(KernelCommandLine::parse("console=ttyS0 TERM=dumb loglevel=4").dumb_terminal);
    assert!(!KernelCommandLine::parse("console=ttyS0 loglevel=4").dumb_terminal);
    assert!(!KernelCommandLine::parse("TERM=dumb TERM=linux").dumb_terminal);
    assert!(KernelCommandLine::parse("TERM=linux TERM=dumb").dumb_terminal);
}

/// The banner is punctuation between stages, so both banners must be the same
/// width whatever they say — a ragged pair reads as a mistake.
#[test]
fn banners_are_a_fixed_width_whatever_the_stage() {
    for stage in [
        "prelude · initramfs · PID 1",
        "peinit · real root · PID 1 · Full boot",
        "peinit · real root · PID 1 · RECOVERY MODE",
    ] {
        let text = banner_with(stage, "v0.0.1", false);
        let lines: Vec<&str> = text.lines().collect();
        // blank, rule, content, rule, blank
        assert_eq!(lines.len(), 5, "{stage}");
        assert!(lines[0].is_empty() && lines[4].is_empty());
        for line in [lines[1], lines[2], lines[3]] {
            assert_eq!(line.chars().count(), BANNER_WIDTH, "{stage}: {line:?}");
        }
    }
}

/// A stage long enough to collide with the version must not silently produce a
/// line wider than the rules above and below it.
#[test]
fn an_overlong_stage_still_leaves_a_gap() {
    let text = banner_with(&"x".repeat(BANNER_WIDTH * 2), "v0.0.1", false);
    let content = text.lines().nth(2).unwrap();
    assert!(content.contains("x v0.0.1") || content.ends_with("v0.0.1"));
}

#[test]
fn the_peinit_banner_names_the_mode() {
    let text = peinit_banner("RECOVERY MODE");
    assert!(text.contains("peinit · real root · PID 1 · RECOVERY MODE"));
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
}

// ---------------------------------------------------------------------------
// Relayed output: lines peinit repeats from another process (an autorun script,
// a bootstrap daemon's stderr). Tested here rather than at either relay,
// because both use it and because `init::linux` needs libpeios to build.
// ---------------------------------------------------------------------------

/// A process sharing the console with peinit must not be able to steer it. An
/// unescaped SGR leaves every following line the wrong colour, and an
/// unescaped erase takes the boot log off the screen.
#[test]
fn control_characters_are_defanged() {
    let line = sanitise_relayed_line("applied \x1b[2J\x1b[31m20 files\x07");
    assert!(!line.contains('\x1b'), "{line:?}");
    assert!(!line.contains('\x07'), "{line:?}");
    assert!(line.starts_with("applied "));
    assert!(line.contains("20 files"));
}

/// Ordinary output survives untouched, or the sanitiser is worse than the
/// problem it solves.
#[test]
fn ordinary_relayed_text_is_unchanged() {
    let line = "applied 20 file(s), 92 key(s); 0 failed";
    assert_eq!(sanitise_relayed_line(line), line);
}

/// Cut, not dropped: a truncated line still names the misbehaving producer.
#[test]
fn an_overlong_relayed_line_is_cut_and_marked() {
    let line = sanitise_relayed_line(&"x".repeat(MAX_RELAYED_LINE * 3));
    assert_eq!(line.chars().count(), MAX_RELAYED_LINE + 1);
    assert!(line.ends_with('…'));
}

/// Multi-byte input must be cut on a character boundary. Slicing a UTF-8
/// string by bytes panics, and a panic here is PID 1 dying over a script's
/// output.
#[test]
fn relayed_truncation_counts_characters_not_bytes() {
    let line = sanitise_relayed_line(&"é".repeat(MAX_RELAYED_LINE * 2));
    assert_eq!(line.chars().count(), MAX_RELAYED_LINE + 1);
}

/// Relayed lines carry the producer's name and the blank tag: peinit does not
/// know whether somebody else's line was good news, and must not pretend to.
#[test]
fn relayed_lines_are_attributed_and_untagged() {
    let lines = relay_lines("10-apply-seeds.sh", "applied 20 files\napplied 1 file\n");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].0, ConsoleTag::None);
    assert_eq!(lines[0].1, "10-apply-seeds.sh: applied 20 files\n");
    assert_eq!(lines[1].1, "10-apply-seeds.sh: applied 1 file\n");
}

/// A trailing newline should not spend a line of the console saying nothing.
#[test]
fn blank_relayed_lines_are_dropped() {
    let lines = relay_lines("x", "a\n\n   \nb\n");
    assert_eq!(lines.len(), 2);
    assert!(lines[0].1.ends_with("a\n"));
    assert!(lines[1].1.ends_with("b\n"));
}

/// A runaway producer is cut off and the remainder counted, so it cannot
/// scroll the boot log off the screen.
#[test]
fn a_flood_is_capped_and_summarised() {
    let captured = "line\n".repeat(MAX_RELAYED_LINES + 25);
    let lines = relay_lines("noisy", &captured);
    assert_eq!(lines.len(), MAX_RELAYED_LINES + 1);
    let (tag, last) = lines.last().unwrap();
    assert_eq!(*tag, ConsoleTag::Warn);
    assert!(last.contains("25 further line(s) not shown"), "{last:?}");
}

/// Exactly at the cap, nothing is suppressed and no summary appears.
#[test]
fn output_at_the_cap_needs_no_summary() {
    let captured = "line\n".repeat(MAX_RELAYED_LINES);
    let lines = relay_lines("x", &captured);
    assert_eq!(lines.len(), MAX_RELAYED_LINES);
    assert!(lines.iter().all(|(tag, _)| *tag == ConsoleTag::None));
}

/// Escape sequences from a relayed producer must not survive into a line that
/// shares a device with peinit's own output.
#[test]
fn relayed_lines_are_sanitised() {
    let lines = relay_lines("x", "before\x1b[2Jafter\n");
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].1.contains('\x1b'), "{:?}", lines[0].1);
}
