use crate::export::to_sql::SqlConvert;
use crate::model;
use rusqlite::{params, Statement, NO_PARAMS};

impl<'a> SqlConvert for model::TaskResult<'a> {
    fn convert_to_sql(
        istm: &mut rusqlite::Statement,
        transaction: &rusqlite::Transaction,
    ) -> Option<crate::error::Result<usize>> {
        let res = (|| {
            let mut num_insertions = 0;
            let mut odlst = transaction
                .prepare(
                    "
            REPLACE INTO result_download
                     (crate_name, crate_version, version, kind, url, content_length, content_type)
              VALUES (?1        , ?2           , ?3     , ?4  , ?5 , ?6            , ?7);
        ",
                )
                .unwrap();

            for (_uid, res) in istm
                .query_map(NO_PARAMS, |r| {
                    let key: String = r.get(0)?;
                    let value: Vec<u8> = r.get(1)?;
                    Ok((key, value))
                })?
                .enumerate()
            {
                let (key, value) = res?;
                let mut tokens = key.split(crate::persistence::KEY_SEP_CHAR);
                let crate_name = tokens.next().unwrap();
                let crate_version = tokens.next().unwrap();
                let process = tokens.next().unwrap();
                let process_version = tokens.next().unwrap();
                let optional_last_key = tokens.next();
                assert!(tokens.next().is_none());

                let value = Self::from(value.as_slice());

                use model::TaskResult;
                match value {
                    TaskResult::Download {
                        kind,
                        url,
                        content_length,
                        content_type,
                    } => {
                        assert_eq!(process, "download");
                        assert_eq!(Some(kind.as_ref()), optional_last_key);
                        odlst.execute(params![
                            crate_name,
                            crate_version,
                            process_version,
                            kind,
                            url,
                            content_length,
                            content_type
                        ])?;
                    }
                    TaskResult::ExplodedCrate {
                        entries_meta_data,
                        selected_entries,
                    } => {
                        assert_eq!(process, "extract_crate");
                    }
                    TaskResult::None => {}
                };
                num_insertions += 1;
            }
            Ok(num_insertions)
        })();
        Some(res)
    }

    fn replace_statement() -> &'static str {
        "will not be called"
    }

    fn source_table_name() -> &'static str {
        "results"
    }

    fn init_table_statement() -> &'static str {
        "
        BEGIN;
        CREATE TABLE result_download (
            crate_name                      TEXT NOT NULL,
            crate_version                   TEXT NOT NULL,
            version                         TEXT NOT NULL, -- version of the process that created the result
            kind                            TEXT NOT NULL,

            url                             TEXT NOT NULL,
            content_length                  INTEGER NOT NULL,
            content_type                    TEXT,
            PRIMARY KEY (crate_name, crate_version, version, kind)
        );
        COMMIT;
        "
    }

    fn insert(
        &self,
        _key: &str,
        _uid: i32,
        _stm: &mut Statement<'_>,
        _sstm: Option<&mut Statement<'_>>,
    ) -> crate::error::Result<usize> {
        unimplemented!("we implement convert_to_sql instead (having our own loop and unlimited prepared statements")
    }
}
