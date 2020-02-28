use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TarHeader<'a> {
    pub path: Cow<'a, [u8]>,
    pub size: u64,
    pub entry_type: u8,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum TaskResult<'a> {
    None,
    ExplodedCrate {
        entries_meta_data: Cow<'a, [TarHeader<'a>]>,
        selected_entries: Cow<'a, [(TarHeader<'a>, Cow<'a, [u8]>)]>,
    },
    Download {
        kind: Cow<'a, str>,
        url: Cow<'a, str>,
        content_length: u32,
        content_type: Option<Cow<'a, str>>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let v = TaskResult::Download {
        kind: "kind".into(),
        url: "url".into(),
        content_length: 42,
        content_type: None,
    };
    let ve = rmp_serde::to_vec(&v)?;
    assert_eq!(v, rmp_serde::from_read(ve.as_slice())?);

    let v = TaskResult::Download {
        kind: "kind".into(),
        url: "url".into(),
        content_length: 42,
        content_type: Some("content-type".into()),
    };
    let ve = rmp_serde::to_vec(&v)?;
    assert_eq!(v, rmp_serde::from_read(ve.as_slice())?);

    let v = TaskResult::ExplodedCrate {
        entries_meta_data: Default::default(),
        selected_entries: Default::default(),
    };
    let ve = rmp_serde::to_vec(&v)?;
    assert_eq!(v, rmp_serde::from_read(ve.as_slice())?);
    Ok(())
}
