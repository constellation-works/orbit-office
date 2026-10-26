//! Sensitivity ordering: private is stricter, and tokens round-trip.

use crate::Sensitivity;

#[test]
fn strictest_wins_and_tokens_round_trip() {
    assert_eq!(
        Sensitivity::Normal.strictest(Sensitivity::Private),
        Sensitivity::Private
    );
    assert_eq!(
        Sensitivity::Private.strictest(Sensitivity::Normal),
        Sensitivity::Private
    );
    for value in [Sensitivity::Normal, Sensitivity::Private] {
        assert_eq!(Sensitivity::parse(value.as_str()), Some(value));
    }
    assert_eq!(Sensitivity::parse("secret"), None);
}
