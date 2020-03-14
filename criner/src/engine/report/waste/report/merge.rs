use super::{AggregateFileInfo, Dict, Report, VersionInfo, WastedFile};
use crate::engine::report::waste::AggregateVersionInfo;
use std::ops::AddAssign;
use std::{collections::BTreeMap, path::PathBuf};

impl std::ops::AddAssign for AggregateFileInfo {
    fn add_assign(&mut self, rhs: Self) {
        self.total_bytes += rhs.total_bytes;
        self.total_files += rhs.total_files;
    }
}

impl std::ops::AddAssign for VersionInfo {
    fn add_assign(&mut self, rhs: Self) {
        self.all += rhs.all;
        self.waste += rhs.waste;
    }
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
        },
    );
    m
}

pub fn crate_info_from_version_info(
    crate_name: String,
    info_by_version: Dict<VersionInfo>,
) -> Dict<AggregateVersionInfo> {
    let v = info_by_version
        .into_iter()
        .fold(AggregateVersionInfo::default(), |mut a, (_, v)| {
            a.waste.add_assign(v.waste);
            a.all.add_assign(v.all);
            a
        });

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
        info_by_crate: crate_info_from_version_info(crate_name, info_by_version),
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
            suggested_fix: _,
        } => Report::Crate {
            crate_name,
            info_by_version: version_to_new_version_map(
                crate_version,
                total_size_in_bytes,
                total_files,
                &wasted_files,
            ),
            total_size_in_bytes,
            total_files,
            wasted_by_extension: into_map_by_extension(wasted_files),
        },
        _ => unreachable!("must only be called with version variant"),
    }
}
