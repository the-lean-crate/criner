use super::{
    merge::{fix_to_wasted_files_aggregate, NO_EXT_MARKER},
    Dict, Report, VersionInfo,
};
use crate::engine::report::waste::AggregateFileInfo;
use bytesize::ByteSize;
use horrorshow::{box_html, html, Render, RenderBox, RenderOnce, TemplateBuffer};
use std::iter::FromIterator;

// TODO: fix these unnecessary clones while maintaining composability

fn potential_savings(info_by_crate: &Dict<VersionInfo>) -> Option<AggregateFileInfo> {
    let gains = info_by_crate
        .iter()
        .fold(AggregateFileInfo::default(), |mut s, (_, e)| {
            s += e.potential_gains.clone().unwrap_or_default();
            s
        });
    if gains.total_bytes > 0 {
        Some(gains)
    } else {
        None
    }
}

fn total_section(bytes: u64, files: u64) -> Box<dyn Render> {
    box_html! {
        section {
            h3: "total uncompressed bytes";
            p: format!("{}", ByteSize(bytes))
        }
        section {
            h3: "total files";
            p: files
        }
    }
}

fn savings_section(d: Option<AggregateFileInfo>) -> Box<dyn Render> {
    box_html! {
        @ if let Some(all) = d.as_ref() {
            section {
                h3: "potential savings";
                p: format!("{} total in {} files", ByteSize(all.total_bytes), all.total_files);
            }
        }
    }
}

fn title_section(title: impl Into<String>) -> Box<dyn RenderBox> {
    let title = title.into();
    box_html! {
        title: title
    }
}

fn page_head(title: impl Into<String>) -> Box<dyn RenderBox> {
    let title = title.into();
    box_html! {
        head {
            title: title;
            span(style="position: fixed; top: 1em; right: 1em; color: pink"): "Ugly Alpha 1";
        }
    }
}

fn info_section(info: VersionInfo) -> Box<dyn RenderBox> {
    let VersionInfo {
        all,
        waste,
        potential_gains,
    } = info;
    box_html! {
        section {
            h3: "Total";
            p: format!("{} total in {} files", ByteSize(all.total_bytes), all.total_files);
        }
        section {
            h3: "Waste";
            p: format!("{} wasted in {} files", ByteSize(waste.total_bytes), waste.total_files);
        }
        @ if let Some(gains) = potential_gains {
            section {
                h3: "Potential Gains";
                p: format!("{} potentially gained in {} files", ByteSize(gains.total_bytes), gains.total_files);
            }
        }
    }
}

fn page_footer() -> impl Render {
    html! {
        footer {
            p {
                : "Created by ";
                a(href="https://github.com/Byron/"): "Byron";
            }
            p {
                a(href="https://github.com/crates-io/criner/issues/new"): "Provide feedback";
            }
        }
    }
}

#[derive(Clone, Copy)]
enum SortOrder {
    Name,
    Waste,
}

fn child_items_section(
    title: impl Into<String>,
    info_by_child: Dict<VersionInfo>,
    prefix: String,
    suffix: impl Into<String>,
    order: SortOrder,
) -> Box<dyn RenderBox> {
    let title = title.into();
    let suffix = suffix.into();
    let mut sorted: Vec<_> = Vec::from_iter(info_by_child.into_iter());
    sorted.sort_by(|(ln, le), (rn, re)| match order {
        SortOrder::Name => ln.cmp(rn),
        SortOrder::Waste => le.waste.total_bytes.cmp(&re.waste.total_bytes),
    });
    box_html! {
        section {
            h1: title;
            ol {
                @ for (name, info) in sorted.into_iter().rev() {
                    li {
                        h3 {
                            a(href=format!("{}{}{}", prefix, name, suffix)) {
                                : name
                            }
                        }
                        : info_section(info);
                    }
                }
            }
        }
    }
}

fn by_extension_section(wasted_by_extension: Dict<AggregateFileInfo>) -> Box<dyn RenderBox> {
    let mut sorted: Vec<_> = Vec::from_iter(wasted_by_extension.into_iter());
    sorted.sort_by_key(|(_, e)| e.total_bytes);
    let top_list = 20;
    let skip_info = if sorted.len() > top_list {
        Some((
            sorted.len() - top_list,
            sorted
                .iter()
                .rev()
                .skip(top_list)
                .fold((0, 0), |(tf, tb), e| {
                    (tf + e.1.total_files, tb + e.1.total_bytes)
                }),
        ))
    } else {
        None
    };
    box_html! {
        section {
            h1: "Waste by Extension";
            ol {
                @ for (name, info) in sorted.into_iter().rev().take(top_list) {
                    li {
                        h3 {
                             @ if name.ends_with(NO_EXT_MARKER) {
                                : "no extension"
                               } else {
                                : &format!("*.{}", name)
                              }
                        }
                        p: format!("{} waste in {} files", ByteSize(info.total_bytes), info.total_files);
                    }
                }
            }
            @ if let Some((num_skipped, (tf, tb))) = skip_info {
                p: format!("Skipped {} extensions totalling {} files and {}", num_skipped, tf, ByteSize(tb))
            }
        }
    }
}

impl RenderOnce for Report {
    fn render_once(self, tmpl: &mut TemplateBuffer<'_>)
    where
        Self: Sized,
    {
        use super::Report::*;
        match self {
            Version {
                crate_name,
                crate_version,
                total_files,
                total_size_in_bytes,
                mut wasted_files,
                suggested_fix,
            } => {
                wasted_files.sort_by_key(|(_, s)| *s);
                let title = format!("{}:{}", crate_name, crate_version);
                tmpl << html! {
                    : page_head(title.clone());
                    body {
                        article {
                            : title_section(title);
                            : total_section(total_size_in_bytes, total_files);
                            : savings_section(fix_to_wasted_files_aggregate(suggested_fix.clone()));
                            @ if suggested_fix.is_some() {
                                section {
                                    h3: "Fix";
                                    section {
                                        |t| write!(t, "{:#?}", suggested_fix.unwrap())
                                    }
                                }
                            } else {
                                p: "Perfectly lean!"
                            }
                            @ if !wasted_files.is_empty() {
                                section {
                                    h3: format!("{} wasted files", wasted_files.len());
                                    p: format!("total waste: {}", ByteSize(wasted_files.iter().map(|(_, s)| *s).sum::<u64>()));
                                    ol {
                                        @ for (path, size) in wasted_files.into_iter().rev() {
                                            li : format_args!("{} : {}", path, ByteSize(size))
                                        }
                                    }
                                }
                            }
                        }
                    }
                    : page_footer();
                }
            }
            Crate {
                crate_name,
                total_size_in_bytes,
                total_files,
                info_by_version,
                wasted_by_extension,
            } => {
                let gains = potential_savings(&info_by_version);
                tmpl << html! {
                    : page_head(crate_name.clone());
                    body {
                        article {
                            : title_section(crate_name.clone());
                            : total_section(total_size_in_bytes, total_files);
                            : savings_section(gains);
                            : by_extension_section(wasted_by_extension);
                            : child_items_section("Versions", info_by_version, format!("{}/", crate_name), ".html", SortOrder::Name);
                        }
                    }
                    : page_footer();
                }
            }
            CrateCollection {
                total_size_in_bytes,
                total_files,
                info_by_crate,
                wasted_by_extension,
            } => {
                let title = "crates.io";
                let no_prefix = String::new();
                let no_suffix = String::new();
                let gains = potential_savings(&info_by_crate);
                let (waste_in_bytes, wasted_files_count) =
                    wasted_by_extension
                        .iter()
                        .fold((0, 0), |(waste_bytes, waste_files), e| {
                            (waste_bytes + e.1.total_bytes, waste_files + e.1.total_files)
                        });
                tmpl << html! {
                    : page_head(title);
                    body {
                        article {
                            : title_section(title);
                            : total_section(total_size_in_bytes, total_files);
                            section {
                                h3: format!("{} wasted in {} files", ByteSize(waste_in_bytes), wasted_files_count);
                            }
                            : savings_section(gains);
                            : by_extension_section(wasted_by_extension);
                            : child_items_section("Crates", info_by_crate, no_prefix, no_suffix, SortOrder::Waste);
                        }
                    }
                    : page_footer();
                }
            }
        }
    }
}
