use super::team_prompt_contract::{apply_rx_native_team_contract, rx_native_team_contract};

#[test]
fn rx_native_team_contract_defines_backend_owned_delegation_boundaries() {
    let contract = rx_native_team_contract();

    assert!(contract.contains("canonical allowed RalphX-native delegate targets"));
    assert!(contract.contains("delegate_start, delegate_wait, and delegate_cancel"));
    assert!(contract.contains("useful independent work"));
    assert!(contract.contains("backend owns lifecycle and settlement"));
    assert!(contract.contains("Wait for required delegated results"));
    assert!(contract.contains("If you stay solo, briefly explain why"));
    assert!(
        contract.contains("Never replay job ids, timestamps, wait knobs, or backend bookkeeping")
    );
}

#[test]
fn apply_rx_native_team_contract_appends_contract_after_existing_prompt() {
    let prompt = apply_rx_native_team_contract("Existing trusted prompt.".to_string());

    assert!(prompt.starts_with("Existing trusted prompt.\n\n"));
    assert!(prompt.ends_with(rx_native_team_contract()));
}
