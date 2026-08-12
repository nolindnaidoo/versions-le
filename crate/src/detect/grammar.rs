//! The constraint grammars, and the one question the checks ask of them:
//! **can a single version satisfy both?**
//!
//! Every modelled constraint becomes a union of intervals over
//! `semver::Version`. Two constraints are disjoint when no interval of
//! one meets any interval of the other. That is exact for the grammars
//! here, which is why `disjoint-constraint` is a claim and not a guess.
//!
//! **A grammar this module does not model is `Unknown`, never
//! approximated.** Approximating would manufacture a conflict out of a
//! syntax the tool does not understand, and a fabricated conflict
//! against somebody's real manifest is the one output this crate cannot
//! afford. `Malformed` is the narrower verdict: shaped like a constraint
//! of that ecosystem, and broken. The difference is who is at fault, and
//! the tool only blames the file when it is sure.
//!
//! One deliberate imprecision, in the safe direction: a prerelease is
//! treated as an ordinary point on the line, so `^1` is modelled as
//! `[1.0.0, 2.0.0)` and therefore admits `2.0.0-rc.1`, which neither npm
//! nor Cargo would install. That makes modelled ranges very slightly
//! *wider* than the real ones — and a wider range can only ever hide a
//! disjointness, never invent one.

use semver::{BuildMetadata, Comparator, Op, Prerelease, Version, VersionReq};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Bound {
    version: Version,
    inclusive: bool,
}

/// One contiguous stretch of the version line. `None` is unbounded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Interval {
    lower: Option<Bound>,
    upper: Option<Bound>,
}

/// A union of intervals — `||` in npm's grammar produces more than one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Range {
    intervals: Vec<Interval>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Constraint {
    /// Modelled, and therefore comparable.
    Range(Range),
    /// In a syntax this tool does not model. Carries the reason, and is
    /// excluded from every comparison.
    Unknown(&'static str),
    /// Shaped like a constraint of its ecosystem, and broken.
    Malformed(&'static str),
}

impl Constraint {
    pub(crate) fn range(&self) -> Option<&Range> {
        match self {
            Self::Range(range) => Some(range),
            _ => None,
        }
    }
}

impl Range {
    fn any() -> Self {
        Self {
            intervals: vec![Interval::default()],
        }
    }

    /// The lowest version this range can admit, if it has one.
    ///
    /// Used by the MSRV check, which asks "how old a toolchain does this
    /// permit", and by the prerelease check, which asks whether the
    /// floor is a prerelease.
    pub(crate) fn floor(&self) -> Option<&Version> {
        self.intervals
            .iter()
            .filter_map(|interval| interval.lower.as_ref())
            .map(|bound| &bound.version)
            .min()
    }

    pub(crate) fn mentions_prerelease(&self) -> bool {
        self.intervals.iter().any(|interval| {
            [interval.lower.as_ref(), interval.upper.as_ref()]
                .into_iter()
                .flatten()
                .any(|bound| !bound.version.pre.is_empty())
        })
    }
}

/// Whether no single version can satisfy both.
///
/// The strongest finding this crate makes, so it is only ever asked of
/// two modelled ranges — an `Unknown` never reaches here.
pub(crate) fn disjoint(left: &Range, right: &Range) -> bool {
    !left.intervals.iter().any(|one| {
        right
            .intervals
            .iter()
            .any(|other| meet(one, other).is_some())
    })
}

/// The intersection of two intervals, or `None` when they do not touch.
fn meet(left: &Interval, right: &Interval) -> Option<Interval> {
    let lower = max_bound(left.lower.as_ref(), right.lower.as_ref());
    let upper = min_bound(left.upper.as_ref(), right.upper.as_ref());
    let inhabited = match (&lower, &upper) {
        (Some(low), Some(high)) => match low.version.cmp(&high.version) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Equal => low.inclusive && high.inclusive,
            std::cmp::Ordering::Greater => false,
        },
        _ => true,
    };
    inhabited.then_some(Interval { lower, upper })
}

fn max_bound(left: Option<&Bound>, right: Option<&Bound>) -> Option<Bound> {
    let (Some(left), Some(right)) = (left, right) else {
        return left.or(right).cloned();
    };
    Some(match left.version.cmp(&right.version) {
        std::cmp::Ordering::Greater => left.clone(),
        std::cmp::Ordering::Less => right.clone(),
        // Same version, so the tighter inclusivity wins.
        std::cmp::Ordering::Equal => Bound {
            version: left.version.clone(),
            inclusive: left.inclusive && right.inclusive,
        },
    })
}

fn min_bound(left: Option<&Bound>, right: Option<&Bound>) -> Option<Bound> {
    let (Some(left), Some(right)) = (left, right) else {
        return left.or(right).cloned();
    };
    Some(match left.version.cmp(&right.version) {
        std::cmp::Ordering::Less => left.clone(),
        std::cmp::Ordering::Greater => right.clone(),
        std::cmp::Ordering::Equal => Bound {
            version: left.version.clone(),
            inclusive: left.inclusive && right.inclusive,
        },
    })
}

// ---------------------------------------------------------------------
// Cargo
// ---------------------------------------------------------------------

/// A `[dependencies]` version string.
///
/// A bare `"1.0.200"` is a caret requirement here — the opposite of npm,
/// where the same eight characters mean exactly one version. That single
/// difference is most of why conflict analysis is scoped per ecosystem.
pub(crate) fn cargo(raw: &str) -> Constraint {
    let text = raw.trim();
    if text.is_empty() {
        return Constraint::Malformed("an empty version requirement");
    }
    let Ok(request) = VersionReq::parse(text) else {
        return Constraint::Malformed("not a Cargo version requirement");
    };
    if request.comparators.is_empty() {
        return Constraint::Range(Range::any());
    }

    let mut interval = Interval::default();
    for comparator in &request.comparators {
        let Some(next) = comparator_interval(comparator) else {
            return Constraint::Unknown("a Cargo comparator this tool does not model");
        };
        let Some(merged) = meet(&interval, &next) else {
            return Constraint::Malformed("a requirement no version can satisfy");
        };
        interval = merged;
    }
    Constraint::Range(Range {
        intervals: vec![interval],
    })
}

fn comparator_interval(comparator: &Comparator) -> Option<Interval> {
    let partial = Partial {
        major: Some(comparator.major),
        minor: comparator.minor,
        patch: comparator.patch,
        pre: comparator.pre.clone(),
    };
    match comparator.op {
        Op::Exact | Op::Wildcard => Some(exactly(&partial)),
        Op::Greater => Some(above(&partial)),
        Op::GreaterEq => Some(at_least(&partial)),
        Op::Less => Some(below(&partial)),
        Op::LessEq => Some(at_most(&partial)),
        Op::Tilde => Some(tilde(&partial)),
        Op::Caret => Some(caret(&partial)),
        // `Op` is `#[non_exhaustive]`: a comparator a future semver adds
        // is refused rather than silently read as something else.
        _ => None,
    }
}

/// A bare minimum-version declaration: `rust-version`, and go.mod's `go`
/// directive. Neither is a range — both say "not older than this".
pub(crate) fn minimum(raw: &str) -> Constraint {
    let Some(partial) = partial(raw.trim()) else {
        return Constraint::Malformed("not a version");
    };
    if partial.major.is_none() {
        return Constraint::Unknown("a wildcard minimum version");
    }
    Constraint::Range(Range {
        intervals: vec![at_least(&partial)],
    })
}

// ---------------------------------------------------------------------
// npm
// ---------------------------------------------------------------------

/// An npm range: `^1.2.3`, `>=1 <2`, `1.2.3 - 2.0.0`, `1.x`, `~0.4`, and
/// alternatives joined by `||`.
pub(crate) fn npm(raw: &str) -> Constraint {
    let text = raw.trim();
    if text.is_empty() {
        return Constraint::Range(Range::any());
    }
    if let Some(reason) = npm_unmodelled(text) {
        return Constraint::Unknown(reason);
    }

    let mut intervals = Vec::new();
    for alternative in text.split("||") {
        match npm_set(alternative.trim()) {
            Ok(Some(interval)) => intervals.push(interval),
            // An alternative that no version satisfies contributes
            // nothing to the union, which is what npm does too.
            Ok(None) => {}
            Err(constraint) => return constraint,
        }
    }
    if intervals.is_empty() {
        // Every alternative was self-contradictory, so nothing satisfies
        // the whole range. An empty union would make this constraint
        // disjoint with everything, which is a finding the manifest did
        // not earn — the manifest earned a `malformed-constraint`.
        return Constraint::Malformed("a range no version can satisfy");
    }
    Constraint::Range(Range { intervals })
}

/// One `||` alternative: whitespace-separated simples, intersected, or a
/// hyphen range.
fn npm_set(text: &str) -> Result<Option<Interval>, Constraint> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return Ok(Some(Interval::default()));
    }
    if let [low, "-", high] = tokens.as_slice() {
        return hyphen(low, high).map(Some);
    }

    let mut interval = Interval::default();
    for token in tokens {
        let next = npm_simple(token)?;
        let Some(merged) = meet(&interval, &next) else {
            return Ok(None);
        };
        interval = merged;
    }
    Ok(Some(interval))
}

fn hyphen(low: &str, high: &str) -> Result<Interval, Constraint> {
    let low = partial(low).ok_or(Constraint::Malformed("not a version range"))?;
    let high = partial(high).ok_or(Constraint::Malformed("not a version range"))?;
    Ok(Interval {
        lower: at_least(&low).lower,
        upper: at_most(&high).upper,
    })
}

fn npm_simple(token: &str) -> Result<Interval, Constraint> {
    let (operator, rest) = split_operator(token);
    if !operator.is_empty() && rest.trim().is_empty() {
        return Err(Constraint::Malformed("an operator with no version"));
    }
    let Some(partial) = partial(rest) else {
        return Err(npm_refusal(token));
    };
    // `*`, `x` and the empty spec name no version at all, so no operator
    // applied to one narrows anything.
    if partial.major.is_none() {
        return Ok(Interval::default());
    }
    Ok(match operator {
        "^" => caret(&partial),
        "~" => tilde(&partial),
        ">=" => at_least(&partial),
        ">" => above(&partial),
        "<=" => at_most(&partial),
        "<" => below(&partial),
        // Bare and `=` are the same thing in npm: a complete version is
        // exactly that version, a partial one is an X-range.
        _ => exactly(&partial),
    })
}

/// A token that did not parse is `Unknown` when it reads as a dist tag
/// and `Malformed` only when it reads as a broken version. The tool
/// blames the manifest only when it is sure.
fn npm_refusal(token: &str) -> Constraint {
    let word = token
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if word {
        return Constraint::Unknown("a dist tag is a moving target, not a version range");
    }
    Constraint::Malformed("not an npm version range")
}

/// The npm specifiers that are not ranges at all.
fn npm_unmodelled(text: &str) -> Option<&'static str> {
    let starts = |prefix: &str| text.starts_with(prefix);
    if text.contains("://") || starts("git@") {
        return Some("a URL or git specifier resolves outside the registry");
    }
    if text.contains(':') {
        // `workspace:`, `npm:`, `file:`, `link:`, `portal:`, `catalog:`.
        return Some("a protocol specifier resolves outside the registry");
    }
    if text.contains('/') || starts(".") {
        return Some("a path or repository shorthand resolves outside the registry");
    }
    None
}

// ---------------------------------------------------------------------
// PEP 440
// ---------------------------------------------------------------------

/// A PEP 440 version specifier — the part of a requirement after the
/// name. Comma-separated clauses are intersected.
///
/// `~=`, `!=`, `===` and `==1.2.*` are **refused**, not approximated.
/// Compatible-release and exclusion clauses do not become intervals
/// without inventing semantics, and inventing them here would put a
/// fabricated range into a conflict claim.
pub(crate) fn python(raw: &str) -> Constraint {
    let text = raw.trim();
    if text.is_empty() {
        return Constraint::Range(Range::any());
    }

    let mut interval = Interval::default();
    for clause in text.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            return Constraint::Malformed("an empty version clause");
        }
        let next = match python_clause(clause) {
            Ok(next) => next,
            Err(constraint) => return constraint,
        };
        let Some(merged) = meet(&interval, &next) else {
            return Constraint::Malformed("a specifier no version can satisfy");
        };
        interval = merged;
    }
    Constraint::Range(Range {
        intervals: vec![interval],
    })
}

fn python_clause(clause: &str) -> Result<Interval, Constraint> {
    for (operator, reason) in [
        ("~=", "PEP 440 compatible-release (~=)"),
        ("!=", "PEP 440 version exclusion (!=)"),
        ("===", "PEP 440 arbitrary equality (===)"),
    ] {
        if clause.starts_with(operator) {
            return Err(Constraint::Unknown(reason));
        }
    }

    let (operator, rest) = split_operator(clause);
    let rest = rest.trim();
    if rest.contains('*') {
        return Err(Constraint::Unknown("a PEP 440 wildcard release clause"));
    }
    let Some(partial) = pep440(rest) else {
        return Err(python_refusal(rest));
    };
    Ok(match operator {
        ">=" => at_least(&partial),
        ">" => above_release(&partial),
        "<=" => at_most_release(&partial),
        "<" => below(&partial),
        "==" => exactly_release(&partial),
        _ => return Err(Constraint::Unknown("a PEP 440 clause with no operator")),
    })
}

fn python_refusal(text: &str) -> Constraint {
    for marker in ["!", "+", "post", "dev", "*"] {
        if text.contains(marker) {
            return Constraint::Unknown("a PEP 440 version form this tool does not model");
        }
    }
    Constraint::Malformed("not a PEP 440 version")
}

/// `1.2`, `2.0.0rc1`, `1.0b2` — the release-plus-prerelease subset this
/// tool models. Epochs, post- and dev-releases and local versions are
/// deliberately not here; they reach `python_refusal` instead.
fn pep440(text: &str) -> Option<Partial> {
    let (release, pre) = split_pep440_pre(text)?;
    let mut partial = partial(release)?;
    partial.pre = pre;
    Some(partial)
}

fn split_pep440_pre(text: &str) -> Option<(&str, Prerelease)> {
    let boundary = text.find(|c: char| c.is_ascii_alphabetic());
    let Some(boundary) = boundary else {
        return Some((text, Prerelease::EMPTY));
    };
    let (release, tail) = text.split_at(boundary);
    let (label, number) = tail.split_at(tail.find(|c: char| c.is_ascii_digit()).unwrap_or(0));
    if !matches!(label, "a" | "b" | "rc") || number.is_empty() {
        return None;
    }
    let pre = Prerelease::new(&format!("{label}.{number}")).ok()?;
    Some((release.trim_end_matches('.'), pre))
}

// ---------------------------------------------------------------------
// go.mod, CI action refs and CI tool versions
// ---------------------------------------------------------------------

/// A `require` version. Go module versions are always exact — there is
/// no range grammar to model.
pub(crate) fn go(raw: &str) -> Constraint {
    let text = raw.trim().trim_start_matches('v');
    let Ok(version) = Version::parse(text) else {
        return Constraint::Malformed("not a go module version");
    };
    Constraint::Range(Range {
        intervals: vec![point(version)],
    })
}

/// The `@ref` of a workflow `uses:`.
pub(crate) fn action_ref(raw: &str) -> Constraint {
    let text = raw.trim();
    if is_commit_sha(text) {
        return Constraint::Unknown("a commit SHA is a pin, but not a version");
    }
    let Some(partial) = partial(text) else {
        return Constraint::Unknown("a branch or tag name is a moving reference, not a version");
    };
    if partial.major.is_none() {
        return Constraint::Unknown("a wildcard action reference");
    }
    // `@v4` is the whole v4 line, exactly as an npm `4` would be.
    Constraint::Range(Range {
        intervals: vec![exactly(&partial)],
    })
}

pub(crate) fn is_commit_sha(text: &str) -> bool {
    // 40 characters is a full SHA-1; a shorter hex string is only read
    // as a SHA when it carries a letter, so `1234567` stays a version.
    let hex = text.chars().all(|c| c.is_ascii_hexdigit());
    hex && (text.len() == 40 || (text.len() >= 7 && text.chars().any(|c| c.is_ascii_alphabetic())))
}

/// A `node-version:`-style workflow input. The setup actions take npm
/// range syntax, so this is npm's grammar with the moving labels named.
pub(crate) fn ci_tool(raw: &str) -> Constraint {
    let text = raw.trim();
    let moving = matches!(
        text,
        "latest" | "stable" | "nightly" | "beta" | "current" | "node" | "lts"
    ) || text.starts_with("lts/");
    if moving {
        return Constraint::Unknown("a channel name is a moving target, not a version");
    }
    npm(text)
}

// ---------------------------------------------------------------------
// Partial versions, shared by every grammar above
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Partial {
    /// `None` is a wildcard: `*`, `x`, or nothing at all.
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Prerelease,
}

fn partial(text: &str) -> Option<Partial> {
    let text = text.trim();
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    if text.is_empty() || is_wildcard(text) {
        return Some(Partial {
            major: None,
            minor: None,
            patch: None,
            pre: Prerelease::EMPTY,
        });
    }

    let text = text.split('+').next().unwrap_or(text);
    let (release, pre) = match text.split_once('-') {
        Some((release, tail)) => (release, Prerelease::new(tail).ok()?),
        None => (text, Prerelease::EMPTY),
    };

    let mut numbers = [None, None, None];
    let mut segments = release.split('.');
    for slot in &mut numbers {
        let Some(segment) = segments.next() else {
            break;
        };
        if is_wildcard(segment) {
            break;
        }
        *slot = Some(segment.parse::<u64>().ok()?);
    }
    // A fourth segment is somebody else's versioning scheme.
    if segments.next().is_some_and(|segment| !segment.is_empty()) {
        return None;
    }
    numbers[0]?;

    Some(Partial {
        major: numbers[0],
        minor: numbers[1],
        patch: numbers[2],
        pre,
    })
}

fn is_wildcard(text: &str) -> bool {
    matches!(text, "*" | "x" | "X")
}

fn split_operator(token: &str) -> (&str, &str) {
    for operator in [">=", "<=", "==", "^", "~", ">", "<", "="] {
        if let Some(rest) = token.strip_prefix(operator) {
            return (operator, rest);
        }
    }
    ("", token)
}

fn build(major: u64, minor: u64, patch: u64, pre: &Prerelease) -> Version {
    Version {
        major,
        minor,
        patch,
        pre: pre.clone(),
        build: BuildMetadata::EMPTY,
    }
}

fn floor(partial: &Partial) -> Version {
    build(
        partial.major.unwrap_or(0),
        partial.minor.unwrap_or(0),
        partial.patch.unwrap_or(0),
        &partial.pre,
    )
}

/// The first version *above* everything the partial names: `1` ceils to
/// `2.0.0`, `1.2` to `1.3.0`. A complete version has no ceiling — it
/// names one point.
fn ceiling(partial: &Partial) -> Option<Version> {
    let major = partial.major?;
    let Some(minor) = partial.minor else {
        return Some(build(major.saturating_add(1), 0, 0, &Prerelease::EMPTY));
    };
    if partial.patch.is_none() {
        return Some(build(major, minor.saturating_add(1), 0, &Prerelease::EMPTY));
    }
    None
}

fn point(version: Version) -> Interval {
    Interval {
        lower: Some(Bound {
            version: version.clone(),
            inclusive: true,
        }),
        upper: Some(Bound {
            version,
            inclusive: true,
        }),
    }
}

fn at_least(partial: &Partial) -> Interval {
    Interval {
        lower: Some(Bound {
            version: floor(partial),
            inclusive: true,
        }),
        upper: None,
    }
}

/// `>1.2` means `>=1.3.0` — a partial is a whole line, and being above
/// it means being above all of it.
fn above(partial: &Partial) -> Interval {
    match ceiling(partial) {
        Some(version) => Interval {
            lower: Some(Bound {
                version,
                inclusive: true,
            }),
            upper: None,
        },
        None => Interval {
            lower: Some(Bound {
                version: floor(partial),
                inclusive: false,
            }),
            upper: None,
        },
    }
}

fn below(partial: &Partial) -> Interval {
    Interval {
        lower: None,
        upper: Some(Bound {
            version: floor(partial),
            inclusive: false,
        }),
    }
}

fn at_most(partial: &Partial) -> Interval {
    match ceiling(partial) {
        Some(version) => Interval {
            lower: None,
            upper: Some(Bound {
                version,
                inclusive: false,
            }),
        },
        None => Interval {
            lower: None,
            upper: Some(Bound {
                version: floor(partial),
                inclusive: true,
            }),
        },
    }
}

fn exactly(partial: &Partial) -> Interval {
    match ceiling(partial) {
        Some(version) => Interval {
            lower: Some(Bound {
                version: floor(partial),
                inclusive: true,
            }),
            upper: Some(Bound {
                version,
                inclusive: false,
            }),
        },
        None => point(floor(partial)),
    }
}

/// PEP 440 zero-pads instead of treating a short release as a line:
/// `==1.2` is the single version `1.2`, not `1.2.x`.
fn exactly_release(partial: &Partial) -> Interval {
    point(floor(partial))
}

fn above_release(partial: &Partial) -> Interval {
    Interval {
        lower: Some(Bound {
            version: floor(partial),
            inclusive: false,
        }),
        upper: None,
    }
}

fn at_most_release(partial: &Partial) -> Interval {
    Interval {
        lower: None,
        upper: Some(Bound {
            version: floor(partial),
            inclusive: true,
        }),
    }
}

fn tilde(partial: &Partial) -> Interval {
    let major = partial.major.unwrap_or(0);
    let upper = match partial.minor {
        Some(minor) => build(major, minor.saturating_add(1), 0, &Prerelease::EMPTY),
        None => build(major.saturating_add(1), 0, 0, &Prerelease::EMPTY),
    };
    bounded(floor(partial), upper)
}

/// `^1.2.3` is `[1.2.3, 2.0.0)`, but below 1.0 the breaking axis moves
/// left: `^0.2.3` is `[0.2.3, 0.3.0)` and `^0.0.3` is `[0.0.3, 0.0.4)`.
fn caret(partial: &Partial) -> Interval {
    let major = partial.major.unwrap_or(0);
    if major > 0 {
        return bounded(
            floor(partial),
            build(major.saturating_add(1), 0, 0, &Prerelease::EMPTY),
        );
    }
    let upper = match (partial.minor, partial.patch) {
        (Some(0), Some(patch)) => build(0, 0, patch.saturating_add(1), &Prerelease::EMPTY),
        (Some(minor), _) => build(0, minor.saturating_add(1), 0, &Prerelease::EMPTY),
        (None, _) => build(1, 0, 0, &Prerelease::EMPTY),
    };
    bounded(floor(partial), upper)
}

fn bounded(lower: Version, upper: Version) -> Interval {
    Interval {
        lower: Some(Bound {
            version: lower,
            inclusive: true,
        }),
        upper: Some(Bound {
            version: upper,
            inclusive: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(constraint: Constraint) -> Range {
        match constraint {
            Constraint::Range(range) => range,
            other => panic!("expected a modelled range, got {other:?}"),
        }
    }

    fn overlap(left: Constraint, right: Constraint) -> bool {
        !disjoint(&range(left), &range(right))
    }

    #[test]
    fn a_bare_cargo_version_is_a_caret_and_a_bare_npm_version_is_exact() {
        // The single most important difference between the two grammars,
        // and the reason comparison is scoped per ecosystem.
        assert!(overlap(cargo("1.0.200"), cargo("1.9.0")));
        assert!(!overlap(npm("1.0.200"), npm("1.9.0")));
    }

    #[test]
    fn a_caret_stops_at_the_next_major() {
        assert!(overlap(cargo("^1.2.3"), cargo("1.9.9")));
        assert!(!overlap(cargo("^1.2.3"), cargo("2.0.0")));
    }

    #[test]
    fn a_zero_x_caret_moves_the_breaking_axis_left() {
        assert!(!overlap(npm("^0.2.3"), npm("0.3.0")));
        assert!(!overlap(npm("^0.0.3"), npm("0.0.4")));
        assert!(overlap(npm("^0.2.3"), npm("0.2.9")));
    }

    #[test]
    fn a_tilde_stops_at_the_next_minor() {
        assert!(overlap(npm("~1.2.3"), npm("1.2.9")));
        assert!(!overlap(npm("~1.2.3"), npm("1.3.0")));
    }

    #[test]
    fn two_pinned_majors_are_disjoint() {
        assert!(disjoint(&range(cargo("^1.0")), &range(cargo("^2.0"))));
        assert!(disjoint(&range(npm("1.x")), &range(npm("2.x"))));
    }

    #[test]
    fn a_narrower_pin_inside_a_wider_one_is_not_disjoint() {
        // The distinction the whole `disjoint-constraint` finding rests
        // on: different is not the same as unsatisfiable.
        assert!(overlap(cargo("1"), cargo("1.0.200")));
        assert!(overlap(npm(">=1.0.0"), npm("1.4.2")));
    }

    #[test]
    fn an_npm_union_takes_either_side() {
        assert!(overlap(npm("^1.0.0 || ^2.0.0"), npm("2.1.0")));
        assert!(!overlap(npm("^1.0.0 || ^2.0.0"), npm("3.1.0")));
    }

    #[test]
    fn npm_ands_a_space_separated_pair() {
        assert!(overlap(npm(">=1.2.0 <2.0.0"), npm("1.5.0")));
        assert!(!overlap(npm(">=1.2.0 <2.0.0"), npm("1.1.0")));
    }

    #[test]
    fn an_npm_hyphen_range_is_inclusive_at_both_ends() {
        assert!(overlap(npm("1.2.3 - 2.3.4"), npm("2.3.4")));
        assert!(!overlap(npm("1.2.3 - 2.3.4"), npm("2.3.5")));
    }

    #[test]
    fn a_partial_bound_covers_its_whole_line() {
        // `>1.2` is `>=1.3.0`, and `<=1.2` is `<1.3.0`.
        assert!(!overlap(npm(">1.2"), npm("1.2.9")));
        assert!(overlap(npm("<=1.2"), npm("1.2.9")));
    }

    #[test]
    fn a_star_matches_everything() {
        assert!(overlap(npm("*"), npm("0.0.1")));
        assert!(overlap(npm("*"), npm("9999.0.0")));
        assert!(range(npm("*")).floor().is_none());
    }

    #[test]
    fn an_npm_specifier_that_is_not_a_range_is_refused_not_guessed() {
        for spec in [
            "workspace:*",
            "file:../local",
            "git+https://example.com/a.git",
            "github:owner/repo",
            "owner/repo",
            "npm:other@^1.0.0",
            "https://example.com/a.tgz",
        ] {
            assert!(
                matches!(npm(spec), Constraint::Unknown(_)),
                "{spec} was not refused"
            );
        }
    }

    #[test]
    fn a_dist_tag_is_unknown_and_not_malformed() {
        // Blaming the manifest for a syntax the tool declined to model
        // would be the wrong refusal.
        assert!(matches!(npm("latest"), Constraint::Unknown(_)));
        assert!(matches!(npm("next"), Constraint::Unknown(_)));
    }

    #[test]
    fn a_broken_range_is_malformed() {
        for spec in ["^^1.0.0", "1.0.0.0.0", ">=", "1.2.beta"] {
            assert!(
                matches!(npm(spec), Constraint::Malformed(_)),
                "{spec} was read as {:?}",
                npm(spec)
            );
        }
        assert!(matches!(cargo(""), Constraint::Malformed(_)));
    }

    #[test]
    fn pep440_models_the_comparison_clauses() {
        assert!(overlap(python(">=3.9"), python("==3.12")));
        assert!(!overlap(python(">=3.9"), python("==3.8")));
        assert!(overlap(python(">=1.0,<2.0"), python("==1.5")));
        assert!(!overlap(python(">=1.0,<2.0"), python("==2.0")));
    }

    #[test]
    fn pep440_equality_is_a_point_not_a_line() {
        // `==1.2` is the single version 1.2, unlike npm's `1.2`.
        assert!(!overlap(python("==1.2"), python("==1.2.9")));
    }

    #[test]
    fn the_pep440_clauses_this_tool_does_not_model_are_refused() {
        for spec in ["~=1.4.2", "!=1.5.0", "===1.0", "==1.2.*", ">=1.0.post1"] {
            assert!(
                matches!(python(spec), Constraint::Unknown(_)),
                "{spec} was read as {:?}",
                python(spec)
            );
        }
    }

    #[test]
    fn a_pep440_prerelease_reads_as_a_prerelease() {
        let parsed = range(python("==2.0.0rc1"));
        assert!(parsed.mentions_prerelease());
        assert!(!range(python("==2.0.0")).mentions_prerelease());
    }

    #[test]
    fn a_go_version_is_a_single_point() {
        assert!(overlap(go("v1.2.3"), go("v1.2.3")));
        assert!(!overlap(go("v1.2.3"), go("v1.2.4")));
        assert!(matches!(go("v1.2.3+incompatible"), Constraint::Range(_)));
        assert!(matches!(go("latest"), Constraint::Malformed(_)));
    }

    #[test]
    fn an_action_major_tag_is_the_whole_major_line() {
        assert!(overlap(action_ref("v4"), action_ref("v4.1.1")));
        assert!(disjoint(&range(action_ref("v4")), &range(action_ref("v5"))));
    }

    #[test]
    fn a_sha_pin_and_a_branch_are_both_refused_for_different_reasons() {
        let sha = action_ref("11bd71901bbe5b1630ceea73d27597364c9af683");
        assert!(matches!(sha, Constraint::Unknown(_)));
        assert!(matches!(action_ref("main"), Constraint::Unknown(_)));
        // A short numeric tag is still a version, not a hash.
        assert!(matches!(action_ref("v4.1.1"), Constraint::Range(_)));
    }

    #[test]
    fn a_ci_channel_name_is_refused_and_a_number_is_a_line() {
        assert!(matches!(ci_tool("stable"), Constraint::Unknown(_)));
        assert!(matches!(ci_tool("lts/*"), Constraint::Unknown(_)));
        assert!(overlap(ci_tool("20"), ci_tool("20.11.0")));
        assert!(disjoint(&range(ci_tool("20")), &range(ci_tool("22"))));
    }

    #[test]
    fn a_minimum_has_no_ceiling() {
        assert!(overlap(minimum("1.88"), minimum("1.99")));
        let floor = range(minimum("1.88"));
        assert_eq!(floor.floor().expect("a floor").to_string(), "1.88.0");
    }

    #[test]
    fn a_prerelease_is_visible_in_the_range() {
        assert!(range(npm("^1.0.0-rc.1")).mentions_prerelease());
        assert!(!range(npm("^1.0.0")).mentions_prerelease());
    }

    #[test]
    fn an_inclusive_and_an_exclusive_bound_meeting_at_a_point_do_not_overlap() {
        // `<2.0.0` and `>=2.0.0` share exactly one candidate and admit
        // none of it.
        assert!(!overlap(npm("<2.0.0"), npm(">=2.0.0")));
        assert!(overlap(npm("<=2.0.0"), npm(">=2.0.0")));
    }
}
