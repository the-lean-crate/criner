use crate::{error::Result, model};
use std::path::Path;

pub struct Generator;

impl Generator {
    pub fn write_files<'a>(
        &mut self,
        _out_dir: impl AsRef<Path>,
        _krates: impl IntoIterator<Item = model::Crate<'a>>,
    ) -> Result<()> {
        Ok(())
    }
}
