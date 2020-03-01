use crate::export::to_sql::SqlConvert;
use crate::model;
use rusqlite::{params, Statement};

impl SqlConvert for model::Context {
    fn replace_statement() -> &'static str {
        "INSERT INTO runtime_statistic
                (sample_day, num_new_crate_versions, num_new_crates, dur_s_fetch_new_crate_versions)
         VALUES (?1        , ?2                    , ?3            , ?4);
        "
    }

    fn source_table_name() -> &'static str {
        "meta"
    }

    fn init_table_statement() -> &'static str {
        "CREATE TABLE runtime_statistic (
            sample_day                      TIMESTAMP NOT NULL,
            num_new_crate_versions          INTEGER NOT NULL,
            num_new_crates                  INTEGER NOT NULL,
            dur_s_fetch_new_crate_versions  INTEGER NOT NULL,
            PRIMARY KEY (sample_day)
        );
        "
    }

    fn insert(
        &self,
        key: &str,
        _uid: i32,
        stm: &mut Statement<'_>,
        _sstm: Option<&mut Statement<'_>>,
    ) -> crate::Result<usize> {
        let mut tokens = key.split('/').skip(1);
        let day_date = tokens.next().unwrap();
        assert!(tokens.next().is_none());
        assert_eq!(day_date.len(), 10);
        let day_date = humantime::parse_rfc3339(&format!("{}T00:00:00Z", day_date)).unwrap();
        let date_stamp = day_date.duration_since(std::time::UNIX_EPOCH).unwrap();

        let model::Context {
            counts:
                model::Counts {
                    crate_versions,
                    crates,
                },
            durations: model::Durations {
                fetch_crate_versions,
            },
        } = self;

        stm.execute(params![
            date_stamp.as_secs() as i64,
            *crate_versions as i64,
            *crates as i64,
            fetch_crate_versions.as_secs() as i64
        ])
        .map_err(Into::into)
    }
}
