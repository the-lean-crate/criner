use crate::{
    engine::report::generic::Aggregate,
    engine::report::waste::{AggregateFileInfo, Fix, PotentialWaste, Report, VersionInfo},
    model::TarHeader,
};
use common_macros::b_tree_map;
use std::collections::BTreeMap;

#[test]
fn crate_merging_version_equivalent_to_version_merging_crate() {
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
fn crate_and_crate_of_different_name() {
    assert_eq!(
        Report::Crate {
            crate_name: "a".into(),
            total_size_in_bytes: 3,
            total_files: 9,
            info_by_version: b_tree_map! {
                "1".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                    waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                    potential_gains: Some(AggregateFileInfo {
                        total_bytes: 2,
                        total_files: 8
                    }),
                    waste_latest_version: None,
                },
                "2".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                    waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                    potential_gains: None,
                    waste_latest_version: None,
                }
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
                "b".into()  => AggregateFileInfo {total_files: 2, total_bytes: 20},
                "c".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
            },
        }
        .merge(Report::Crate {
            crate_name: "b".into(),
            total_size_in_bytes: 9,
            total_files: 3,
            info_by_version: b_tree_map! {
                "2".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                    waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                    potential_gains: None,
                    waste_latest_version: None,
                }
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
                "b".into()  => AggregateFileInfo {total_files: 2, total_bytes: 20},
                "d".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
            },
        }),
        Report::CrateCollection {
            total_size_in_bytes: 12,
            total_files: 12,
            info_by_crate: b_tree_map! {
                "a".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 4*2, total_bytes: 1*2},
                    waste: AggregateFileInfo { total_files: 3*2, total_bytes: 50*2},
                    potential_gains: Some(AggregateFileInfo {
                        total_bytes: 2,
                        total_files: 8
                    }),
                    waste_latest_version: Some(("2".into(), AggregateFileInfo { total_files: 3, total_bytes: 50 }))
                },
                "b".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                    waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                    potential_gains: None,
                    waste_latest_version: Some(("2".into(), AggregateFileInfo { total_files: 6, total_bytes: 150 }))
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
fn two_crate_collections() {
    let lhs_collection = Report::CrateCollection {
        total_size_in_bytes: 12,
        total_files: 10,
        info_by_crate: b_tree_map! {
            "a".into() => VersionInfo {
                all: AggregateFileInfo { total_files: 4, total_bytes: 1},
                waste: AggregateFileInfo { total_files: 3, total_bytes: 50},
                potential_gains: Some(AggregateFileInfo {
                    total_files: 5,
                    total_bytes: 10,
                }),
                waste_latest_version: Some(("3".into(), AggregateFileInfo { total_files: 1, total_bytes: 20},))
            },
        },
        wasted_by_extension: b_tree_map! {
            "a".into()  => AggregateFileInfo {total_files: 4, total_bytes: 40},
            "b".into()  => AggregateFileInfo {total_files: 4, total_bytes: 40},
            "c".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
            "d".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
        },
    };
    let rhs_collection = Report::CrateCollection {
        total_size_in_bytes: 12,
        total_files: 10,
        info_by_crate: b_tree_map! {
            "a".into() => VersionInfo {
                all: AggregateFileInfo { total_files: 40, total_bytes: 10},
                waste: AggregateFileInfo { total_files: 30, total_bytes: 500},
                potential_gains: Some(AggregateFileInfo {
                    total_files: 50,
                    total_bytes: 100,
                }),
                waste_latest_version: Some(("4".into(), AggregateFileInfo { total_files: 2, total_bytes: 40}))
            },
            "b".into() => VersionInfo {
                all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                potential_gains: None,
                waste_latest_version: Some(("1".into(), AggregateFileInfo { total_files: 3, total_bytes: 50}))
            },
        },
        wasted_by_extension: b_tree_map! {
            "a".into()  => AggregateFileInfo {total_files: 4, total_bytes: 40},
            "b".into()  => AggregateFileInfo {total_files: 4, total_bytes: 40},
            "c".into()  => AggregateFileInfo {total_files: 3, total_bytes: 30},
            "d".into()  => AggregateFileInfo {total_files: 1, total_bytes: 10},
            "e".into()  => AggregateFileInfo {total_files: 4, total_bytes: 2},
        },
    };
    assert_eq!(
        lhs_collection.merge(rhs_collection),
        Report::CrateCollection {
            total_size_in_bytes: 24,
            total_files: 20,
            info_by_crate: b_tree_map! {
                "a".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 40+4, total_bytes: 10 +1},
                    waste: AggregateFileInfo { total_files: 30+3, total_bytes: 500+50},
                    potential_gains: Some(AggregateFileInfo {
                        total_files: 55,
                        total_bytes: 110
                    }),
                    waste_latest_version: Some(("4".into(), AggregateFileInfo { total_files: 2, total_bytes: 40}))
                },
                "b".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                    waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                    potential_gains: None,
                    waste_latest_version: Some(("1".into(), AggregateFileInfo { total_files: 3, total_bytes: 50}))
                }
            },
            wasted_by_extension: b_tree_map! {
                "a".into()  => AggregateFileInfo {total_files: 4*2, total_bytes: 40*2},
                "b".into()  => AggregateFileInfo {total_files: 4*2, total_bytes: 40*2},
                "c".into()  => AggregateFileInfo {total_files: 3*2, total_bytes: 30*2},
                "d".into()  => AggregateFileInfo {total_files: 1*2, total_bytes: 10*2},
                "e".into()  => AggregateFileInfo {total_files: 4, total_bytes: 2},
            },
        }
    );
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
                    potential_gains: Some(AggregateFileInfo {
                        total_files: 50,
                        total_bytes: 100
                    }),
                    waste_latest_version: None,
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
                    potential_gains: Some(AggregateFileInfo {
                        total_files: 5,
                        total_bytes: 10
                    }),
                    waste_latest_version: None,
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
                    potential_gains:Some(AggregateFileInfo {
                        total_files: 50,
                        total_bytes: 100
                    }),
                    waste_latest_version: None,
                },
                "2".into() => VersionInfo {
                    all: AggregateFileInfo { total_files: 8, total_bytes: 10 },
                    waste: AggregateFileInfo { total_files: 6, total_bytes: 150 },
                    potential_gains: Some(AggregateFileInfo {
                        total_files: 5,
                        total_bytes: 10
                    }),
                    waste_latest_version: None,
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
            suggested_fix: Some(Fix::ImprovedInclude {
                include: vec![],
                include_removed: vec![],
                potential: Some(PotentialWaste {
                    patterns_to_fix: vec![],
                    potential_waste: vec![TarHeader {
                        path: (&b"a/d.c"[..]).into(),
                        size: 10,
                        entry_type: 0
                    }]
                }),
                has_build_script: false
            })
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
            suggested_fix: Some(Fix::ImprovedInclude {
                include: vec![],
                include_removed: vec![],
                potential: Some(PotentialWaste {
                    patterns_to_fix: vec![],
                    potential_waste: vec![TarHeader {
                        path: (&b"a/d.c"[..]).into(),
                        size: 100,
                        entry_type: 0
                    }]
                }),
                has_build_script: false
            })
        }),
        Report::Crate {
            crate_name: "a".into(),
            total_size_in_bytes: 3,
            total_files: 9,
            info_by_version: b_tree_map! {
                 "1".into() => VersionInfo {
                                all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                                waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                                potential_gains: Some(AggregateFileInfo {total_files: 1, total_bytes: 10}),
                                waste_latest_version: None,
                              },
                 "2".into() => VersionInfo {
                                all: AggregateFileInfo { total_files: 5, total_bytes: 2 },
                                waste: AggregateFileInfo { total_files: 3, total_bytes: 180 },
                                potential_gains: Some(AggregateFileInfo {total_files: 1, total_bytes: 100}),
                                waste_latest_version: None,
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

#[test]
fn two_versions_of_different_crate() {
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
            suggested_fix: Some(Fix::ImprovedInclude {
                include: vec![],
                include_removed: vec![],
                potential: Some(PotentialWaste {
                    patterns_to_fix: vec![],
                    potential_waste: vec![TarHeader {
                        path: (&b"a/b.c"[..]).into(),
                        size: 10,
                        entry_type: 0
                    }]
                }),
                has_build_script: false
            })
        }
        .merge(Report::Version {
            crate_name: "b".into(),
            crate_version: "1".into(),
            total_size_in_bytes: 2,
            total_files: 5,
            wasted_files: vec![
                ("a.a".into(), 40),
                ("c/a.b".into(), 50),
                ("d/a.c".into(), 90)
            ],
            suggested_fix: Some(Fix::ImprovedInclude {
                include: vec![],
                include_removed: vec![],
                potential: Some(PotentialWaste {
                    patterns_to_fix: vec![],
                    potential_waste: vec![TarHeader {
                        path: (&b"a/d.c"[..]).into(),
                        size: 100,
                        entry_type: 0
                    }]
                }),
                has_build_script: false
            })
        }),
        Report::CrateCollection {
            total_size_in_bytes: 3,
            total_files: 9,
            info_by_crate: b_tree_map! {
                 "a".into() => VersionInfo {
                                all: AggregateFileInfo { total_files: 4, total_bytes: 1 },
                                waste: AggregateFileInfo { total_files: 3, total_bytes: 50 },
                                potential_gains: Some(AggregateFileInfo{total_files: 1, total_bytes: 10}),
                                waste_latest_version: Some(("1".into(), AggregateFileInfo { total_files: 3, total_bytes: 50 }))
                              },
                 "b".into() => VersionInfo {
                                all: AggregateFileInfo { total_files: 5, total_bytes: 2 },
                                waste: AggregateFileInfo { total_files: 3, total_bytes: 180 },
                                potential_gains: Some(AggregateFileInfo{total_files: 1, total_bytes: 100}),
                                waste_latest_version: Some(("1".into(), AggregateFileInfo { total_files: 3, total_bytes: 180 }))
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
