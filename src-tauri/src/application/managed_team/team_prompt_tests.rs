use crate::application::managed_team::render_team_origin_message;
use crate::domain::entities::{TeamMessageActorKind, TeamMessageEnvelopeTarget, TeamMessageKind};
use crate::testing::team_fixtures::team_message;

#[test]
fn team_origin_prompt_is_attributed_and_xml_escaped() {
    let mut message = team_message("message-1", "team-1", 7);
    message.sender_kind = TeamMessageActorKind::Member;
    message.target_kind = TeamMessageEnvelopeTarget::Coordinator;
    message.kind = TeamMessageKind::Result;
    message.content = "<unsafe>&done".to_string();

    let rendered = render_team_origin_message(&message, Some("Worker \"<one>\""));

    assert!(rendered.contains("sender=\"Worker &quot;&lt;one&gt;&quot;\""));
    assert!(rendered.contains("&lt;unsafe&gt;&amp;done"));
    assert!(rendered.starts_with("<team_message "));
    assert!(rendered.ends_with("</team_message>"));
}
