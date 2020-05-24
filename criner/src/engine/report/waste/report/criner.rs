use crate::model::TaskResult;
use std::path::{Path, PathBuf};

pub const TOP_LEVEL_REPORT_NAME: &str = "__top-level-report__";

pub fn path_from_prefix(out_dir: &Path, prefix: &str) -> PathBuf {
    use crate::engine::report::generic::Generator;
    out_dir.join(format!(
        "{}-{}-{}.rmp",
        prefix,
        super::super::Generator::name(),
        super::super::Generator::version()
    ))
}

impl super::Report {
    pub fn path_to_storage_location(&self, out_dir: &Path) -> PathBuf {
        use super::Report::*;
        let prefix = match self {
            Version { crate_name, .. } | Crate { crate_name, .. } => crate_name.as_str(),
            CrateCollection { .. } => TOP_LEVEL_REPORT_NAME,
        };
        path_from_prefix(out_dir, prefix)
    }

    pub fn from_result(crate_name: &str, crate_version: &str, result: TaskResult) -> super::Report {
        match result {
            TaskResult::ExplodedCrate {
                entries_meta_data,
                selected_entries,
            } => Self::from_package(
                crate_name,
                crate_version,
                super::TarPackage {
                    entries_meta_data,
                    entries: selected_entries,
                },
            ),
            _ => unreachable!("caller must assure we are always an exploded entry"),
        }
    }
}
