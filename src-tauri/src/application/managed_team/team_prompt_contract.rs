const RX_NATIVE_TEAM_CONTRACT: &str = r#"<rx_native_team_contract>
RX-native Team mode:
- Use delegate_start, delegate_wait, and delegate_cancel only with canonical allowed RalphX-native delegate targets exposed to you.
- Choose delegates only when they add useful independent work.
- You choose delegate targets; the backend owns lifecycle and settlement.
- Wait for required delegated results before depending on them.
- If you stay solo, briefly explain why delegation would not add useful independent work.
- Never replay job ids, timestamps, wait knobs, or backend bookkeeping.
</rx_native_team_contract>"#;

pub fn rx_native_team_contract() -> &'static str {
    RX_NATIVE_TEAM_CONTRACT
}

pub fn apply_rx_native_team_contract(prompt: String) -> String {
    format!("{prompt}\n\n{}", rx_native_team_contract())
}
