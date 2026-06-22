use std::path::{Path, PathBuf};

pub const MARKER_DIR: &str = "/data/zellij-new-tab-next-to-current";

pub fn sanitize_session_name(session_name: &str) -> String {
    if session_name.is_empty() {
        return "default".to_string();
    }

    session_name
        .chars()
        .map(|character| {
            if character == '/' || character == '\\' || character.is_control() {
                '_'
            } else {
                character
            }
        })
        .collect()
}

pub fn marker_path(marker_name: &str, session_name: &str) -> PathBuf {
    Path::new(MARKER_DIR).join(format!(
        "{}-{}",
        marker_name,
        sanitize_session_name(session_name)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_files_live_in_plugin_data_directory() {
        assert_eq!(
            marker_path("heartbeat", "development"),
            PathBuf::from("/data/zellij-new-tab-next-to-current/heartbeat-development")
        );
        assert_eq!(
            marker_path("lock", "development"),
            PathBuf::from("/data/zellij-new-tab-next-to-current/lock-development")
        );
    }

    #[test]
    fn session_name_cannot_add_path_components() {
        assert_eq!(sanitize_session_name("team/dev\\one\n"), "team_dev_one_");
        assert_eq!(
            marker_path("lock", "../other/session"),
            PathBuf::from("/data/zellij-new-tab-next-to-current/lock-.._other_session")
        );
    }

    #[test]
    fn empty_session_name_uses_default_suffix() {
        assert_eq!(sanitize_session_name(""), "default");
    }
}
