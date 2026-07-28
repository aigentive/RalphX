//! HTTP handlers for the managed-Team lifecycle surface (`/api/managed_team/*`).

pub mod lifecycle;
pub mod members;

pub use lifecycle::{ensure_managed_team, get_managed_team_roster, get_managed_team_status};
pub use members::{
    add_managed_team_member, assign_managed_team_member, list_idle_managed_team_members,
    stop_managed_team_member,
};
