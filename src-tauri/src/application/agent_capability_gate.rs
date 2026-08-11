use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub team: bool,
    pub workflows: bool,
    pub autopilot: bool,
}

#[derive(Debug, Default)]
pub struct AgentCapabilityGate {
    team: AtomicBool,
    workflows: AtomicBool,
    autopilot: AtomicBool,
}

impl AgentCapabilityGate {
    pub fn snapshot(&self) -> AgentCapabilities {
        AgentCapabilities {
            team: self.team.load(Ordering::Acquire),
            workflows: self.workflows.load(Ordering::Acquire),
            autopilot: self.autopilot.load(Ordering::Acquire),
        }
    }

    pub fn replace(&self, capabilities: AgentCapabilities) {
        self.team.store(capabilities.team, Ordering::Release);
        self.workflows
            .store(capabilities.workflows, Ordering::Release);
        self.autopilot
            .store(capabilities.autopilot, Ordering::Release);
    }

    pub fn team_enabled(&self) -> bool {
        self.team.load(Ordering::Acquire)
    }

    pub fn workflows_enabled(&self) -> bool {
        self.workflows.load(Ordering::Acquire)
    }

    pub fn autopilot_enabled(&self) -> bool {
        self.autopilot.load(Ordering::Acquire)
    }
}
