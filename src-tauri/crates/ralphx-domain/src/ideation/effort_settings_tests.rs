use std::str::FromStr;

use crate::ideation::effort_settings::EffortLevel;

#[test]
fn test_effort_level_round_trip() {
    let values = ["low", "medium", "high", "max", "ultra", "inherit"];
    for v in values {
        let parsed = EffortLevel::from_str(v).expect("parse");
        assert_eq!(parsed.to_string(), v, "round-trip for '{}'", v);
    }
}

#[test]
fn test_effort_level_invalid() {
    assert!(EffortLevel::from_str("").is_err());
    assert!(EffortLevel::from_str("INHERIT").is_err());
}

#[test]
fn test_effort_level_default_is_inherit() {
    assert_eq!(EffortLevel::default(), EffortLevel::Inherit);
}

#[test]
fn test_effort_level_serde() {
    let json = serde_json::to_string(&EffortLevel::High).unwrap();
    assert_eq!(json, "\"high\"");
    let de: EffortLevel = serde_json::from_str("\"ultra\"").unwrap();
    assert_eq!(de, EffortLevel::Ultra);
}
