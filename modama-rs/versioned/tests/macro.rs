use versioned::VersionRange;

use versioned::versions;

#[test]
fn test_version_macro_start_only() {
    assert_eq!(
        versions!("1.0.0.."),
        VersionRange::parse("1.0.0..").unwrap()
    )
}

#[test]
fn test_version_macro_end_only() {
    assert_eq!(
        versions!("..1.0.0"),
        VersionRange::parse("..1.0.0").unwrap()
    )
}

#[test]
fn test_version_macro_both() {
    assert_eq!(
        versions!("1.0.0..2.0.0"),
        VersionRange::parse("1.0.0..2.0.0").unwrap()
    )
}

#[test]
fn test_version_macro_exact() {
    assert_eq!(
        versions!("18446744073709551615.0.18446744073709551615"),
        VersionRange::parse("18446744073709551615.0.18446744073709551615").unwrap()
    );
}
