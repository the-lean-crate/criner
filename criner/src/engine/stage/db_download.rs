use crate::{engine::work, persistence::Db, persistence::TableAccess, Result};
use bytesize::ByteSize;
use futures::FutureExt;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
};

mod model {
    use serde_derive::Deserialize;
    use std::time::SystemTime;

    type UserId = u32;
    pub type Id = u32;

    pub struct Keyword<'a> {
        name: &'a str,
        // amount of crates using the keyword
        crates_count: u32,
    }

    #[derive(Deserialize)]
    pub struct Category {
        pub id: Id,
        #[serde(rename = "category")]
        pub name: String,
        #[serde(rename = "crates_cnt")]
        pub crates_count: u32,
        pub description: String,
        pub path: String,
        pub slug: String,
    }

    pub struct Crate<'a> {
        name: &'a str,
        created_at: SystemTime,
        updated_at: SystemTime,
        description: Option<&'a str>,
        documentation: Option<&'a str>,
        downloads: u64,
        homepage: Option<&'a str>,
        readme: Option<&'a str>,
        repository: Option<&'a str>,
        created_by: UserId,
        keywords: Vec<Keyword<'a>>,
        categories: Vec<Category>,
        owner: UserId,
    }

    pub enum UserKind {
        Individual,
        Team,
    }

    pub struct User<'a> {
        github_avatar_url: &'a str,
        github_id: u32,
        github_login: &'a str,
        crates_io_id: UserId,
        name: Option<&'a str>,
        kind: UserKind,
    }

    #[derive(Deserialize)]
    pub struct Feature {
        name: String,
        dependencies: Vec<String>,
    }

    fn deserialize_json<'de, D>(deserializer: D) -> Result<Vec<Feature>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        Vec::deserialize(deserializer)
    }

    fn deserialize_yanked<'de, D>(deserializer: D) -> Result<bool, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        let val = std::borrow::Cow::<'de, str>::deserialize(deserializer)?;
        Ok(val == "t")
    }

    fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        let val = std::borrow::Cow::<'de, str>::deserialize(deserializer)?;
        // 2017-11-30 04:00:19.334919
        let t: time::PrimitiveDateTime =
            time::parse(val, "%F %T").map_err(serde::de::Error::custom)?;
        Ok(t.into())
    }

    #[derive(Deserialize)]
    pub struct Version {
        pub id: Id,
        pub crate_id: Id,
        pub crate_size: Option<u32>,
        #[serde(deserialize_with = "deserialize_timestamp")]
        pub created_at: SystemTime,
        #[serde(deserialize_with = "deserialize_timestamp")]
        pub updated_at: SystemTime,
        pub downloads: u32,
        #[serde(deserialize_with = "deserialize_json")]
        pub features: Vec<Feature>,
        pub license: String,
        #[serde(rename = "num")]
        pub semver: String,
        pub published_by: Option<UserId>,
        #[serde(deserialize_with = "deserialize_yanked")]
        pub is_yanked: bool,
    }
}

mod from_csv {
    use super::model;
    use std::collections::BTreeMap;

    pub trait AsId {
        fn as_id(&self) -> model::Id;
    }

    impl AsId for model::Category {
        fn as_id(&self) -> model::Id {
            self.id
        }
    }
    impl AsId for model::Version {
        fn as_id(&self) -> model::Id {
            self.id
        }
    }

    pub fn records<T>(
        csv: &[u8],
        progress: &mut prodash::tree::Item,
        mut cb: impl FnMut(T),
    ) -> crate::Result<()>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut rd = csv::ReaderBuilder::new()
            .delimiter(b',')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv);
        for (idx, item) in rd.deserialize().enumerate() {
            cb(item?);
            progress.set((idx + 1) as u32);
        }
        Ok(())
    }

    pub fn mapping<T>(
        csv_map: &mut BTreeMap<&&str, Vec<u8>>,
        name: &'static str,
        progress: &mut prodash::tree::Item,
    ) -> crate::Result<BTreeMap<model::Id, T>>
    where
        T: serde::de::DeserializeOwned + AsId,
    {
        let mut decode = progress.add_child("decoding");
        decode.init(None, Some(name));
        let mut map = BTreeMap::new();
        records(&csv_map[&name], &mut decode, |v: T| {
            map.insert(v.as_id(), v);
        })?;
        csv_map.remove(&name);
        Ok(map)
    }
}

fn extract_and_ingest(
    _db: Db,
    mut progress: prodash::tree::Item,
    db_file_path: PathBuf,
) -> crate::Result<()> {
    progress.init(None, Some("csv files"));
    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(File::open(
        db_file_path,
    )?))?);
    let whitelist_names = [
        "crates",
        "crate_owners",
        "versions",
        "version_authors",
        "crates_categories",
        "categories",
        "crates_keywords",
        "keywords",
        "users",
        "teams",
    ];

    let mut csv_map = BTreeMap::new();
    let mut num_files_seen = 0;
    let mut num_bytes_seen = 0;
    for (eid, entry) in archive.entries()?.enumerate() {
        num_files_seen = eid + 1;
        progress.set(eid as u32);

        let mut entry = entry?;
        let entry_size = entry.header().size()?;
        num_bytes_seen += entry_size;

        if let Some(name) = entry.path().ok().and_then(|p| {
            whitelist_names
                .iter()
                .find(|n| p.ends_with(format!("{}.csv", n)))
        }) {
            let mut buf = Vec::with_capacity(entry_size as usize);
            entry.read_to_end(&mut buf)?;
            csv_map.insert(name, buf);

            progress.done(format!(
                "extracted '{}' with size {} into memory",
                entry.path()?.display(),
                ByteSize(entry_size)
            ))
        }
    }
    progress.done(format!(
        "Saw {} files and a total of {}",
        num_files_seen,
        ByteSize(num_bytes_seen)
    ));

    let categories =
        from_csv::mapping::<model::Category>(&mut csv_map, "categories", &mut progress)?;
    let versions = from_csv::mapping::<model::Version>(&mut csv_map, "versions", &mut progress)?;
    Ok(())
}

pub async fn trigger(
    db: Db,
    assets_dir: PathBuf,
    mut progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    startup_time: std::time::SystemTime,
) -> Result<()> {
    let (tx_result, rx_result) = async_std::sync::channel(1);
    let tx_io = {
        let (tx_io, rx) = async_std::sync::channel(1);
        let max_retries_on_timeout = 80;
        tokio.spawn(
            work::generic::processor(
                db.clone(),
                progress.add_child("↓ IDLE"),
                rx,
                work::iobound::Agent::new(&db, tx_result, {
                    move |_, _, output_file_path| Some(output_file_path.to_path_buf())
                })?,
                max_retries_on_timeout,
            )
            .map(|r| {
                if let Err(e) = r {
                    log::warn!("db download: iobound processor failed: {}", e);
                }
            }),
        );
        tx_io
    };

    let today_yyyy_mm_dd = time::OffsetDateTime::now_local().format("%F");
    let task_key = format!(
        "{}{}{}",
        "crates-io-db-dump",
        crate::persistence::KEY_SEP_CHAR,
        today_yyyy_mm_dd
    );

    let tasks = db.open_tasks()?;
    if tasks
        .get(&task_key)?
        .map(|t| t.can_be_started(startup_time) || t.state.is_complete()) // always allow the extractor to run - must be idempotent
        .unwrap_or(true)
    {
        let db_file_path = assets_dir
            .join("crates-io-db")
            .join(format!("{}-crates-io-db-dump.tar.gz", today_yyyy_mm_dd));
        tx_io
            .send(work::iobound::DownloadRequest {
                output_file_path: db_file_path,
                progress_name: "db dump".to_string(),
                task_key,
                crate_name_and_version: None,
                kind: "tar.gz",
                url: "https://static.crates.io/db-dump.tar.gz".to_string(),
            })
            .await;

        if let Some(db_file_path) = rx_result.recv().await {
            extract_and_ingest(db, progress.add_child("ingest"), db_file_path).map_err(|err| {
                progress.fail(format!("ingestion failed: {}", err));
                err
            })?;
        }
    }

    // TODO: cleanup old db dumps

    Ok(())
}
