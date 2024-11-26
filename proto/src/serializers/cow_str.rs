use std::borrow::Cow;
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::Deref;

use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct CowStr<'a>(#[serde(borrow)] Cow<'a, str>);

impl<'a> Debug for CowStr<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        <&str as Debug>::fmt(&&*self.0, f)
    }
}

impl<'a> Display for CowStr<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        <&str as Display>::fmt(&&*self.0, f)
    }
}

impl<'a> Deref for CowStr<'a> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> AsRef<str> for CowStr<'a> {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'a> AsRef<[u8]> for CowStr<'a> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}
