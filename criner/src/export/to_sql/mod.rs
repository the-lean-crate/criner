mod dbdump_crate;
mod krate;
mod krate_version;
mod meta;
mod result;
mod task;

pub fn to_seconds_since_epoch(time: std::time::SystemTime) -> i64 {
    time.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

pub trait SqlConvert {
    fn convert_to_sql(
        _input_statement: &mut rusqlite::Statement,
        _transaction: &rusqlite::Transaction,
    ) -> Option<crate::Result<usize>> {
        None
    }
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
    ) -> crate::Result<usize>;
}
