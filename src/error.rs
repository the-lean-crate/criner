use std::{error::Error, fmt, process};

struct WithCauses<'a>(&'a dyn Error);

impl<'a> fmt::Display for WithCauses<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ERROR: {}", self.0)?;
        let mut cursor = self.0;
        while let Some(err) = cursor.source() {
            write!(f, "\ncaused by: \n{}", err)?;
            cursor = err;
        }
        write!(f, "\n")
    }
}

pub fn ok_or_exit<T, E>(result: Result<T, E>) -> T
where
    E: Error,
{
    match result {
        Ok(v) => v,
        Err(err) => {
            println!("{}", WithCauses(&err));
            process::exit(2);
        }
    }
}
