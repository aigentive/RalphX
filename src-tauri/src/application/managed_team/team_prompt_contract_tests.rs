use super::team_prompt_contract::{apply_rx_native_team_contract, rx_native_team_contract};

#[test]
fn rx_native_team_contract_defines_backend_owned_delegation_boundaries() {
    let contract = rx_native_team_contract();

    assert!(contract.contains("team_add_member"));
    assert!(contract.contains("team_assign"));
    assert!(contract.contains("team_list"));
    assert!(contract.contains("team_stop_member"));
    assert!(contract.contains("normalized name"));
    assert!(contract.contains("Never pass or replay ids"));
    assert!(contract.contains("useful independent work"));
    assert!(contract.contains("backend owns member generations, reservations, run bindings"));
    assert!(contract.contains("Wait for required member results"));
    assert!(contract.contains("If you stay solo, briefly explain why"));
}

#[test]
fn apply_rx_native_team_contract_appends_contract_after_existing_prompt() {
    let prompt = apply_rx_native_team_contract("Existing trusted prompt.".to_string());

    assert!(prompt.starts_with("Existing trusted prompt.\n\n"));
    assert!(prompt.ends_with(rx_native_team_contract()));
}
