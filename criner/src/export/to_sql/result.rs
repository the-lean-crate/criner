use crate::export::to_sql::SqlConvert;
use crate::model;
use rusqlite::{params, Statement};

impl SqlConvert for model::TaskResult {
    fn convert_to_sql(
        istm: &mut rusqlite::Statement,
        transaction: &rusqlite::Transaction,
    ) -> Option<crate::Result<usize>> {
        let res = (|| {
            let mut num_downloads = 0;
            let mut num_extract_crates = 0;
            let mut num_crate_entries = 0;
            let mut insert_download = transaction
                .prepare(
                    "
            REPLACE INTO result_download
                     (crate_name, crate_version, version, kind, url, content_length, content_type)
              VALUES (?1        , ?2           , ?3     , ?4  , ?5 , ?6            , ?7);
        ",
                )
                .unwrap();
            let mut insert_extract_crate = transaction
                .prepare(
                    "
            REPLACE INTO result_extract_crate
                     (id, crate_name, crate_version, version, num_crate_entries)
              VALUES (?1, ?2        , ?3           , ?4     , ?5);
        ",
                )
                .unwrap();

            let mut insert_crate_entry = transaction
                .prepare(
                    "
            REPLACE INTO crate_entry
                     (parent_id, path, size, entry_type, data)
              VALUES (?1        , ?2 , ?3  , ?4        , ?5);
        ",
                )
                .unwrap();

            for res in istm.query_map([], |r| {
                let key: String = r.get(0)?;
                let value: Vec<u8> = r.get(1)?;
                Ok((key, value))
            })? {
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
                        insert_download.execute(params![
                            crate_name,
                            crate_version,
                            process_version,
                            kind,
                            url,
                            content_length,
                            content_type
                        ])?;
                        num_downloads += 1;
                    }
                    TaskResult::ExplodedCrate {
                        entries_meta_data,
                        selected_entries,
                    } => {
                        assert_eq!(process, "extract_crate");
                        let id = num_extract_crates as i32;
                        insert_extract_crate.execute(params![
                            id,
                            crate_name,
                            crate_version,
                            process_version,
                            entries_meta_data.len() as i64
                        ])?;
                        for entry in entries_meta_data.iter() {
                            let model::TarHeader { path, size, entry_type } = entry;
                            insert_crate_entry.execute(params![
                                id,
                                std::str::from_utf8(path).expect("utf8 path in crate - lets see how long this is true"),
                                *size as i64,
                                entry_type,
                                rusqlite::types::Null
                            ])?;
                            num_crate_entries += 1;
                        }
                        for (entry, data) in selected_entries.iter() {
                            let model::TarHeader { path, size, entry_type } = entry;
                            insert_crate_entry.execute(params![
                                id,
                                std::str::from_utf8(path).expect("utf8 path in crate - lets see how long this is true"),
                                *size as i64,
                                entry_type,
                                data
                            ])?;
                            num_crate_entries += 1;
                        }
                        num_extract_crates += 1;
                    }
                    TaskResult::None => {}
                };
            }
            Ok(num_downloads + num_extract_crates + num_crate_entries)
        })();
        Some(res)
    }

    fn replace_statement() -> &'static str {
        "will not be called"
    }

    fn source_table_name() -> &'static str {
        "result"
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
        CREATE TABLE result_extract_crate (
            id                              INTEGER UNIQUE NOT NULL,
            crate_name                      TEXT NOT NULL,
            crate_version                   TEXT NOT NULL,
            version                         TEXT NOT NULL, -- version of the process that created the result

            num_crate_entries               INTEGER NOT NULL,
            PRIMARY KEY (crate_name, crate_version, version)
        );
        CREATE TABLE crate_entry (
            parent_id                       INTEGER NOT NULL,
            path                            TEXT NOT NULL,

            size                            INTEGER NOT NULL, -- size in bytes
            entry_type                      INTEGER NOT NULL, -- tar::EntryType
            data                            BLOB, -- optionally with entire content

            PRIMARY KEY (parent_id, path),
            FOREIGN KEY (parent_id) REFERENCES result_extract_crate(id)
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
    ) -> crate::Result<usize> {
        unimplemented!("we implement convert_to_sql instead (having our own loop and unlimited prepared statements")
    }
}
