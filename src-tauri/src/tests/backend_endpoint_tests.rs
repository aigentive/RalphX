use crate::utils::backend_endpoint::{
    parse_backend_port, DEVELOPMENT_BACKEND_PORT, PRODUCTION_BACKEND_PORT,
};

#[test]
fn backend_port_parser_uses_default_when_override_is_missing() {
    assert_eq!(
        parse_backend_port(None, DEVELOPMENT_BACKEND_PORT),
        Ok(DEVELOPMENT_BACKEND_PORT)
    );
}

#[test]
fn backend_port_parser_accepts_valid_override() {
    assert_eq!(
        parse_backend_port(Some("3857"), PRODUCTION_BACKEND_PORT),
        Ok(3857)
    );
}

#[test]
fn backend_port_parser_rejects_zero_empty_and_non_numeric_values() {
    assert!(parse_backend_port(Some("0"), PRODUCTION_BACKEND_PORT).is_err());
    assert!(parse_backend_port(Some(" "), PRODUCTION_BACKEND_PORT).is_err());
    assert!(parse_backend_port(Some("not-a-port"), PRODUCTION_BACKEND_PORT).is_err());
}
