use super::csv_model;
use std::collections::BTreeMap;

pub trait AsId {
    fn as_id(&self) -> csv_model::Id;
}

macro_rules! impl_as_id {
    ($name:ident) => {
        impl AsId for csv_model::$name {
            fn as_id(&self) -> csv_model::Id {
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
    for item in rd.deserialize() {
        cb(item?);
        progress.inc();
    }
    Ok(())
}

pub fn mapping<T>(
    rd: impl std::io::Read,
    name: &'static str,
    progress: &mut prodash::tree::Item,
) -> crate::Result<BTreeMap<csv_model::Id, T>>
where
    T: serde::de::DeserializeOwned + AsId,
{
    let mut decode = progress.add_child("decoding");
    decode.init(None, Some(name.into()));
    let mut map = BTreeMap::new();
    records(rd, &mut decode, |v: T| {
        map.insert(v.as_id(), v);
    })?;
    decode.info(format!("Decoded {} {} into memory", map.len(), name));
    Ok(map)
}

pub fn vec<T>(rd: impl std::io::Read, name: &'static str, progress: &mut prodash::tree::Item) -> crate::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let mut decode = progress.add_child("decoding");
    decode.init(None, Some(name.into()));
    let mut vec = Vec::new();
    records(rd, &mut decode, |v: T| {
        vec.push(v);
    })?;
    vec.shrink_to_fit();
    decode.info(format!("Decoded {} {} into memory", vec.len(), name));
    Ok(vec)
}
