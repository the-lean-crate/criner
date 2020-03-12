mod merge;
mod result;

use crate::{
    model::{TarHeader, TaskResult},
    Result,
};
use async_trait::async_trait;
use serde_derive::Deserialize;
use std::{collections::BTreeMap, path::Path};

pub type Patterns = Vec<String>;

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Fix {
    EnrichedInclude {
        include: Patterns,
        include_added: Patterns,
        include_removed: Patterns,
        has_build_script: bool,
    },
    EnrichedExclude {
        exclude: Patterns,
        exclude_added: Patterns,
        has_build_script: bool,
    },
    NewInclude {
        include: Patterns,
        has_build_script: bool,
    },
    RemoveExcludeAndUseInclude {
        include_added: Patterns,
        include: Patterns,
        include_removed: Patterns,
    },
    RemoveExclude,
}

#[derive(Default, Deserialize)]
pub struct Package {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    build: Option<String>,
}

pub type WastedFile = (String, u64);

#[derive(Default, Debug, PartialEq, Clone)]
pub struct AggregateFileInfo {
    pub total_bytes: u64,
    pub total_files: u64,
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct VersionInfo {
    pub all: AggregateFileInfo,
    pub waste: AggregateFileInfo,
}

pub type AggregateVersionInfo = VersionInfo;

pub type Dict<T> = BTreeMap<String, T>;

#[derive(Debug, PartialEq, Clone)]
pub enum Report {
    Version {
        crate_name: String,
        crate_version: String,
        total_size_in_bytes: u64,
        total_files: u64,
        wasted_files: Vec<WastedFile>,
        suggested_fix: Option<Fix>,
    },
    Crate {
        crate_name: String,
        total_size_in_bytes: u64,
        total_files: u64,
        info_by_version: Dict<VersionInfo>,
        wasted_by_extension: Dict<AggregateFileInfo>,
    },
    CrateCollection {
        total_size_in_bytes: u64,
        total_files: u64,
        info_by_crate: Dict<AggregateVersionInfo>,
        wasted_by_extension: Dict<AggregateFileInfo>,
    },
}

#[async_trait]
impl crate::engine::report::generic::Aggregate for Report {
    fn merge(self, other: Self) -> Self {
        use Report::*;
        match (self, other) {
            (lhs @ Version { .. }, rhs @ Version { .. }) => {
                merge::crate_from_version(lhs).merge(rhs)
            }
            (version @ Version { .. }, krate @ Crate { .. }) => krate.merge(version),
            (version @ Version { .. }, collection @ CrateCollection { .. }) => {
                collection.merge(version)
            }
            (collection @ CrateCollection { .. }, version @ Version { .. }) => {
                collection.merge(merge::crate_from_version(version))
            }
            (krate @ Crate { .. }, collection @ CrateCollection { .. }) => collection.merge(krate),
            (
                Crate {
                    crate_name: lhs_crate_name,
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_version,
                    wasted_by_extension,
                },
                Version {
                    crate_name: rhs_crate_name,
                    crate_version,
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    wasted_files,
                    suggested_fix: _,
                },
            ) => {
                assert_eq!(lhs_crate_name, rhs_crate_name, "We dont have to support merging different crate versions into a crate of another name");
                Crate {
                    crate_name: lhs_crate_name,
                    total_size_in_bytes: lhs_tsb + rhs_tsb,
                    total_files: lhs_tf + rhs_tf,
                    info_by_version: merge::map_into_map(
                        info_by_version,
                        merge::version_to_new_version_map(
                            crate_version,
                            rhs_tsb,
                            rhs_tf,
                            &wasted_files,
                        ),
                    ),
                    wasted_by_extension: merge::vec_into_map_by_extension(
                        wasted_by_extension,
                        wasted_files,
                    ),
                }
            }
            (
                Crate {
                    crate_name: lhs_crate_name,
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_version: lhs_ibv,
                    wasted_by_extension: lhs_wbe,
                },
                Crate {
                    crate_name: rhs_crate_name,
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    info_by_version: rhs_ibv,
                    wasted_by_extension: rhs_wbe,
                },
            ) => {
                if lhs_crate_name != rhs_crate_name {
                    merge::collection_from_crate(lhs_crate_name, lhs_tsb, lhs_tf, lhs_ibv, lhs_wbe)
                        .merge(Crate {
                            crate_name: rhs_crate_name,
                            total_size_in_bytes: rhs_tsb,
                            total_files: rhs_tf,
                            info_by_version: rhs_ibv,
                            wasted_by_extension: rhs_wbe,
                        })
                } else {
                    Crate {
                        crate_name: lhs_crate_name,
                        total_size_in_bytes: lhs_tsb + rhs_tsb,
                        total_files: lhs_tf + rhs_tf,
                        info_by_version: merge::map_into_map(lhs_ibv, rhs_ibv),
                        wasted_by_extension: merge::map_into_map(lhs_wbe, rhs_wbe),
                    }
                }
            }
            (
                CrateCollection {
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_crate: lhs_ibc,
                    wasted_by_extension: lhs_wbe,
                },
                CrateCollection {
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    info_by_crate: rhs_ibc,
                    wasted_by_extension: rhs_wbe,
                },
            ) => CrateCollection {
                total_size_in_bytes: lhs_tsb + rhs_tsb,
                total_files: lhs_tf + rhs_tf,
                info_by_crate: merge::map_into_map(lhs_ibc, rhs_ibc),
                wasted_by_extension: merge::map_into_map(lhs_wbe, rhs_wbe),
            },
            (
                CrateCollection {
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_crate,
                    wasted_by_extension: lhs_wbe,
                },
                Crate {
                    crate_name,
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    info_by_version,
                    wasted_by_extension: rhs_wbe,
                },
            ) => CrateCollection {
                total_size_in_bytes: lhs_tsb + rhs_tsb,
                total_files: lhs_tf + rhs_tf,
                wasted_by_extension: merge::map_into_map(lhs_wbe, rhs_wbe),
                info_by_crate: merge::map_into_map(
                    info_by_crate,
                    merge::crate_info_from_version_info(crate_name, info_by_version),
                ),
            },
        }
    }

    async fn complete(
        &mut self,
        _out_dir: &Path,
        _progress: &mut prodash::tree::Item,
    ) -> Result<()> {
        Ok(())
    }
    async fn load_previous_state(
        &mut self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Option<Self> {
        None
    }
    async fn store_current_state(
        &mut self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Result<()> {
        Ok(())
    }
}

impl Report {
    pub fn from_result(crate_name: &str, crate_version: &str, result: TaskResult) -> Report {
        match result {
            TaskResult::ExplodedCrate {
                entries_meta_data,
                selected_entries,
            } => {
                let total_size_in_bytes = entries_meta_data.iter().map(|e| e.size).sum();
                let total_files = entries_meta_data.len() as u64;
                let package = Self::package_from_entries(&selected_entries);
                let (suggested_fix, wasted_files) =
                    match (package.include, package.exclude, package.build) {
                        (Some(includes), Some(excludes), _build_script_does_not_matter) => {
                            Self::compute_includes_from_includes_and_excludes(
                                entries_meta_data,
                                includes,
                                excludes,
                            )
                        }
                        (Some(includes), None, build) => Self::enrich_includes(
                            entries_meta_data,
                            selected_entries,
                            includes,
                            build,
                        ),
                        (None, Some(excludes), build) => Self::enrich_excludes(
                            entries_meta_data,
                            selected_entries,
                            excludes,
                            build,
                        ),
                        (None, None, build) => {
                            Self::standard_includes(entries_meta_data, selected_entries, build)
                        }
                    };
                let wasted_files = Self::convert_to_wasted_files(wasted_files);
                Report::Version {
                    crate_name: crate_name.into(),
                    crate_version: crate_version.into(),
                    total_size_in_bytes,
                    total_files,
                    wasted_files,
                    suggested_fix,
                }
            }
            _ => unreachable!("need caller to assure we get exploded crates only"),
        }
    }
}
