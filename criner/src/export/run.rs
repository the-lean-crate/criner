use super::to_sql::SqlConvert;
use crate::model;
use rusqlite::{Connection, NO_PARAMS};
use std::path::Path;

pub fn run_blocking(
    source_db: impl AsRef<Path>,
    destination_db: impl AsRef<Path>,
) -> crate::error::Result<()> {
    if destination_db.as_ref().is_file() {
        return Err(crate::Error::Message(format!(
            "Destination database at '{}' does already exist - this is currently unsupported",
            destination_db.as_ref().display()
        )));
    }
    let mut input = Connection::open(source_db)?;
    let mut output = Connection::open(destination_db)?;

    // Turn off keychecks during insertion - we assume we can't get it wrong
    // However, we do embed foreign key relations as form of documentation.
    output.execute_batch(
        "
    PRAGMA foreign_keys = FALSE; -- assume we don't mess up relations, save validation time
    PRAGMA journal_mode = 'OFF' -- no journal, direct writes
",
    )?;

    transfer::<model::Crate>(&mut input, &mut output)?;
    transfer::<model::Task>(&mut input, &mut output)?;
    transfer::<model::Context>(&mut input, &mut output)?;
    transfer::<model::CrateVersion>(&mut input, &mut output)?;
    transfer::<model::TaskResult>(&mut input, &mut output)?;

    Ok(())
}

fn transfer<T>(input: &mut Connection, output: &mut Connection) -> crate::error::Result<()>
where
    for<'a> T: SqlConvert + From<&'a [u8]>,
{
    output.execute_batch(T::init_table_statement())?;
    let mut istm = input.prepare(&format!("SELECT key, data FROM {}", T::source_table_name()))?;
    let transaction = output.transaction()?;
    let mut count = 0;
    let start = std::time::SystemTime::now();
    {
        if let Some(res) = T::convert_to_sql(&mut istm, &transaction) {
            count = res?;
        } else {
            let mut ostm = transaction.prepare(T::replace_statement())?;
            let mut secondary_ostm = match T::secondary_replace_statement() {
                Some(s) => Some(transaction.prepare(s)?),
                None => None,
            };
            for (uid, res) in istm
                .query_map(NO_PARAMS, |r| {
                    let key: String = r.get(0)?;
                    let value: Vec<u8> = r.get(1)?;
                    Ok((key, value))
                })?
                .enumerate()
            {
                count += 1;
                let (key, value) = res?;
                let value = T::from(value.as_slice());
                value.insert(&key, uid as i32, &mut ostm, secondary_ostm.as_mut())?;
            }
        }
    }
    transaction.commit()?;
    log::info!(
        "Inserted {} {} in {:?}",
        count,
        T::source_table_name(),
        std::time::SystemTime::now().duration_since(start).unwrap()
    );

    Ok(())
}
