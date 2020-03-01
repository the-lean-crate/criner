mod krate;
mod meta;
mod task;

pub trait SqlConvert {
    fn replace_statement() -> &'static str;
    fn secondary_replace_statement() -> Option<&'static str> {
        None
    }
    fn source_table_name() -> &'static str;
    fn init_table_statement() -> &'static str;
    fn insert(
        &self,
        key: &str,
        uid: i32,
        stm: &mut rusqlite::Statement,
        sstm: Option<&mut rusqlite::Statement>,
    ) -> crate::error::Result<usize>;
}
