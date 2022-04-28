use crate::model::{db_dump, Context, Crate, CrateVersion, ReportResult, Task, TaskResult};

fn expect<T, E: std::fmt::Display>(r: std::result::Result<T, E>, panic_message: impl FnOnce(E) -> String) -> T {
    match r {
        Ok(v) => v,
        Err(e) => std::panic::panic_any(panic_message(e)),
    }
}

macro_rules! impl_deserialize {
    ($ty:ty) => {
        impl From<&[u8]> for $ty {
            fn from(b: &[u8]) -> Self {
                expect(rmp_serde::from_slice(b), |e| {
                    format!(
                        concat!("&[u8]: migration should succeed: ", stringify!($ty), "{:#?}: {}"),
                        rmpv::decode::value::read_value(&mut std::io::Cursor::new(b)).unwrap(),
                        e
                    )
                })
            }
        }
    };
}

impl_deserialize!(Crate);
impl_deserialize!(Task);
impl_deserialize!(TaskResult);
impl_deserialize!(CrateVersion);
impl_deserialize!(Context);
impl_deserialize!(ReportResult);
impl_deserialize!(db_dump::Crate);
