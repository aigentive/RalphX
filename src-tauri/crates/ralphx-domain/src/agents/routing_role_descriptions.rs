use super::routing_role::RoutingRole;

pub(super) const fn routing_role_description(role: RoutingRole) -> &'static str {
    match role {
        RoutingRole::WorkspaceChat => "General project conversation and workspace assistance.",
        RoutingRole::WorkspaceEdit => "Implements requested changes in the project workspace.",
        RoutingRole::WorkspacePlan => "Develops implementation plans for project changes.",
        RoutingRole::WorkspaceIdeation => {
            "Explores product and technical ideas into actionable plans."
        }
        RoutingRole::WorkspaceReviewPr => {
            "Reviews pull request changes and reports actionable findings."
        }
        RoutingRole::WorkspaceAutomation => "Runs configured project automation conversations.",
        RoutingRole::AutomationPlanJudge => {
            "Evaluates whether an automation plan is ready to execute."
        }
        RoutingRole::AutomationResultJudge => {
            "Evaluates automation results and required follow-up."
        }
        RoutingRole::WorkspaceReviewer => "Reviews workspace changes and identifies issues.",
        RoutingRole::WorkspaceRepair => "Repairs workspace setup, branch, or execution problems.",
        RoutingRole::WorkspaceMergeRepair => {
            "Resolves merge conflicts and incomplete merge states."
        }
        RoutingRole::WorkspacePrFixer => "Addresses pull request feedback and failing checks.",
        RoutingRole::IdeationPrimary => "Leads ideation and produces the working plan.",
        RoutingRole::IdeationVerifier => "Challenges an ideation plan before implementation.",
        RoutingRole::IdeationSubagent => "Explores a focused question for the ideation lead.",
        RoutingRole::IdeationVerifierSubagent => "Investigates a focused verification concern.",
        RoutingRole::DelegatedSubagent => "Handles a bounded task delegated by another agent.",
        RoutingRole::ExecutionWorker => {
            "Implements an execution-plan task in its isolated workspace."
        }
        RoutingRole::ExecutionQaPrep => "Prepares changed code for quality validation.",
        RoutingRole::ExecutionQaRefiner => "Refines changes in response to quality findings.",
        RoutingRole::ExecutionQaTester => "Runs targeted tests and reports behavioral evidence.",
        RoutingRole::ExecutionReviewer => {
            "Reviews completed execution work for correctness and scope."
        }
        RoutingRole::ExecutionReexecutor => "Implements follow-up changes requested by review.",
        RoutingRole::ExecutionMerger => "Completes approved branch integration and merge cleanup.",
        RoutingRole::UtilityLightweight => {
            "Handles small utility tasks with minimal runtime overhead."
        }
        RoutingRole::UtilityPrDescriber => {
            "Summarizes a completed change for pull request publication."
        }
        RoutingRole::UtilityProjectAnalyzer => {
            "Inspects a project and reports relevant implementation context."
        }
        RoutingRole::MemoryCapture => "Extracts durable project knowledge from completed work.",
        RoutingRole::MemoryMaintainer => "Curates and updates stored project knowledge.",
    }
}
