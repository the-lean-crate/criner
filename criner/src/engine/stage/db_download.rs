use crate::{engine::work, persistence::Db, persistence::TableAccess, Result};
use bytesize::ByteSize;
use futures::FutureExt;
use std::{collections::BTreeMap, fs::File, io::BufReader, path::PathBuf};

mod model {
    use serde_derive::Deserialize;
    use std::collections::BTreeMap;
    use std::time::SystemTime;

    type UserId = u32;
    pub type Id = u32;
    pub type GitHubId = i32;

    #[derive(Deserialize)]
    pub struct Keyword {
        pub id: Id,
        #[serde(rename = "keyword")]
        pub name: String,
        // amount of crates using the keyword
        #[serde(rename = "crates_cnt")]
        pub crates_count: u32,
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

    #[derive(Deserialize)]
    pub struct Crate {
        pub id: Id,
        pub name: String,
        #[serde(deserialize_with = "deserialize_timestamp")]
        pub created_at: SystemTime,
        #[serde(deserialize_with = "deserialize_timestamp")]
        pub updated_at: SystemTime,
        pub description: Option<String>,
        pub documentation: Option<String>,
        pub downloads: u64,
        pub homepage: Option<String>,
        pub readme: Option<String>,
        pub repository: Option<String>,
    }

    pub enum UserKind {
        Individual,
        Team,
    }

    #[derive(Deserialize)]
    pub struct User {
        pub id: Id,
        #[serde(rename = "gh_avatar")]
        pub github_avatar_url: String,
        #[serde(rename = "gh_id")]
        pub github_id: GitHubId,
        #[serde(rename = "gh_login")]
        pub github_login: String,
        pub name: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct Team {
        pub id: Id,
        #[serde(rename = "avatar")]
        pub github_avatar_url: String,
        #[serde(rename = "github_id")]
        pub github_id: GitHubId,
        #[serde(rename = "login")]
        pub github_login: String,
        pub name: Option<String>,
    }

    fn deserialize_owner_kind<'de, D>(deserializer: D) -> Result<UserKind, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        let val = u8::deserialize(deserializer)?;
        Ok(if val == 0 {
            UserKind::Individual
        } else {
            UserKind::Team
        })
    }

    fn deserialize_json_map<'de, D>(deserializer: D) -> Result<Vec<Feature>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        let val = std::borrow::Cow::<'de, str>::deserialize(deserializer)?;
        let val: BTreeMap<String, Vec<String>> =
            serde_json::from_str(&val).map_err(serde::de::Error::custom)?;
        Ok(val
            .into_iter()
            .map(|(name, crates)| Feature { name, crates })
            .collect())
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

    pub struct Feature {
        pub name: String,
        /// The crates the feature depends on
        pub crates: Vec<String>,
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
        #[serde(deserialize_with = "deserialize_json_map")]
        pub features: Vec<Feature>,
        pub license: String,
        #[serde(rename = "num")]
        pub semver: String,
        pub published_by: Option<UserId>,
        #[serde(deserialize_with = "deserialize_yanked", rename = "yanked")]
        pub is_yanked: bool,
    }

    #[derive(Deserialize)]
    pub struct CrateOwner {
        pub crate_id: Id,
        pub created_by: Option<UserId>,
        pub owner_id: UserId,
        #[serde(deserialize_with = "deserialize_owner_kind")]
        pub owner_kind: UserKind,
    }

    #[derive(Deserialize)]
    pub struct VersionAuthor {
        pub name: String,
        pub version_id: Id,
    }

    #[derive(Deserialize)]
    pub struct CratesCategory {
        pub category_id: Id,
        pub crate_id: Id,
    }

    #[derive(Deserialize)]
    pub struct CratesKeyword {
        pub keyword_id: Id,
        pub crate_id: Id,
    }
}

mod from_csv {
    use super::model;
    use std::collections::BTreeMap;

    pub trait AsId {
        fn as_id(&self) -> model::Id;
    }

    macro_rules! impl_as_id {
        ($name:ident) => {
            impl AsId for model::$name {
                fn as_id(&self) -> model::Id {
                    self.id
                }
            }
        };
    }

    impl_as_id!(Keyword);
    impl_as_id!(Version);
    impl_as_id!(Category);
    impl_as_id!(User);
    impl_as_id!(Team);
    impl_as_id!(Crate);

    pub fn records<T>(
        csv: impl std::io::Read,
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
        rd: impl std::io::Read,
        name: &'static str,
        progress: &mut prodash::tree::Item,
    ) -> crate::Result<BTreeMap<model::Id, T>>
    where
        T: serde::de::DeserializeOwned + AsId,
    {
        let mut decode = progress.add_child("decoding");
        decode.init(None, Some(name));
        let mut map = BTreeMap::new();
        records(rd, &mut decode, |v: T| {
            map.insert(v.as_id(), v);
        })?;
        decode.info(format!("Decoded {} {} into memory", map.len(), name));
        Ok(map)
    }

    pub fn vec<T>(
        rd: impl std::io::Read,
        name: &'static str,
        progress: &mut prodash::tree::Item,
    ) -> crate::Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut decode = progress.add_child("decoding");
        decode.init(None, Some(name));
        let mut vec = Vec::new();
        records(rd, &mut decode, |v: T| {
            vec.push(v);
        })?;
        vec.shrink_to_fit();
        decode.info(format!("Decoded {} {} into memory", vec.len(), name));
        Ok(vec)
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

    let mut num_files_seen = 0;
    let mut num_bytes_seen = 0;
    let (
        mut teams,
        mut categories,
        mut versions,
        mut keywords,
        mut users,
        mut crates,
        mut crate_owners,
        mut version_authors,
        mut crates_categories,
        mut crates_keywords,
    ) = (
        None::<BTreeMap<model::Id, model::Team>>,
        None::<BTreeMap<model::Id, model::Category>>,
        None::<BTreeMap<model::Id, model::Version>>,
        None::<BTreeMap<model::Id, model::Keyword>>,
        None::<BTreeMap<model::Id, model::User>>,
        None::<BTreeMap<model::Id, model::Crate>>,
        None::<Vec<model::CrateOwner>>,
        None::<Vec<model::VersionAuthor>>,
        None::<Vec<model::CratesCategory>>,
        None::<Vec<model::CratesKeyword>>,
    );
    for (eid, entry) in archive.entries()?.enumerate() {
        num_files_seen = eid + 1;
        progress.set(eid as u32);

        let entry = entry?;
        let entry_size = entry.header().size()?;
        num_bytes_seen += entry_size;

        if let Some(name) = entry.path().ok().and_then(|p| {
            whitelist_names
                .iter()
                .find(|n| p.ends_with(format!("{}.csv", n)))
        }) {
            let done_msg = format!(
                "extracted '{}' with size {}",
                entry.path()?.display(),
                ByteSize(entry_size)
            );
            match *name {
                "teams" => teams = Some(from_csv::mapping(entry, name, &mut progress)?),
                "categories" => {
                    categories = Some(from_csv::mapping(entry, "categories", &mut progress)?);
                }
                "versions" => {
                    versions = Some(from_csv::mapping(entry, "versions", &mut progress)?);
                }
                "keywords" => {
                    keywords = Some(from_csv::mapping(entry, "keywords", &mut progress)?);
                }
                "users" => {
                    users = Some(from_csv::mapping(entry, "users", &mut progress)?);
                }
                "crates" => {
                    crates = Some(from_csv::mapping(entry, "crates", &mut progress)?);
                }
                "crate_owners" => {
                    crate_owners = Some(from_csv::vec(entry, "crate_owners", &mut progress)?);
                }
                "version_authors" => {
                    version_authors = Some(from_csv::vec(entry, "version_authors", &mut progress)?);
                }
                "crates_categories" => {
                    crates_categories =
                        Some(from_csv::vec(entry, "crates_categories", &mut progress)?);
                }
                "crates_keywords" => {
                    crates_keywords = Some(from_csv::vec(entry, "crates_keywords", &mut progress)?);
                }
                _ => progress.fail(format!(
                    "bug or oversight: Could not parse table of type {:?}",
                    name
                )),
            }
            progress.done(done_msg);
        }
    }
    progress.done(format!(
        "Saw {} files and a total of {}",
        num_files_seen,
        ByteSize(num_bytes_seen)
    ));

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
        drop(tx_io);
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
