#[derive(Debug, Clone, Copy)]
pub(crate) enum MetricsScope<'a> {
    Project(&'a str),
    AllProjects,
}

impl<'a> MetricsScope<'a> {
    pub(crate) fn from_optional_project_id(project_id: Option<&'a str>) -> Self {
        match project_id.map(str::trim).filter(|value| !value.is_empty()) {
            Some(project_id) => Self::Project(project_id),
            None => Self::AllProjects,
        }
    }

    pub(crate) fn cache_key(self, week_start_day: u8, tz_offset_minutes: i32) -> String {
        match self {
            Self::Project(project_id) => {
                format!("project:{project_id}:{week_start_day}:{tz_offset_minutes}")
            }
            Self::AllProjects => format!("all:{week_start_day}:{tz_offset_minutes}"),
        }
    }

    pub(crate) fn project_filter(self, alias: &str) -> String {
        match self {
            Self::Project(_) => format!("{alias}.project_id = ?1"),
            Self::AllProjects => "1 = 1".to_string(),
        }
    }

    pub(crate) fn project_params(self) -> Vec<&'a str> {
        match self {
            Self::Project(project_id) => vec![project_id],
            Self::AllProjects => Vec::new(),
        }
    }
}
