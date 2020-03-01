use crate::model::{Context, Crate, CrateVersion, ReportResult, Task, TaskResult};

fn expect<T, E: std::fmt::Display>(
    r: std::result::Result<T, E>,
    panic_message: impl FnOnce(E) -> String,
) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!(panic_message(e)),
    }
}

macro_rules! impl_ivec_transform {
    ($ty:ty) => {
        impl From<&[u8]> for $ty {
            fn from(b: &[u8]) -> Self {
                expect(rmp_serde::from_read_ref(b), |e| {
                    format!(
                        concat!(
                            "&[u8]: migration should succeed: ",
                            stringify!($ty),
                            "{:#?}: {}"
                        ),
                        rmpv::decode::value::read_value(&mut std::io::Cursor::new(b)).unwrap(),
                        e
                    )
                })
            }
        }
        impl From<sled::IVec> for $ty {
            fn from(b: sled::IVec) -> Self {
                expect(rmp_serde::from_read_ref(b.as_ref()), |e| {
                    format!(
                        concat!(
                            "IVec: migration should succeed: ",
                            stringify!($ty),
                            "{:#?}: {}"
                        ),
                        rmpv::decode::value::read_value(&mut std::io::Cursor::new(b)).unwrap(),
                        e
                    )
                })
            }
        }
        impl From<$ty> for sled::IVec {
            fn from(c: $ty) -> Self {
                rmp_serde::to_vec(&c)
                    .expect("serialization to always succeed")
                    .into()
            }
        }
    };
}

impl_ivec_transform!(Crate<'_>);
impl_ivec_transform!(Task<'_>);
impl_ivec_transform!(TaskResult<'_>);
impl_ivec_transform!(CrateVersion<'_>);
impl_ivec_transform!(Context);
impl_ivec_transform!(ReportResult);
