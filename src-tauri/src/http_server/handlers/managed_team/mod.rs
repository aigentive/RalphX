//! HTTP handlers for the managed-Team lifecycle surface (`/api/managed_team/*`).

pub mod lifecycle;
pub mod members;
pub mod messaging;

pub use lifecycle::{ensure_managed_team, get_managed_team_roster, get_managed_team_status};
pub use members::{
    add_managed_team_member, assign_managed_team_member, exit_managed_team,
    list_idle_managed_team_members, stop_managed_team_member,
};
pub use messaging::{get_managed_team_member_roster, send_managed_team_message};
