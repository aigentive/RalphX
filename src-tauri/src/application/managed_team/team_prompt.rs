//! Typed prompt rendering for Team-originated messages.

//! Team spool entries are never represented as user text. This small pure
//! boundary gives the normal chat queue an escaped, attributed prompt payload.

use crate::application::agent_workspace_pr_description::escape_xml_text;
use crate::domain::entities::{TeamMessage, TeamMessageActorKind};

pub fn render_team_origin_message(message: &TeamMessage, sender_name: Option<&str>) -> String {
    let sender = match message.sender_kind {
        TeamMessageActorKind::Coordinator => "coordinator",
        TeamMessageActorKind::Member => sender_name.unwrap_or("member"),
        TeamMessageActorKind::System => "system",
    };
    format!(
        "<team_message sequence=\"{}\" sender_kind=\"{}\" sender=\"{}\" kind=\"{}\">{}</team_message>",
        message.sequence,
        actor_kind_name(message.sender_kind),
        escape_xml_attribute(sender),
        message_kind_name(message.kind),
        escape_xml_text(&message.content),
    )
}

fn escape_xml_attribute(value: &str) -> String {
    escape_xml_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn actor_kind_name(kind: TeamMessageActorKind) -> &'static str {
    match kind {
        TeamMessageActorKind::Coordinator => "coordinator",
        TeamMessageActorKind::Member => "member",
        TeamMessageActorKind::System => "system",
    }
}

fn message_kind_name(kind: crate::domain::entities::TeamMessageKind) -> &'static str {
    match kind {
        crate::domain::entities::TeamMessageKind::Instruction => "instruction",
        crate::domain::entities::TeamMessageKind::Result => "result",
        crate::domain::entities::TeamMessageKind::Question => "question",
        crate::domain::entities::TeamMessageKind::Status => "status",
        crate::domain::entities::TeamMessageKind::Control => "control",
        crate::domain::entities::TeamMessageKind::Approval => "approval",
    }
}
