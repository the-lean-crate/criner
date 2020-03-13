use super::{Dict, Report, VersionInfo};
use crate::engine::report::waste::AggregateFileInfo;
use bytesize::ByteSize;
use horrorshow::{box_html, html, Render, RenderBox, RenderOnce, TemplateBuffer};
use std::iter::FromIterator;

// TODO: fix these unnecessary clones while maintaining composability

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

fn title_section(title: impl Into<String>) -> Box<dyn RenderBox> {
    let title = title.into();
    box_html! {
        head {
            title: title
        }
    }
}

fn page_head(title: impl Into<String>) -> Box<dyn RenderBox> {
    let title = title.into();
    box_html! {
        head {
            title: title
        }
    }
}

fn info_section(info: VersionInfo) -> Box<dyn RenderBox> {
    let VersionInfo { all, waste } = info;
    box_html! {
        section {
            h3: "Total";
            p: format!("{} total in {} files", ByteSize(all.total_bytes), all.total_files);
        }
        section {
            h3: "Waste";
            p: format!("{} wasted in {} files", ByteSize(waste.total_bytes), waste.total_files);
        }
    }
}

fn page_footer() -> impl Render {
    html! {
        footer {
            p {
                : "Created by";
                a(href="https://github.com/Byron/"): "Byron";
            }
            p {
                a(href="https://github.com/crates-io/criner/issues/new"): "Provide feedback";
            }
        }
    }
}

fn child_items_section(info_by_child: Dict<VersionInfo>) -> Box<dyn RenderBox> {
    let mut sorted: Vec<_> = Vec::from_iter(info_by_child.into_iter());
    sorted.sort_by_key(|(_, e)| e.waste.total_bytes);
    box_html! {
        section {
            ol {
                @ for (name, info) in sorted.into_iter().rev() {
                    li {
                        h3 {
                            a(href=&name) {
                                name
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
    box_html! {
        section {
            ol {
                @ for (name, info) in sorted.into_iter().rev() {
                    li {
                        h3: format!("*.{}", name);
                        p: format!("{} waste in {} files", ByteSize(info.total_bytes), info.total_files);
                    }
                }
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
                tmpl << html! {
                    : page_head(crate_name.clone());
                    body {
                        article {
                            : title_section(crate_name);
                            : total_section(total_size_in_bytes, total_files);
                            : child_items_section(info_by_version);
                            : by_extension_section(wasted_by_extension);
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
                tmpl << html! {
                    : page_head(title);
                    body {
                        article {
                            : title_section(title);
                            : total_section(total_size_in_bytes, total_files);
                            : child_items_section(info_by_crate);
                            : by_extension_section(wasted_by_extension);
                        }
                    }
                    : page_footer();
                }
            }
        }
    }
}
