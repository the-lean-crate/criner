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
    User,
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
        UserKind::User
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
    let t: time::PrimitiveDateTime = time::parse(val, "%F %T").map_err(serde::de::Error::custom)?;
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
