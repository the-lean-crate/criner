use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::time::SystemTime;

type UserId = u32;
pub type Id = u32;
pub type GitHubId = i32;

#[derive(Deserialize, Default, Clone)]
pub struct Keyword {
    pub id: Id,
    #[serde(rename = "keyword")]
    pub name: String,
    // amount of crates using the keyword
    #[serde(rename = "crates_cnt")]
    pub crates_count: u32,
}

#[derive(Deserialize, Default, Clone)]
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
    pub homepage: Option<String>,
    pub readme: Option<String>,
    pub repository: Option<String>,
}

#[derive(Deserialize)]
pub struct CrateDownloads {
    pub crate_id: Id,
    pub downloads: u64,
}

pub enum UserKind {
    User,
    Team,
}

#[derive(Deserialize)]
pub struct User {
    pub id: Id,
    #[serde(rename = "gh_id")]
    pub github_id: GitHubId,
    #[serde(rename = "gh_login")]
    pub github_login: String,
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct Team {
    pub id: Id,
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
    Ok(if val == 0 { UserKind::User } else { UserKind::Team })
}

fn deserialize_json_map<'de, D>(deserializer: D) -> Result<Vec<Feature>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let val = std::borrow::Cow::<'de, str>::deserialize(deserializer)?;
    let val: BTreeMap<String, Vec<String>> = serde_json::from_str(&val).map_err(serde::de::Error::custom)?;
    Ok(val.into_iter().map(|(name, crates)| Feature { name, crates }).collect())
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
    // 2017-11-30 04:00:19.334919+00 (fractional seconds and/or offset may be absent)
    let trimmed = val.find(['.', '+']).map_or(val.as_ref(), |idx| &val[..idx]);
    let t = time::PrimitiveDateTime::parse(
        trimmed,
        //   2015 -04     - 24    18   :  26    :    11
        &time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
    )
    .map_err(serde::de::Error::custom)?;
    Ok(t.assume_offset(time::UtcOffset::UTC).into())
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
pub struct CratesCategory {
    pub category_id: Id,
    pub crate_id: Id,
}

#[derive(Deserialize)]
pub struct CratesKeyword {
    pub keyword_id: Id,
    pub crate_id: Id,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Row {
        #[serde(deserialize_with = "deserialize_timestamp")]
        t: SystemTime,
    }

    fn parse(value: &str) -> SystemTime {
        let csv = format!("t\n{}\n", value);
        let mut rd = csv::ReaderBuilder::new().has_headers(true).from_reader(csv.as_bytes());
        rd.deserialize::<Row>().next().unwrap().unwrap().t
    }

    #[test]
    fn timestamp_with_fractional_seconds_and_offset() {
        parse("2015-10-21 11:48:14.991765+00");
    }

    #[test]
    fn timestamp_with_offset_but_no_fractional_seconds() {
        parse("2017-10-29 06:27:11+00");
    }

    #[test]
    fn timestamp_with_neither_fractional_seconds_nor_offset() {
        parse("2017-10-29 06:27:11");
    }
}
