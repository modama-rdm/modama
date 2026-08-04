//! Version range parsing and matching.
//!
//! Supports Rust-style range syntax:
//! - `1.0.0..2.0.0` - versions from 1.0.0 (inclusive) to 2.0.0 (exclusive)
//! - `1.0.0..` - versions from 1.0.0 onwards
//! - `..2.0.0` - versions before 2.0.0
//! - `..` - all versions
//! - `1.0.0` - exactly version 1.0.0
//!
//! API versions are plain `X.Y.Z` versions. Prerelease and build metadata are
//! intentionally unsupported.

use semver::Version;
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// An exact version or half-open version range with Rust-style syntax.
#[derive(Clone, PartialEq, Eq)]
pub struct VersionRange {
    kind: VersionRangeKind,
}

#[derive(Clone, PartialEq, Eq)]
enum VersionRangeKind {
    Exact(Version),
    Range {
        start: Option<Version>,
        end: Option<Version>,
    },
}

#[doc(hidden)]
pub type VersionTriplet = (u64, u64, u64);

/// Error returned when bounded range endpoints do not form a non-empty range.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRangeBoundsError {
    Empty(u64, u64, u64),
    Reversed {
        start: VersionTriplet,
        end: VersionTriplet,
    },
}

impl fmt::Display for VersionRangeBoundsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(major, minor, patch) => write!(
                f,
                "range start must be less than range end; both bounds are {major}.{minor}.{patch}"
            ),
            Self::Reversed { start, end } => write!(
                f,
                "range start must be less than range end; start is {}.{}.{} and end is {}.{}.{}",
                start.0, start.1, start.2, end.0, end.1, end.2
            ),
        }
    }
}

impl std::error::Error for VersionRangeBoundsError {}

/// Validate optional numeric bounds shared by runtime and macro parsing.
#[doc(hidden)]
pub fn validate_numeric_bounds(
    start: Option<VersionTriplet>,
    end: Option<VersionTriplet>,
) -> Result<(), VersionRangeBoundsError> {
    match (start, end) {
        (Some(start), Some(end)) => match start.cmp(&end) {
            Ordering::Less => Ok(()),
            Ordering::Equal => Err(VersionRangeBoundsError::Empty(start.0, start.1, start.2)),
            Ordering::Greater => Err(VersionRangeBoundsError::Reversed { start, end }),
        },
        _ => Ok(()),
    }
}

impl fmt::Debug for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            VersionRangeKind::Exact(version) => f
                .debug_tuple("VersionRange::Exact")
                .field(&version.to_string())
                .finish(),
            VersionRangeKind::Range { start, end } => f
                .debug_struct("VersionRange::Range")
                .field("start", &start.as_ref().map(ToString::to_string))
                .field("end", &end.as_ref().map(ToString::to_string))
                .finish(),
        }
    }
}

impl VersionRange {
    /// Construct an exact plain version for use by the `versions!` macro.
    #[doc(hidden)]
    #[must_use]
    pub fn __exact(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            kind: VersionRangeKind::Exact(Version::new(major, minor, patch)),
        }
    }

    /// Construct plain range bounds for use by the `versions!` macro.
    #[doc(hidden)]
    pub fn __range(
        start: Option<VersionTriplet>,
        end: Option<VersionTriplet>,
    ) -> Result<Self, VersionRangeBoundsError> {
        validate_numeric_bounds(start, end)?;
        Ok(Self {
            kind: VersionRangeKind::Range {
                start: start.map(|(major, minor, patch)| Version::new(major, minor, patch)),
                end: end.map(|(major, minor, patch)| Version::new(major, minor, patch)),
            },
        })
    }

    /// Inclusive lower bound, if this is a range.
    #[must_use]
    pub fn start(&self) -> Option<&Version> {
        match &self.kind {
            VersionRangeKind::Exact(_) => None,
            VersionRangeKind::Range { start, .. } => start.as_ref(),
        }
    }

    /// Exclusive upper bound, if this is a range.
    #[must_use]
    pub fn end(&self) -> Option<&Version> {
        match &self.kind {
            VersionRangeKind::Exact(_) => None,
            VersionRangeKind::Range { end, .. } => end.as_ref(),
        }
    }

    /// The exact version, if this is an exact version.
    #[must_use]
    pub fn exact_version(&self) -> Option<&Version> {
        match &self.kind {
            VersionRangeKind::Exact(version) => Some(version),
            VersionRangeKind::Range { .. } => None,
        }
    }

    /// Check if a version is within this range.
    pub fn contains(&self, version: &Version) -> bool {
        if !version.pre.is_empty() || !version.build.is_empty() {
            return false;
        }
        match &self.kind {
            VersionRangeKind::Exact(exact) => version == exact,
            VersionRangeKind::Range { start, end } => {
                start.as_ref().is_none_or(|start| version >= start)
                    && end.as_ref().is_none_or(|end| version < end)
            }
        }
    }

    /// Check whether this range and another range contain any common version.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        match (&self.kind, &other.kind) {
            (VersionRangeKind::Exact(version), _) => other.contains(version),
            (_, VersionRangeKind::Exact(version)) => self.contains(version),
            (
                VersionRangeKind::Range {
                    start: first_start,
                    end: first_end,
                },
                VersionRangeKind::Range {
                    start: second_start,
                    end: second_end,
                },
            ) => {
                first_end
                    .as_ref()
                    .is_none_or(|end| second_start.as_ref().is_none_or(|start| start < end))
                    && second_end
                        .as_ref()
                        .is_none_or(|end| first_start.as_ref().is_none_or(|start| start < end))
            }
        }
    }

    /// Parse a plain `X.Y.Z` API version.
    pub fn parse_version(s: &str) -> Result<Version, VersionRangeParseError> {
        let version = Version::parse(s).map_err(|e| VersionRangeParseError {
            version: s.to_string(),
            message: e.to_string(),
        })?;
        if !version.pre.is_empty() || !version.build.is_empty() {
            return Err(VersionRangeParseError {
                version: s.to_owned(),
                message: "expected a plain X.Y.Z version without prerelease or build metadata"
                    .to_owned(),
            });
        }
        Ok(version)
    }

    /// Parse special cases like ".." (all versions).
    /// Returns None if not a special case.
    fn parse_special_cases(s: &str) -> Option<Self> {
        if s == ".." {
            Some(Self {
                kind: VersionRangeKind::Range {
                    start: None,
                    end: None,
                },
            })
        } else {
            None
        }
    }

    /// Parse a bounded range with ".." separator (e.g., "1.0.0..2.0.0", "1.0.0..", "..2.0.0").
    fn parse_bounded_range(s: &str) -> Result<Self, VersionRangeParseError> {
        // Safety: This function is only called when s.contains("..") is true (see parse())
        let idx = s
            .find("..")
            .unwrap_or_else(|| unreachable!("bounded range must contain '..'"));
        let start_str = &s[..idx];
        let end_str = &s[idx + 2..];

        let start = if start_str.is_empty() {
            None
        } else {
            Some(Self::parse_version(start_str)?)
        };

        let end = if end_str.is_empty() {
            None
        } else {
            Some(Self::parse_version(end_str)?)
        };

        let start_triplet = start.as_ref().map(version_triplet);
        let end_triplet = end.as_ref().map(version_triplet);
        validate_numeric_bounds(start_triplet, end_triplet).map_err(|error| {
            VersionRangeParseError {
                version: s.to_owned(),
                message: error.to_string(),
            }
        })?;

        Ok(Self {
            kind: VersionRangeKind::Range { start, end },
        })
    }

    /// Parse an exact version.
    fn parse_exact_version(s: &str) -> Result<Self, VersionRangeParseError> {
        let version = Self::parse_version(s)?;
        Ok(Self {
            kind: VersionRangeKind::Exact(version),
        })
    }

    /// Parse a version range from string.
    ///
    /// Supports:
    ///
    /// - `1.0.0..2.0.0` - bounded range
    /// - `1.0.0..` - from version
    /// - `..2.0.0` - up to version
    /// - `..` - all versions
    /// - `1.0.0` - exact version
    ///
    /// # Errors
    /// Returns `VersionRangeParseError` if the version string cannot be parsed.
    pub fn parse(s: &str) -> Result<Self, VersionRangeParseError> {
        let s = s.trim();

        // Check for special cases first
        if let Some(range) = Self::parse_special_cases(s) {
            return Ok(range);
        }

        // Check if it contains ".." (bounded range)
        if s.contains("..") {
            Self::parse_bounded_range(s)
        } else {
            // Exact version
            Self::parse_exact_version(s)
        }
    }
}

fn version_triplet(version: &Version) -> VersionTriplet {
    (version.major, version.minor, version.patch)
}

impl FromStr for VersionRange {
    type Err = VersionRangeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            VersionRangeKind::Exact(version) => write!(f, "{version}"),
            VersionRangeKind::Range { start, end } => match (start, end) {
                (Some(start), Some(end)) => write!(f, "{start}..{end}"),
                (Some(start), None) => write!(f, "{start}.."),
                (None, Some(end)) => write!(f, "..{end}"),
                (None, None) => write!(f, ".."),
            },
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid version '{version}': {message}")]
pub struct VersionRangeParseError {
    pub version: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_forms() {
        let range = VersionRange::parse("1.0.0..2.0.0").unwrap();
        assert_eq!(range.start(), Some(&Version::new(1, 0, 0)));
        assert_eq!(range.end(), Some(&Version::new(2, 0, 0)));
        assert_eq!(
            VersionRange::parse("1.0.0").unwrap().exact_version(),
            Some(&Version::new(1, 0, 0))
        );
        assert_eq!(VersionRange::parse("1.0.0..").unwrap().end(), None);
        assert_eq!(VersionRange::parse("..2.0.0").unwrap().start(), None);
        assert_eq!(VersionRange::parse("..").unwrap().to_string(), "..");
    }

    #[test]
    fn bounded_ranges_are_half_open() {
        let range = VersionRange::parse("1.0.0..2.0.0").unwrap();
        assert!(range.contains(&Version::new(1, 0, 0)));
        assert!(range.contains(&Version::new(1, 5, 0)));
        assert!(!range.contains(&Version::new(2, 0, 0)));
        assert!(!range.contains(&Version::new(0, 9, 9)));
    }

    #[test]
    fn exact_versions_are_exact_and_do_not_overflow() {
        let range = VersionRange::parse("1.0.0").unwrap();
        assert!(range.contains(&Version::new(1, 0, 0)));
        assert!(!range.contains(&Version::new(1, 0, 1)));

        let maximum = VersionRange::parse("1.0.18446744073709551615").unwrap();
        assert!(maximum.contains(&Version::new(1, 0, u64::MAX)));
    }

    #[test]
    fn modifiers_are_rejected_everywhere() {
        for input in [
            "1.0.0-alpha",
            "1.0.0+build",
            "1.0.0-alpha..2.0.0",
            "1.0.0..2.0.0+build",
        ] {
            assert!(VersionRange::parse(input).is_err(), "{input}");
        }

        let range = VersionRange::parse("1.0.0..2.0.0").unwrap();
        assert!(!range.contains(&Version::parse("1.5.0-alpha").unwrap()));
        assert!(!range.contains(&Version::parse("1.5.0+build").unwrap()));
    }

    #[test]
    fn exact_and_range_overlap_correctly() {
        let exact = VersionRange::parse("1.5.0").unwrap();
        assert!(exact.overlaps(&VersionRange::parse("1.0.0..2.0.0").unwrap()));
        assert!(!exact.overlaps(&VersionRange::parse("1.5.1..").unwrap()));
        assert!(!exact.overlaps(&VersionRange::parse("1.5.1").unwrap()));
    }

    #[test]
    fn rejects_empty_and_reversed_bounds() {
        for (input, detail) in [
            ("1.0.0..1.0.0", "both bounds are 1.0.0"),
            ("2.0.0..1.0.0", "start is 2.0.0 and end is 1.0.0"),
            (
                "18446744073709551615.0.0..0.0.0",
                "start is 18446744073709551615.0.0 and end is 0.0.0",
            ),
        ] {
            let error = VersionRange::parse(input).unwrap_err();
            assert_eq!(error.version, input);
            assert!(error.message.contains(detail), "{error}");
        }

        assert!(VersionRange::__range(Some((1, 0, 0)), Some((1, 0, 0))).is_err());
        assert!(VersionRange::__range(Some((2, 0, 0)), Some((1, 0, 0))).is_err());
    }

    #[test]
    fn unbounded_forms_remain_valid() {
        for input in ["..", "1.0.0..", "..1.0.0"] {
            VersionRange::parse(input).unwrap();
        }
    }

    #[test]
    fn generated_boundaries_follow_half_open_properties() {
        let values = [0, 1, 2, u64::MAX];
        for &start in &values {
            for &end in &values {
                let parsed = VersionRange::__range(Some((0, 0, start)), Some((0, 0, end)));
                assert_eq!(parsed.is_ok(), start < end);

                if let Ok(range) = parsed {
                    for &candidate in &values {
                        assert_eq!(
                            range.contains(&Version::new(0, 0, candidate)),
                            start <= candidate && candidate < end
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn overlap_is_symmetric_and_touching_ranges_are_disjoint() {
        let ranges = [
            VersionRange::parse("0.0.0").unwrap(),
            VersionRange::parse("0.0.0..1.0.0").unwrap(),
            VersionRange::parse("1.0.0").unwrap(),
            VersionRange::parse("1.0.0..2.0.0").unwrap(),
            VersionRange::parse("2.0.0..").unwrap(),
            VersionRange::parse("..").unwrap(),
        ];
        for first in &ranges {
            for second in &ranges {
                assert_eq!(first.overlaps(second), second.overlaps(first));
            }
        }
        assert!(!ranges[1].overlaps(&ranges[3]));
    }

    #[test]
    fn display_and_debug_preserve_the_form() {
        let exact = VersionRange::parse("1.0.0").unwrap();
        assert_eq!(exact.to_string(), "1.0.0");
        assert_eq!(format!("{exact:?}"), r#"VersionRange::Exact("1.0.0")"#);
        let range = VersionRange::parse("1.0.0..2.0.0").unwrap();
        assert_eq!(
            format!("{range:?}"),
            r#"VersionRange::Range { start: Some("1.0.0"), end: Some("2.0.0") }"#
        );
    }
}
