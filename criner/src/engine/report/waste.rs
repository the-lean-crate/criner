use crate::{error::Result, model, persistence};
use std::path::{Path, PathBuf};

const GENERATOR_VERSION: &str = "1.0.0";

pub struct Generator {
    pub db: persistence::Db,
}

impl Generator {
    pub fn write_files<'a>(
        &mut self,
        out_dir: impl AsRef<Path>,
        krates: impl IntoIterator<Item = (sled::IVec, model::Crate<'a>)>,
        mut progress: prodash::tree::Item,
    ) -> Result<()> {
        use std::os::unix::fs;
        for (k, c) in krates.into_iter() {
            let name = to_name(&k);
            let c: model::Crate = c;
            let mut p = progress.add_child(name);
            p.init(Some(c.versions.len() as u32), Some("versions"));

            for (vid, v) in c.versions.iter().enumerate() {
                p.set((vid + 1) as u32);
                let out_file = output_file_html(out_dir.as_ref(), name, &v);
                std::fs::create_dir_all(out_file.parent().expect("parent dir for file"))?;

                let mut marker = out_file.clone();
                marker.set_file_name(GENERATOR_VERSION);
                if !marker
                    .symlink_metadata()
                    .map(|m| m.is_file())
                    .unwrap_or(false)
                {
                    generate_single_file(&out_file)?;
                    fs::symlink(&out_file, &marker)?;
                }
            }
        }
        Ok(())
    }
}

fn generate_single_file(out: &Path) -> Result<()> {
    std::fs::write(out, "hello world").map_err(Into::into)
}

fn output_file_html(base: &Path, name: &str, version: &str) -> PathBuf {
    base.join(name).join(version).join("index.html")
}

fn to_name(key: &sled::IVec) -> &str {
    std::str::from_utf8(key).expect("unicode keys")
}
