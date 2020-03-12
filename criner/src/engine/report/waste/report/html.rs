use super::Report;
use horrorshow::{html, RenderOnce, TemplateBuffer};

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
                    head {
                        title: &title
                    }
                    body {
                        article {
                            header {
                                h1 : title
                            }
                            section {
                                h3: "total uncompressed bytes";
                                p: total_size_in_bytes
                            }
                            section {
                                h3: "total files";
                                p: total_files
                            }
                            section {
                                h3: "wasted files";
                                ol(id="count") {
                                    // You can embed for loops, while loops, and if statements.
                                    @ for (path, size) in wasted_files {
                                        li : format_args!("{} : {}", path, bytesize::ByteSize(size))
                                    }
                                }
                            }
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
                        }
                    }
                }
            }
            Crate { .. } => unimplemented!("html crate"),
            CrateCollection { .. } => unimplemented!("html crate collection"),
        }
    }
}
