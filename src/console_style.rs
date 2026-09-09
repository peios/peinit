//! The Peios boot console line format.
//!
//! Every line peinit writes to `/dev/console` has the same shape: a
//! fixed-width outcome tag, then the component name, then the message.
//!
//! ```text
//! [  OK  ] peinit: service authd started
//! [FAILED] peinit: service netd failed to launch: token materialisation failed
//! [      ] peinit: phase 1 starting
//! ```
//!
//! The tag column is the whole point. An operator scanning a boot reads down
//! one column instead of reading sentences, and a failure stops being a line
//! that looks exactly like the twenty around it.
//!
//! # Why the rendering lives here and not at the call sites
//!
//! Producers say *what happened*; this module decides *how it looks*. Two
//! reasons that split is worth the indirection:
//!
//! - Colour depends on the terminal, which is a fact about the device. A
//!   producer deep in the supervisor has no business knowing it, and there are
//!   dozens of producers and one console.
//! - The peinit TRM already anticipates forwarding suppressed messages to
//!   eventd. A second sink wants the structured form, not a coloured string it
//!   has to strip escapes back out of.
//!
//! # Not shared with prelude
//!
//! prelude renders the same format from its own copy of this logic, and that
//! duplication is deliberate. prelude is a size-critical initramfs PID 1 with
//! *zero* dependencies, and the only crate it could share with peinit
//! (`peios-rs`) binds libpeios over FFI. Coupling an initramfs PID 1 to the
//! kernel-boundary library to save forty lines of string formatting is a bad
//! trade. What is shared is the *format*, which is specified in the docs tree,
//! not the code.

// Every real consumer of the renderer sits behind `peios-boundary` — the
// runtime's `ConsoleSink` and the Linux `InitPlatform` — so a default build has
// only this module's own tests using it. Allowed rather than gated, because the
// format has to keep compiling and being tested without the feature: it is the
// thing prelude and the hook scripts are held to, and a format nobody can build
// is a format that drifts. Same situation, and the same answer, as
// `runtime::console::QuietPolicy`.
#![cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};

/// Width of the tag field between the brackets. Every tag is padded to this,
/// so the component name always starts at the same column.
const TAG_WIDTH: usize = 6;

/// Total width of a banner, including the two leading spaces. Fits inside 80
/// columns with room to spare, which is what both a VT and a serial console
/// give you.
const BANNER_WIDTH: usize = 64;

/// What happened, as far as the console is concerned.
///
/// Deliberately *not* the same axis as [`crate::runtime::console::ConsoleSeverity`].
/// Severity decides who is allowed to print under `peios.quiet` and terminal
/// ownership; the tag decides how the line looks. They correlate but neither
/// determines the other — a Normal service failing and a Critical service
/// failing both render `[FAILED]` while carrying different severities — and
/// collapsing them would quietly change what `peios.quiet=2` suppresses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConsoleTag {
    /// Progress with no outcome yet. Renders as an empty bracket, which holds
    /// the column while receding: `OK` and `FAILED` are what should catch the
    /// eye, and a screen where every line is tagged has no emphasis left.
    #[default]
    None,
    /// It worked. The verb in the message should agree — an `Ok` line reads
    /// "mounted /bin", never "mounting /bin".
    Ok,
    /// Deliberately not done. A dependency failed, safe mode excluded it, a
    /// condition was not met.
    Skip,
    /// Something is wrong but the boot continues.
    Warn,
    /// Something did not work.
    Failed,
    /// The machine is about to be lost: recovery, a halt with no shell, a
    /// Critical service forcing a reboot.
    Crit,
    /// Output that is already its own punctuation and must not be given a tag
    /// column: a stage banner. Distinct from [`ConsoleTag::None`], which is an
    /// untagged *line* and still occupies the column so the text below it
    /// stays aligned. `Bare` passes the string through untouched.
    Bare,
}

impl ConsoleTag {
    fn word(self) -> &'static str {
        match self {
            Self::None | Self::Bare => "",
            Self::Ok => "OK",
            Self::Skip => "SKIP",
            Self::Warn => "WARN",
            Self::Failed => "FAILED",
            Self::Crit => "CRIT",
        }
    }

    /// The SGR parameters for this tag, or `None` to leave it uncoloured.
    fn sgr(self) -> Option<&'static str> {
        match self {
            Self::None | Self::Bare => None,
            Self::Ok => Some("1;32"),      // bold green
            Self::Skip => Some("1;33"),    // bold yellow
            Self::Warn => Some("1;33"),    // bold yellow
            Self::Failed => Some("1;31"),  // bold red
            Self::Crit => Some("1;37;41"), // bold white on red
        }
    }
}

/// Whether to emit SGR escapes.
///
/// A global rather than a field on the sink because Phase 1 writes to the
/// console through a different path than the runtime does (raw strings via
/// `InitPlatform::write_console_message`, rather than `ConsoleMessage` through
/// the `ConsoleSink`), and both have to agree. Set once, early, before
/// anything is written.
static COLOUR: AtomicBool = AtomicBool::new(true);

/// Set colour from the parsed command line, systemd's rule: colour unless the
/// terminal is declared dumb.
///
/// Called once, as soon as the command line has been read and before anything
/// tagged is written. Until then colour is on, which is the right default for
/// the handful of lines peinit emits before it can know better — they are also
/// the only evidence peinit started at all.
pub fn set_colour(enabled: bool) {
    COLOUR.store(enabled, Ordering::Relaxed);
}

fn colour() -> bool {
    COLOUR.load(Ordering::Relaxed)
}

/// Render one console line, using the process-wide colour setting.
///
/// `text` is expected to carry its own trailing newline and its own
/// `component: ` prefix, which is how every producer already writes it.
pub fn render(tag: ConsoleTag, text: &str) -> String {
    render_with(tag, text, colour())
}

/// The renderer proper, with colour passed in rather than read from the
/// global. Everything real goes through [`render`]; this exists so the layout
/// can be tested without a process-wide flag that parallel tests race on.
fn render_with(tag: ConsoleTag, text: &str, colour: bool) -> String {
    // A banner brings its own layout and its own blank lines. Giving it a tag
    // column would indent it out of alignment with the rules above and below.
    if tag == ConsoleTag::Bare {
        return text.to_string();
    }
    let word = tag.word();
    // Centre the word in the field, leaning left when it cannot be even, so
    // `OK` sits as `[  OK  ]` and `FAILED` fills its bracket exactly.
    let spare = TAG_WIDTH.saturating_sub(word.len());
    let left = spare / 2;
    let right = spare - left;

    let mut out = String::with_capacity(text.len() + TAG_WIDTH + 16);
    out.push('[');
    out.push_str(&" ".repeat(left));
    match (colour, tag.sgr()) {
        (true, Some(sgr)) => {
            out.push_str("\x1b[");
            out.push_str(sgr);
            out.push('m');
            out.push_str(word);
            out.push_str("\x1b[0m");
        }
        _ => out.push_str(word),
    }
    out.push_str(&" ".repeat(right));
    out.push_str("] ");
    out.push_str(text);
    out
}

/// The longest line peinit will relay from another process before cutting it.
///
/// Relayed output is arbitrary: an autorun script the composer put in the
/// image, or a bootstrap daemon's stderr. A runaway `cat` of a binary would
/// otherwise scroll the console past everything that mattered. Cut rather than
/// drop — a truncated line still names the misbehaving producer.
pub const MAX_RELAYED_LINE: usize = 512;

/// The greatest number of lines peinit will relay from one producer. Past this
/// the rest is counted and summarised rather than printed.
pub const MAX_RELAYED_LINES: usize = 200;

/// Make one line of somebody else's output safe to put on a shared console.
///
/// Nothing stops a relayed process emitting escape sequences, and peinit shares
/// the device with it. Left alone, a stray `ESC[2J` takes the boot log off the
/// screen and a stray SGR leaves every following line the wrong colour —
/// peinit's own included. Control characters become `?`, which keeps the line's
/// shape and length while making it inert.
///
/// Truncation counts CHARACTERS, not bytes: slicing a UTF-8 string on a byte
/// boundary panics, and a panic here is PID 1 dying over a script's output.
pub fn sanitise_relayed_line(line: &str) -> String {
    let mut out: String = line
        .chars()
        .take(MAX_RELAYED_LINE)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if line.chars().count() > MAX_RELAYED_LINE {
        out.push('…');
    }
    out
}

/// Turn another process's captured output into console lines.
///
/// Used by both relays — the autorun runner and the Phase 1 registryd failure
/// path — so that output peinit repeats looks the same wherever it came from.
///
/// Every line gets [`ConsoleTag::None`]. peinit has no idea whether a given
/// line of somebody else's output is good news; the producer knows and peinit
/// does not, and guessing from prose is how a rename silently turns an error
/// green. The producer's *outcome* is reported separately by the caller, from
/// the exit code or the failure it caught, which is the thing peinit does know.
///
/// Blank lines are dropped: a script ending with a newline should not spend a
/// line of the console saying nothing.
pub fn relay_lines(component: &str, captured: &str) -> Vec<(ConsoleTag, String)> {
    let mut out = Vec::new();
    let mut shown = 0usize;
    let mut suppressed = 0usize;
    for line in captured.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if shown < MAX_RELAYED_LINES {
            out.push((
                ConsoleTag::None,
                format!("{component}: {}\n", sanitise_relayed_line(line)),
            ));
            shown += 1;
        } else {
            suppressed += 1;
        }
    }
    if suppressed > 0 {
        out.push((
            ConsoleTag::Warn,
            format!("{component}: {suppressed} further line(s) not shown\n"),
        ));
    }
    out
}

/// A stage banner: the punctuation between one PID 1 and the next.
///
/// ```text
///
///   ══════════════════════════════════════════════════════════════
///    peinit · real root · PID 1 · Full boot                 v0.0.1
///   ══════════════════════════════════════════════════════════════
///
/// ```
///
/// `stage` names where the boot has got to, in words. Not numbered, because
/// peinit already prints `phase 1` and `phase 2` for its own internal phases
/// and two overlapping numbered sequences on one console is a trap.
pub fn banner(stage: &str, version: &str) -> String {
    banner_with(stage, version, colour())
}

/// As [`render_with`]: the real implementation, colour passed in so the layout
/// is testable without touching the global.
fn banner_with(stage: &str, version: &str, colour: bool) -> String {
    let rule = "═".repeat(BANNER_WIDTH - 2);
    // Character count, not byte length: the rule is multi-byte and the stage
    // may be too (the separator is a middle dot).
    let used = 3 + stage.chars().count() + version.chars().count();
    let gap = BANNER_WIDTH.saturating_sub(used).max(1);

    let mut out = String::new();
    out.push('\n');
    if colour {
        // Dim the rules so the text between them is what reads.
        out.push_str(&format!("  \x1b[2m{rule}\x1b[0m\n"));
        out.push_str(&format!(
            "   \x1b[1m{stage}\x1b[0m{}\x1b[2m{version}\x1b[0m\n",
            " ".repeat(gap)
        ));
        out.push_str(&format!("  \x1b[2m{rule}\x1b[0m\n"));
    } else {
        out.push_str(&format!("  {rule}\n"));
        out.push_str(&format!("   {stage}{}{version}\n", " ".repeat(gap)));
        out.push_str(&format!("  {rule}\n"));
    }
    out.push('\n');
    out
}

/// peinit's own banner text, given the mode it is booting in.
pub fn peinit_banner(mode: &str) -> String {
    banner(
        &format!("peinit · real root · PID 1 · {mode}"),
        concat!("v", env!("CARGO_PKG_VERSION")),
    )
}

#[cfg(test)]
mod tests;
