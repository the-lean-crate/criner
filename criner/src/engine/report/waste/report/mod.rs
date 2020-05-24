use crate::model::TaskResult;
pub use criner_waste_report::*;
use std::path::{Path, PathBuf};

mod merge;

pub const TOP_LEVEL_REPORT_NAME: &str = "__top-level-report__";

pub fn path_from_prefix(out_dir: &Path, prefix: &str) -> PathBuf {
    use crate::engine::report::generic::Generator;
    out_dir.join(format!(
        "{}-{}-{}.rmp",
        prefix,
        super::Generator::name(),
        super::Generator::version()
    ))
}

pub fn path_to_storage_location(report: &Report, out_dir: &Path) -> PathBuf {
    use Report::*;
    let prefix = match report {
        Version { crate_name, .. } | Crate { crate_name, .. } => crate_name.as_str(),
        CrateCollection { .. } => TOP_LEVEL_REPORT_NAME,
    };
    path_from_prefix(out_dir, prefix)
}

pub fn from_result(crate_name: &str, crate_version: &str, result: TaskResult) -> Report {
    match result {
        TaskResult::ExplodedCrate {
            entries_meta_data,
            selected_entries,
        } => Report::from_package(
            crate_name,
            crate_version,
            TarPackage {
                entries_meta_data,
                entries: selected_entries,
            },
        ),
        _ => unreachable!("caller must assure we are always an exploded entry"),
    }
}
