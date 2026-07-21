#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiFeatureFlagOverrides {
    pub agent_personas: Option<bool>,
    pub agent_conversation_team: bool,
    pub agent_conversation_workflows: bool,
    pub agent_conversation_autopilot: bool,
}
