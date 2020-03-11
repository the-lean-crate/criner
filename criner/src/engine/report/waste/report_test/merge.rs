use super::super::{AggregateFileInfo, Fix, Report, VersionInfo};
use crate::engine::report::generic::Aggregate;
use common_macros::b_tree_map;
use std::collections::BTreeMap;

#[test]
fn crate_version_and_version_crate() {
    let version = Report::Version {
        crate_name: "a".into(),
        crate_version: "1".into(),
        total_size_in_bytes: 1,
        total_files: 4,
        wasted_files: vec![("a.a".into(), 20)],
        suggested_fix: Some(Fix::RemoveExclude),
    };

    let krate = Report::Crate {
        crate_name: "a".into(),
        total_size_in_bytes: 3,
        total_files: 9,
        info_by_version: BTreeMap::new(),
        wasted_by_extension: b_tree_map! {
            "a".into()  => AggregateFileInfo {total_files: 2, total_bytes: 60},
            "b".into()  => AggregateFileInfo {total_files: 3, total_bytes: 80},
            "c".into()  => AggregateFileInfo {total_files: 1, total_bytes: 90},
        },
    };
    assert_eq!(version.clone().merge(krate.clone()), krate.merge(version));
}

#[test]
fn crate_and_crate_of_same_name() {
    assert_eq!(
        Report::Crate {
            crate_name: "a".into(),
            total_size_in_bytes: 3,
            total_files: 9,
            info_by_version: b_tree_map! {
                "1".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                    waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                }
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
                "b".into()  => AggregateFileInfo {total_files: 2, total_bytes: 20},
                "c".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
            },
        }
        .merge(Report::Crate {
            crate_name: "a".into(),
            total_size_in_bytes: 9,
            total_files: 3,
            info_by_version: b_tree_map! {
                "2".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                    waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                }
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
                "b".into()  => AggregateFileInfo {total_files: 2, total_bytes: 20},
                "d".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
            },
        }),
        Report::Crate {
            crate_name: "a".to_string(),
            total_size_in_bytes: 12,
            total_files: 12,
            info_by_version: b_tree_map! {
                "1".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                    waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                },
                "2".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                    waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                }
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 4, total_bytes: 40},
                "b".into()  => AggregateFileInfo {total_files: 4, total_bytes: 40},
                "c".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
                "d".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
            },
        }
    );
}

#[test]
fn two_versions_of_same_crate() {
    assert_eq!(
        Report::Version {
            crate_name: "a".into(),
            crate_version: "1".into(),
            total_size_in_bytes: 1,
            total_files: 4,
            wasted_files: vec![
                ("a.a".into(), 20),
                ("b/a.b".into(), 20),
                ("c/a.b".into(), 10)
            ],
            suggested_fix: Some(Fix::RemoveExclude)
        }
        .merge(Report::Version {
            crate_name: "a".into(),
            crate_version: "2".into(),
            total_size_in_bytes: 2,
            total_files: 5,
            wasted_files: vec![
                ("a.a".into(), 40),
                ("c/a.b".into(), 50),
                ("d/a.c".into(), 90)
            ],
            suggested_fix: None
        }),
        Report::Crate {
            crate_name: "a".into(),
            total_size_in_bytes: 3,
            total_files: 9,
            info_by_version: b_tree_map! {
                 "1".into() => VersionInfo {
                                all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                                waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                              },
                 "2".into() => VersionInfo {
                                all: AggregateFileInfo { total_files: 5, total_bytes: 2 },
                                waste: AggregateFileInfo { total_files: 3, total_bytes: 180 },
                              },
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 2, total_bytes: 60},
                "b".into()  => AggregateFileInfo {total_files: 3, total_bytes: 80},
                "c".into()  => AggregateFileInfo {total_files: 1, total_bytes: 90},
            },
        }
    );
}
