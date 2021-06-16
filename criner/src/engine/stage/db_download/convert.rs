use super::csv_model;
use crate::{model::db_dump, utils::parse_semver};
use std::collections::BTreeMap;
use std::time::SystemTime;

lazy_static! {
    static ref PERSON: regex::Regex =
        regex::Regex::new("(?P<name>[\\w ]+)(<(?P<email>.*)>)?").expect("valid statically known regex");
}

impl From<csv_model::User> for db_dump::Actor {
    fn from(
        csv_model::User {
            id,
            github_avatar_url,
            github_id,
            github_login,
            name,
        }: csv_model::User,
    ) -> Self {
        db_dump::Actor {
            crates_io_id: id,
            kind: db_dump::ActorKind::User,
            github_avatar_url,
            github_id,
            github_login,
            name,
        }
    }
}

impl From<csv_model::Team> for db_dump::Actor {
    fn from(
        csv_model::Team {
            id,
            github_avatar_url,
            github_id,
            github_login,
            name,
        }: csv_model::Team,
    ) -> Self {
        db_dump::Actor {
            crates_io_id: id,
            kind: db_dump::ActorKind::Team,
            github_avatar_url,
            github_id,
            github_login,
            name,
        }
    }
}

impl From<csv_model::Version> for db_dump::CrateVersion {
    fn from(
        csv_model::Version {
            id: _,
            crate_id: _,
            crate_size,
            created_at,
            updated_at,
            downloads,
            features,
            license,
            semver,
            published_by: _,
            is_yanked,
        }: csv_model::Version,
    ) -> Self {
        db_dump::CrateVersion {
            crate_size,
            created_at,
            updated_at,
            downloads,
            features: features
                .into_iter()
                .map(|f| db_dump::Feature {
                    name: f.name,
                    crates: f.crates,
                })
                .collect(),
            license,
            semver,
            published_by: None,
            is_yanked,
        }
    }
}

impl From<csv_model::Crate> for db_dump::Crate {
    fn from(
        csv_model::Crate {
            id: _,
            name,
            created_at,
            updated_at,
            description,
            documentation,
            downloads,
            homepage,
            readme,
            repository,
        }: csv_model::Crate,
    ) -> Self {
        db_dump::Crate {
            versions: Vec::new(),
            keywords: Vec::new(),
            categories: Vec::new(),
            owners: Vec::new(),
            stored_at: SystemTime::UNIX_EPOCH,
            created_by: None,
            name,
            created_at,
            updated_at,
            description,
            documentation,
            downloads,
            homepage,
            readme,
            repository,
        }
    }
}

impl From<csv_model::Keyword> for db_dump::Keyword {
    fn from(
        csv_model::Keyword {
            id: _,
            name,
            crates_count,
        }: csv_model::Keyword,
    ) -> Self {
        db_dump::Keyword { name, crates_count }
    }
}

impl From<csv_model::Category> for db_dump::Category {
    fn from(
        csv_model::Category {
            id: _,
            name,
            crates_count,
            description,
            path,
            slug,
        }: csv_model::Category,
    ) -> Self {
        db_dump::Category {
            name,
            crates_count,
            description,
            path,
            slug,
        }
    }
}

impl From<String> for db_dump::Person {
    fn from(v: String) -> Self {
        PERSON
            .captures(&v)
            .map(|cap| db_dump::Person {
                name: cap.name("name").expect("name should always exist").as_str().to_owned(),
                email: cap.name("email").map(|e| e.as_str().to_owned()),
            })
            .unwrap_or_default()
    }
}

impl From<csv_model::UserKind> for db_dump::ActorKind {
    fn from(v: csv_model::UserKind) -> Self {
        match v {
            csv_model::UserKind::User => db_dump::ActorKind::User,
            csv_model::UserKind::Team => db_dump::ActorKind::Team,
        }
    }
}

pub fn into_actors_by_id(
    users: BTreeMap<csv_model::Id, csv_model::User>,
    teams: BTreeMap<csv_model::Id, csv_model::Team>,
    mut progress: prodash::tree::Item,
) -> BTreeMap<(db_dump::Id, db_dump::ActorKind), db_dump::Actor> {
    progress.init(Some(users.len() + teams.len()), Some("users and teams".into()));
    let mut map = BTreeMap::new();

    let mut count = 0;
    for (id, actor) in users.into_iter() {
        count += 1;
        progress.set(count);
        let actor: db_dump::Actor = actor.into();
        map.insert((id, actor.kind), actor);
    }

    for (id, actor) in teams.into_iter() {
        count += 1;
        progress.set(count);
        let actor: db_dump::Actor = actor.into();
        map.insert((id, actor.kind), actor);
    }

    map
}

pub fn into_versions_by_crate_id(
    mut versions: Vec<csv_model::Version>,
    actors: &BTreeMap<(db_dump::Id, db_dump::ActorKind), db_dump::Actor>,
    mut progress: prodash::tree::Item,
) -> BTreeMap<db_dump::Id, Vec<db_dump::CrateVersion>> {
    progress.init(Some(versions.len()), Some("versions converted".into()));
    versions.sort_by_key(|v| v.id);
    let versions_len = versions.len();

    let mut version_by_id = BTreeMap::new();
    for version in versions.into_iter() {
        progress.inc();
        let crate_id = version.crate_id;
        let published_by = version.published_by;
        let version_id = version.id;
        let mut version: db_dump::CrateVersion = version.into();
        version.published_by =
            published_by.and_then(|user_id| actors.get(&(user_id, db_dump::ActorKind::User)).cloned());
        version_by_id.insert(version_id, (crate_id, version));
    }
    progress.done(format!(
        "transformed {} crate versions and assigned publishing actor",
        version_by_id.len()
    ));

    let mut map = BTreeMap::new();
    progress.init(
        Some(version_by_id.len()),
        Some("version-crate associations made".into()),
    );
    for (_, (crate_id, version)) in version_by_id.into_iter() {
        progress.inc();
        map.entry(crate_id).or_insert_with(Vec::new).push(version);
    }
    progress.done(format!(
        "Associated {} crate versions to {} crates",
        versions_len,
        map.len()
    ));

    map
}

#[allow(clippy::too_many_arguments)]
pub fn into_crates(
    crates: Vec<csv_model::Crate>,
    mut keywords_by_id: BTreeMap<csv_model::Id, csv_model::Keyword>,
    crates_keywords: Vec<csv_model::CratesKeyword>,
    mut categories_by_id: BTreeMap<csv_model::Id, csv_model::Category>,
    crates_categories: Vec<csv_model::CratesCategory>,
    actors_by_id: BTreeMap<(db_dump::Id, db_dump::ActorKind), db_dump::Actor>,
    crate_owners: Vec<csv_model::CrateOwner>,
    mut versions_by_crate_id: BTreeMap<db_dump::Id, Vec<db_dump::CrateVersion>>,
    mut progress: prodash::tree::Item,
) -> Vec<db_dump::Crate> {
    let mut crate_by_id = BTreeMap::new();
    progress.init(Some(crates.len()), Some("crates converted".into()));
    for krate in crates.into_iter() {
        progress.inc();
        let crate_id = krate.id;
        let mut krate: db_dump::Crate = krate.into();
        let mut versions: Vec<_> = std::mem::replace(
            &mut versions_by_crate_id
                .get_mut(&crate_id)
                .expect("at least one version per crate"),
            Vec::new(),
        );
        versions.sort_by_key(|v| parse_semver(&v.semver));
        krate.versions = versions;
        crate_by_id.insert(crate_id, krate);
    }
    drop(versions_by_crate_id);
    progress.done(format!(
        "converted {} crates and assigned crate versions",
        crate_by_id.len()
    ));

    progress.init(Some(crates_keywords.len()), Some("crates keywords".into()));
    let crates_keywords_len = crates_keywords.len();
    for csv_model::CratesKeyword { keyword_id, crate_id } in crates_keywords.into_iter() {
        progress.inc();
        crate_by_id
            .get_mut(&crate_id)
            .expect("matching crate for keyword")
            .keywords
            .push(
                keywords_by_id
                    .get_mut(&keyword_id)
                    .expect("keyword for id")
                    .to_owned()
                    .into(),
            )
    }
    progress.done(format!("assigned {} keywords", crates_keywords_len));

    progress.init(Some(crates_categories.len()), Some("crates categories".into()));
    let crates_categories_len = crates_categories.len();
    for csv_model::CratesCategory { category_id, crate_id } in crates_categories.into_iter() {
        progress.inc();
        crate_by_id
            .get_mut(&crate_id)
            .expect("matching crate for category")
            .categories
            .push(
                categories_by_id
                    .get_mut(&category_id)
                    .expect("category for id")
                    .to_owned()
                    .into(),
            )
    }
    progress.done(format!("assigned {} categories", crates_categories_len));

    let crate_owners_len = crate_owners.len();
    progress.init(Some(crate_owners_len), Some("crates owners".into()));
    for csv_model::CrateOwner {
        crate_id,
        created_by,
        owner_id,
        owner_kind,
    } in crate_owners.into_iter()
    {
        progress.inc();
        if let Some(owner) = actors_by_id.get(&(owner_id, owner_kind.into())).map(ToOwned::to_owned) {
            let created_by = created_by.and_then(|id| actors_by_id.get(&(id, db_dump::ActorKind::User)).cloned());
            let krate = crate_by_id
                .get_mut(&crate_id)
                .expect("crate id to match crate for owner assignment");
            if krate.created_by.is_none() {
                krate.created_by = created_by;
            }
            krate.owners.push(owner);
        }
    }

    progress.done(format!("assigned {} owners", crate_owners_len));

    crate_by_id.into_iter().map(|(_, v)| v).collect()
}
