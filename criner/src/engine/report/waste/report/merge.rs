use super::{AggregateFileInfo, Dict, Report, VersionInfo, WastedFile};
use crate::{
    engine::report::waste::{AggregateVersionInfo, Fix},
    Result,
};
use async_trait::async_trait;
use std::{collections::BTreeMap, ops::AddAssign, path::Path, path::PathBuf};

impl std::ops::AddAssign for AggregateFileInfo {
    fn add_assign(&mut self, rhs: Self) {
        let Self {
            total_bytes,
            total_files,
        } = rhs;
        self.total_bytes += total_bytes;
        self.total_files += total_files;
    }
}

impl std::ops::AddAssign for VersionInfo {
    fn add_assign(&mut self, rhs: Self) {
        let Self {
            all,
            waste,
            potential_gains,
            waste_latest_version,
        } = rhs;
        self.all += all;
        self.waste += waste;
        self.potential_gains =
            add_optional_aggregate(self.potential_gains.clone(), potential_gains);
        self.waste_latest_version =
            add_named_optional_aggregate(self.waste_latest_version.clone(), waste_latest_version);
    }
}

pub fn add_named_optional_aggregate(
    lhs: Option<(String, AggregateFileInfo)>,
    rhs: Option<(String, AggregateFileInfo)>,
) -> Option<(String, AggregateFileInfo)> {
    Some(match (lhs, rhs) {
        (Some((lhs_name, lhs)), Some((rhs_name, _))) if lhs_name > rhs_name => (lhs_name, lhs),
        (Some(_), Some((rhs_name, rhs))) => (rhs_name, rhs),
        (Some(v), None) => v,
        (None, Some(v)) => v,
        (None, None) => return None,
    })
}

pub fn add_optional_aggregate(
    lhs: Option<AggregateFileInfo>,
    rhs: Option<AggregateFileInfo>,
) -> Option<AggregateFileInfo> {
    Some(match (lhs, rhs) {
        (Some(mut lhs), Some(rhs)) => {
            lhs += rhs;
            lhs
        }
        (Some(v), None) => v,
        (None, Some(v)) => v,
        (None, None) => return None,
    })
}

pub const NO_EXT_MARKER: &str = "<NO_EXT>";

pub fn vec_into_map_by_extension(
    initial: Dict<AggregateFileInfo>,
    from: Vec<WastedFile>,
) -> Dict<AggregateFileInfo> {
    from.into_iter().fold(initial, |mut m, e| {
        let entry = m
            .entry(
                PathBuf::from(e.0)
                    .extension()
                    .and_then(|oss| oss.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| NO_EXT_MARKER.to_string()),
            )
            .or_insert_with(Default::default);
        entry.total_bytes += e.1;
        entry.total_files += 1;
        m
    })
}

pub fn fix_to_wasted_files_aggregate(fix: Option<Fix>) -> Option<AggregateFileInfo> {
    match fix.unwrap_or(Fix::RemoveExclude) {
        Fix::ImprovedInclude {
            potential: Some(potential),
            ..
        } => Some(potential.potential_waste),
        _ => None,
    }
    .map(|v| {
        v.into_iter()
            .fold(AggregateFileInfo::default(), |mut a, e| {
                a.total_files += 1;
                a.total_bytes += e.size;
                a
            })
    })
}

pub fn into_map_by_extension(from: Vec<WastedFile>) -> Dict<AggregateFileInfo> {
    vec_into_map_by_extension(BTreeMap::new(), from)
}

pub fn map_into_map<T>(lhs: Dict<T>, rhs: Dict<T>) -> Dict<T>
where
    T: std::ops::AddAssign + Default,
{
    rhs.into_iter().fold(lhs, |mut m, (k, v)| {
        let entry = m.entry(k).or_insert_with(Default::default);
        entry.add_assign(v);
        m
    })
}

pub fn byte_count(files: &[WastedFile]) -> u64 {
    files.iter().map(|e| e.1).sum::<u64>()
}

pub fn version_to_new_version_map(
    crate_version: String,
    total_size_in_bytes: u64,
    total_files: u64,
    wasted_files: &[WastedFile],
    potential_gains: Option<AggregateFileInfo>,
) -> Dict<VersionInfo> {
    let mut m = BTreeMap::new();
    m.insert(
        crate_version,
        VersionInfo {
            all: AggregateFileInfo {
                total_bytes: total_size_in_bytes,
                total_files,
            },
            waste: AggregateFileInfo {
                total_bytes: byte_count(&wasted_files),
                total_files: wasted_files.len() as u64,
            },
            potential_gains,
            waste_latest_version: None,
        },
    );
    m
}

pub fn crate_collection_info_from_version_info(
    crate_name: String,
    info_by_version: Dict<VersionInfo>,
) -> Dict<AggregateVersionInfo> {
    let (_, v) = info_by_version.into_iter().fold(
        (String::new(), AggregateVersionInfo::default()),
        |(mut previous_name, mut a), (version_name, v)| {
            let VersionInfo {
                waste,
                all,
                potential_gains,
                waste_latest_version: _unused_and_always_none,
            } = v;
            a.waste.add_assign(waste.clone());
            a.all.add_assign(all);
            a.potential_gains = add_optional_aggregate(a.potential_gains.clone(), potential_gains);
            a.waste_latest_version = if version_name > previous_name {
                previous_name = version_name.clone();
                Some((version_name, waste))
            } else {
                a.waste_latest_version
            };
            (previous_name, a)
        },
    );

    let mut m = BTreeMap::new();
    m.insert(crate_name, v);
    m
}

pub fn collection_from_crate(
    crate_name: String,
    total_size_in_bytes: u64,
    total_files: u64,
    info_by_version: Dict<VersionInfo>,
    wasted_by_extension: Dict<AggregateFileInfo>,
) -> Report {
    Report::CrateCollection {
        total_size_in_bytes,
        total_files,
        info_by_crate: crate_collection_info_from_version_info(crate_name, info_by_version),
        wasted_by_extension,
    }
}

pub fn crate_from_version(version: Report) -> Report {
    match version {
        Report::Version {
            crate_name,
            crate_version,
            total_size_in_bytes,
            total_files,
            wasted_files,
            suggested_fix,
        } => Report::Crate {
            crate_name,
            info_by_version: version_to_new_version_map(
                crate_version,
                total_size_in_bytes,
                total_files,
                &wasted_files,
                fix_to_wasted_files_aggregate(suggested_fix),
            ),
            total_size_in_bytes,
            total_files,
            wasted_by_extension: into_map_by_extension(wasted_files),
        },
        _ => unreachable!("must only be called with version variant"),
    }
}

#[async_trait]
impl crate::engine::report::generic::Aggregate for Report {
    fn merge(self, other: Self) -> Self {
        use Report::*;
        match (self, other) {
            (lhs @ Version { .. }, rhs @ Version { .. }) => crate_from_version(lhs).merge(rhs),
            (version @ Version { .. }, krate @ Crate { .. }) => krate.merge(version),
            (version @ Version { .. }, collection @ CrateCollection { .. }) => {
                collection.merge(version)
            }
            (collection @ CrateCollection { .. }, version @ Version { .. }) => {
                collection.merge(crate_from_version(version))
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
                    suggested_fix,
                },
            ) => {
                if lhs_crate_name == rhs_crate_name {
                    Crate {
                        crate_name: lhs_crate_name,
                        total_size_in_bytes: lhs_tsb + rhs_tsb,
                        total_files: lhs_tf + rhs_tf,
                        info_by_version: map_into_map(
                            info_by_version,
                            version_to_new_version_map(
                                crate_version,
                                rhs_tsb,
                                rhs_tf,
                                &wasted_files,
                                fix_to_wasted_files_aggregate(suggested_fix),
                            ),
                        ),
                        wasted_by_extension: vec_into_map_by_extension(
                            wasted_by_extension,
                            wasted_files,
                        ),
                    }
                } else {
                    collection_from_crate(
                        lhs_crate_name,
                        lhs_tsb,
                        lhs_tf,
                        info_by_version,
                        wasted_by_extension,
                    )
                    .merge(Version {
                        crate_name: rhs_crate_name,
                        crate_version,
                        total_size_in_bytes: rhs_tsb,
                        total_files: rhs_tf,
                        wasted_files,
                        suggested_fix,
                    })
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
                    collection_from_crate(lhs_crate_name, lhs_tsb, lhs_tf, lhs_ibv, lhs_wbe).merge(
                        Crate {
                            crate_name: rhs_crate_name,
                            total_size_in_bytes: rhs_tsb,
                            total_files: rhs_tf,
                            info_by_version: rhs_ibv,
                            wasted_by_extension: rhs_wbe,
                        },
                    )
                } else {
                    Crate {
                        crate_name: lhs_crate_name,
                        total_size_in_bytes: lhs_tsb + rhs_tsb,
                        total_files: lhs_tf + rhs_tf,
                        info_by_version: map_into_map(lhs_ibv, rhs_ibv),
                        wasted_by_extension: map_into_map(lhs_wbe, rhs_wbe),
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
                info_by_crate: map_into_map(lhs_ibc, rhs_ibc),
                wasted_by_extension: map_into_map(lhs_wbe, rhs_wbe),
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
                wasted_by_extension: map_into_map(lhs_wbe, rhs_wbe),
                info_by_crate: map_into_map(
                    info_by_crate,
                    crate_collection_info_from_version_info(crate_name, info_by_version),
                ),
            },
        }
    }

    async fn complete(
        &mut self,
        progress: &mut prodash::tree::Item,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        use horrorshow::Template;

        progress.blocked("writing report to disk", None);
        let report = self.clone();
        report.write_to_io(out)?;
        Ok(())
    }
    async fn load_previous_state(
        &self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Option<Self> {
        if let Some(path) = self.path_to_storage_location(out_dir) {
            progress.blocked("loading previous waste report from disk", None);
            async_std::fs::read(path)
                .await
                .ok()
                .and_then(|v| rmp_serde::from_read(v.as_slice()).ok())
        } else {
            None
        }
    }
    async fn store_current_state(
        &self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Result<()> {
        let path = self
            .path_to_storage_location(out_dir)
            .expect("a path for every occasion");
        progress.blocked("storing current waste report to disk", None);
        let data = rmp_serde::to_vec(self)?;
        async_std::fs::write(path, data).await.map_err(Into::into)
    }
}
