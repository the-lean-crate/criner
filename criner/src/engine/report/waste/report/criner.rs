use crate::model::TaskResult;

impl super::Report {
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
