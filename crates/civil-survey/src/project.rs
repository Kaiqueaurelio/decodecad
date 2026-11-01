//! Versioned, bounded JSON persistence for civil survey projects.

use cadcraft_civil_core::ProjectSettings;
use serde::{Deserialize, Serialize};

use crate::SurveyDataset;

pub const CIVIL_PROJECT_FORMAT_VERSION: u32 = 1;
pub const MAX_PROJECT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CivilProject {
    pub format_version: u32,
    pub name: String,
    pub settings: ProjectSettings,
    pub surveys: Vec<SurveyDataset>,
}

impl CivilProject {
    pub fn new(name: impl Into<String>) -> Self {
        Self { format_version: CIVIL_PROJECT_FORMAT_VERSION, name: name.into(), settings: ProjectSettings::default(), surveys: Vec::new() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectFileError {
    #[error("project file exceeds the supported size limit")]
    TooLarge,
    #[error("unsupported civil project format version {0}")]
    UnsupportedVersion(u32),
    #[error("invalid civil project file: {0}")]
    Invalid(String),
}

pub fn save_project(project: &CivilProject) -> Result<Vec<u8>, ProjectFileError> {
    if project.format_version != CIVIL_PROJECT_FORMAT_VERSION {
        return Err(ProjectFileError::UnsupportedVersion(project.format_version));
    }
    let bytes = serde_json::to_vec_pretty(project).map_err(|error| ProjectFileError::Invalid(error.to_string()))?;
    if bytes.len() > MAX_PROJECT_BYTES {
        return Err(ProjectFileError::TooLarge);
    }
    Ok(bytes)
}

pub fn load_project(bytes: &[u8]) -> Result<CivilProject, ProjectFileError> {
    if bytes.len() > MAX_PROJECT_BYTES {
        return Err(ProjectFileError::TooLarge);
    }
    let project: CivilProject = serde_json::from_slice(bytes).map_err(|error| ProjectFileError::Invalid(error.to_string()))?;
    if project.format_version != CIVIL_PROJECT_FORMAT_VERSION {
        return Err(ProjectFileError::UnsupportedVersion(project.format_version));
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_project_and_points() {
        let mut project = CivilProject::new("Obra teste");
        project.surveys.push(SurveyDataset { name: "levantamento".into(), points: Vec::new() });
        let encoded = save_project(&project);
        assert!(encoded.is_ok());
        if let Ok(bytes) = encoded {
            let decoded = load_project(&bytes);
            assert!(decoded.is_ok());
            if let Ok(decoded) = decoded {
                assert_eq!(decoded, project);
            }
        }
    }

    #[test]
    fn rejects_unknown_format_version() {
        let project = CivilProject { format_version: 99, ..CivilProject::new("test") };
        assert!(matches!(save_project(&project), Err(ProjectFileError::UnsupportedVersion(99))));
    }
}