const RX_NATIVE_TEAM_CONTRACT: &str = r#"<rx_native_team_contract>
RX-native Team mode:
- Use team_add_member to create a durable lazy member before assigning work.
- Use team_assign only for independent caller-led tasks; choose each member by normalized name and declare write reservation surfaces for write work.
- Use team_list to find idle members and team_stop_member to stop a member by name.
- The backend owns member generations, reservations, run bindings, launch, settlement, and recovery. Never pass or replay ids, generations, sessions, runs, timestamps, or lifecycle bookkeeping.
- Wait for required member results before depending on them. If you stay solo, briefly explain why delegation would not add useful independent work.
</rx_native_team_contract>"#;

pub fn rx_native_team_contract() -> &'static str {
    RX_NATIVE_TEAM_CONTRACT
}

pub fn apply_rx_native_team_contract(prompt: String) -> String {
    format!("{prompt}\n\n{}", rx_native_team_contract())
}
