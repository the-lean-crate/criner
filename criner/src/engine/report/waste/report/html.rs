use super::Report;
use bytesize::ByteSize;
use horrorshow::{box_html, html, Render, RenderBox, RenderOnce, TemplateBuffer};

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

fn title_section(title: String) -> Box<dyn RenderBox> {
    box_html! {
        head {
            title: title
        }
    }
}

fn page_head(title: String) -> Box<dyn RenderBox> {
    box_html! {
        head {
            title: title
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
                wasted_files,
                suggested_fix,
            } => {
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
                                    p: format!("total waste: {}", ByteSize(wasted_files.iter().map(|(_, s)| s).sum::<u64>()));
                                    ol(id="count") {
                                        // You can embed for loops, while loops, and if statements.
                                        @ for (path, size) in wasted_files {
                                            li : format_args!("{} : {}", path, ByteSize(size))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Crate {
                crate_name,
                total_size_in_bytes,
                total_files,
                info_by_version: _,
                wasted_by_extension: _,
            } => {
                tmpl << html! {
                    : page_head(crate_name.clone());
                    body {
                        article {
                            : title_section(crate_name);
                            : total_section(total_size_in_bytes, total_files);
                        }
                    }
                }
            }
            CrateCollection { .. } => unimplemented!("html crate collection"),
        }
    }
}
