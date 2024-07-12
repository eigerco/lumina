use std::fmt::{self, Display};
use std::future::Future;
use std::mem;
use std::ops::{Deref, RangeInclusive};

use celestia_types::ExtendedHeader;

use crate::block_ranges::{BlockRange, BlockRangeExt};
use crate::executor::yield_now;
use crate::store::Result;

pub(crate) const VALIDATIONS_PER_YIELD: usize = 4;

/// based on the stored headers and current network head height, calculate range of headers that
/// should be fetched from the network, starting from the front up to a `limit` of headers
pub(crate) fn calculate_range_to_fetch(
    head_height: u64,
    store_headers: &[RangeInclusive<u64>],
    syncing_window_edge: Option<u64>,
    limit: u64,
) -> BlockRange {
    let mut missing_range = get_most_recent_missing_range(head_height, store_headers);

    // truncate to syncing window, if height is known
    if let Some(window_edge) = syncing_window_edge {
        if missing_range.start() < &window_edge {
            missing_range = window_edge + 1..=*missing_range.end();
        }
    }

    // truncate number of headers to limit
    if missing_range.len() > limit {
        let end = missing_range.end();
        let start = end.saturating_sub(limit) + 1;
        missing_range = start..=*end;
    }

    missing_range
}

fn get_most_recent_missing_range(
    head_height: u64,
    store_headers: &[RangeInclusive<u64>],
) -> BlockRange {
    let mut store_headers_iter = store_headers.iter().rev();

    let Some(store_head_range) = store_headers_iter.next() else {
        // empty store, we're missing everything
        return 1..=head_height;
    };

    if store_head_range.end() < &head_height {
        // if we haven't caught up with network head, start from there
        return store_head_range.end() + 1..=head_height;
    }

    // there exists a range contiguous with network head. inspect previous range end
    let penultimate_range_end = store_headers_iter.next().map(|r| *r.end()).unwrap_or(0);

    penultimate_range_end + 1..=store_head_range.start().saturating_sub(1)
}

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct ValidatedExtendedHeader(ExtendedHeader);

#[derive(Clone, Debug)]
pub struct ValidatedExtendedHeaders(Vec<ExtendedHeader>);

#[derive(Clone, Debug)]
pub struct VerifiedExtendedHeaders(Vec<ExtendedHeader>);

pub trait IntoVerifiedExtendedHeaders: Send {
    fn into_verified(
        self,
    ) -> impl Future<Output = Result<VerifiedExtendedHeaders, celestia_types::Error>> + Send;
}

impl ValidatedExtendedHeader {
    pub fn new(header: ExtendedHeader) -> celestia_types::Result<Self> {
        header.validate()?;
        Ok(ValidatedExtendedHeader(header))
    }

    /// Create a new instance out of pre-checked vec of headers
    ///
    /// # Safety
    ///
    /// This function may produce invalid `ValidatedExtendedHeader`, if passed range
    /// is not validated manually.
    pub unsafe fn new_unchecked(header: ExtendedHeader) -> Self {
        ValidatedExtendedHeader(header)
    }

    pub fn into_inner(self) -> ExtendedHeader {
        self.0
    }
}

impl ValidatedExtendedHeaders {
    pub async fn new(headers: Vec<ExtendedHeader>) -> celestia_types::Result<Self> {
        validate_headers(&headers).await?;
        Ok(ValidatedExtendedHeaders(headers))
    }

    /// Create a new instance out of pre-checked vec of headers
    ///
    /// # Safety
    ///
    /// This function may produce invalid `ValidatedExtendedHeaders`, if passed range
    /// is not validated manually.
    pub unsafe fn new_unchecked(headers: Vec<ExtendedHeader>) -> Self {
        ValidatedExtendedHeaders(headers)
    }

    pub fn into_inner(self) -> Vec<ExtendedHeader> {
        self.0
    }

    pub fn into_validated_vec(self) -> Vec<ValidatedExtendedHeader> {
        // SAFETY: It is safe to transmute because of `repr(transparent)`.
        unsafe { mem::transmute(self.0) }
    }
}

impl VerifiedExtendedHeaders {
    pub async fn new(headers: Vec<ExtendedHeader>) -> celestia_types::Result<Self> {
        let headers = ValidatedExtendedHeaders::new(headers).await?;
        VerifiedExtendedHeaders::from_validated(headers)
    }

    pub fn from_validated(headers: ValidatedExtendedHeaders) -> celestia_types::Result<Self> {
        verify_headers(&headers.0)?;
        Ok(VerifiedExtendedHeaders(headers.0))
    }

    /// Create a new instance out of pre-checked vec of headers
    ///
    /// # Safety
    ///
    /// This function may produce invalid `VerifiedExtendedHeaders`, if passed range
    /// is not validated and verified manually.
    pub unsafe fn new_unchecked(headers: Vec<ExtendedHeader>) -> Self {
        VerifiedExtendedHeaders(headers)
    }

    pub fn into_inner(self) -> Vec<ExtendedHeader> {
        self.0
    }

    pub fn into_validated_vec(self) -> Vec<ValidatedExtendedHeader> {
        // SAFETY: It is safe to transmute because of `repr(transparent)`.
        unsafe { mem::transmute(self.0) }
    }
}

impl Display for ValidatedExtendedHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromIterator<ValidatedExtendedHeader> for ValidatedExtendedHeaders {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = ValidatedExtendedHeader>,
    {
        let headers = iter.into_iter().map(|h| h.0).collect();
        ValidatedExtendedHeaders(headers)
    }
}

impl IntoIterator for ValidatedExtendedHeaders {
    type Item = ExtendedHeader;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl IntoIterator for VerifiedExtendedHeaders {
    type Item = ExtendedHeader;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Deref for ValidatedExtendedHeader {
    type Target = ExtendedHeader;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for ValidatedExtendedHeaders {
    type Target = [ExtendedHeader];

    fn deref(&self) -> &Self::Target {
        &self.0[..]
    }
}

impl Deref for VerifiedExtendedHeaders {
    type Target = [ExtendedHeader];

    fn deref(&self) -> &Self::Target {
        &self.0[..]
    }
}

impl AsRef<ExtendedHeader> for ValidatedExtendedHeader {
    fn as_ref(&self) -> &ExtendedHeader {
        &self.0
    }
}

impl AsRef<[ExtendedHeader]> for ValidatedExtendedHeaders {
    fn as_ref(&self) -> &[ExtendedHeader] {
        &self.0[..]
    }
}

impl AsRef<[ExtendedHeader]> for VerifiedExtendedHeaders {
    fn as_ref(&self) -> &[ExtendedHeader] {
        &self.0[..]
    }
}

impl From<ValidatedExtendedHeader> for ExtendedHeader {
    fn from(header: ValidatedExtendedHeader) -> Self {
        header.0
    }
}

impl From<ValidatedExtendedHeaders> for Vec<ExtendedHeader> {
    fn from(headers: ValidatedExtendedHeaders) -> Self {
        headers.0
    }
}

impl From<VerifiedExtendedHeaders> for Vec<ExtendedHeader> {
    fn from(headers: VerifiedExtendedHeaders) -> Self {
        headers.0
    }
}

impl From<ValidatedExtendedHeader> for ValidatedExtendedHeaders {
    fn from(header: ValidatedExtendedHeader) -> Self {
        ValidatedExtendedHeaders(vec![header.0])
    }
}

impl<'a> From<&'a ValidatedExtendedHeader> for ValidatedExtendedHeaders {
    fn from(header: &'a ValidatedExtendedHeader) -> Self {
        header.to_owned().into()
    }
}

impl<'a> From<&'a mut ValidatedExtendedHeader> for ValidatedExtendedHeaders {
    fn from(header: &'a mut ValidatedExtendedHeader) -> Self {
        header.to_owned().into()
    }
}

impl From<Vec<ValidatedExtendedHeader>> for ValidatedExtendedHeaders {
    fn from(headers: Vec<ValidatedExtendedHeader>) -> Self {
        // SAFETY: It is safe to transmute because of `repr(transparent)`.
        let headers = unsafe { mem::transmute(headers) };
        ValidatedExtendedHeaders(headers)
    }
}

impl<'a> From<&'a [ValidatedExtendedHeader]> for ValidatedExtendedHeaders {
    fn from(headers: &'a [ValidatedExtendedHeader]) -> Self {
        Vec::from(headers).into()
    }
}

impl<'a> From<&'a mut [ValidatedExtendedHeader]> for ValidatedExtendedHeaders {
    fn from(headers: &'a mut [ValidatedExtendedHeader]) -> Self {
        Vec::from(headers).into()
    }
}

impl<'a, const N: usize> From<[ValidatedExtendedHeader; N]> for ValidatedExtendedHeaders {
    fn from(headers: [ValidatedExtendedHeader; N]) -> Self {
        Vec::from(headers).into()
    }
}

impl<'a, const N: usize> From<&'a [ValidatedExtendedHeader; N]> for ValidatedExtendedHeaders {
    fn from(headers: &'a [ValidatedExtendedHeader; N]) -> Self {
        Vec::from(headers).into()
    }
}

impl<'a, const N: usize> From<&'a mut [ValidatedExtendedHeader; N]> for ValidatedExtendedHeaders {
    fn from(headers: &'a mut [ValidatedExtendedHeader; N]) -> Self {
        Vec::from(headers).into()
    }
}

/*
impl IntoIterator for ValidExtendedHeadersChain {
    type Item = &ExtendedHeader;
    type IntoIter<'a> = std::slice::Iter<'a, ExtendedHeader>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl IntoIterator for VerifiedExtendedHeaders {
    type Item = ExtendedHeader;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl AsRef<[ExtendedHeader]> for ValidExtendedHeadersChain {
    fn as_ref(&self) -> &[ExtendedHeader] {
        unsafe { mem::transmute(self.0.as_ref()) }
    }
}
*/

impl IntoVerifiedExtendedHeaders for VerifiedExtendedHeaders {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        Ok(self)
    }
}

impl IntoVerifiedExtendedHeaders for ValidatedExtendedHeader {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        // A single validated header has no neighbors so there nothing to verify.
        Ok(VerifiedExtendedHeaders(vec![self.0]))
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a ValidatedExtendedHeader {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        // A single validated header has no neighbors so there nothing to verify.
        Ok(VerifiedExtendedHeaders(vec![self.0.to_owned()]))
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a mut ValidatedExtendedHeader {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        // A single validated header has no neighbors so there nothing to verify.
        Ok(VerifiedExtendedHeaders(vec![self.0.to_owned()]))
    }
}

impl IntoVerifiedExtendedHeaders for ValidatedExtendedHeaders {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        VerifiedExtendedHeaders::from_validated(self)
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a ValidatedExtendedHeaders {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        verify_headers(&self.0[..])?;
        Ok(VerifiedExtendedHeaders(self.0.to_owned()))
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a mut ValidatedExtendedHeaders {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        verify_headers(&self.0[..])?;
        Ok(VerifiedExtendedHeaders(self.0.to_owned()))
    }
}

impl IntoVerifiedExtendedHeaders for ExtendedHeader {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        self.validate()?;
        Ok(VerifiedExtendedHeaders(vec![self]))
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a ExtendedHeader {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        self.validate()?;
        Ok(VerifiedExtendedHeaders(vec![self.to_owned()]))
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a mut ExtendedHeader {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        self.validate()?;
        Ok(VerifiedExtendedHeaders(vec![self.to_owned()]))
    }
}

impl IntoVerifiedExtendedHeaders for Vec<ExtendedHeader> {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        VerifiedExtendedHeaders::new(self).await
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a [ExtendedHeader] {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        validate_headers(self).await?;
        verify_headers(self)?;
        Ok(VerifiedExtendedHeaders(Vec::from(self)))
    }
}

impl<'a> IntoVerifiedExtendedHeaders for &'a mut [ExtendedHeader] {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        validate_headers(self).await?;
        verify_headers(self)?;
        Ok(VerifiedExtendedHeaders(Vec::from(self)))
    }
}

impl<const N: usize> IntoVerifiedExtendedHeaders for [ExtendedHeader; N] {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        validate_headers(&self[..]).await?;
        verify_headers(&self[..])?;
        Ok(VerifiedExtendedHeaders(Vec::from(self)))
    }
}

impl<'a, const N: usize> IntoVerifiedExtendedHeaders for &'a [ExtendedHeader; N] {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        validate_headers(&self[..]).await?;
        verify_headers(&self[..])?;
        Ok(VerifiedExtendedHeaders(Vec::from(self)))
    }
}

impl<'a, const N: usize> IntoVerifiedExtendedHeaders for &'a mut [ExtendedHeader; N] {
    async fn into_verified(self) -> celestia_types::Result<VerifiedExtendedHeaders> {
        validate_headers(&self[..]).await?;
        verify_headers(&self[..])?;
        Ok(VerifiedExtendedHeaders(Vec::from(self)))
    }
}

pub(crate) fn verify_headers(headers: &[ExtendedHeader]) -> celestia_types::Result<()> {
    let Some(head) = headers.first() else {
        return Ok(());
    };

    head.verify_adjacent_range(&headers[1..])
}

pub(crate) async fn validate_headers(headers: &[ExtendedHeader]) -> celestia_types::Result<()> {
    for headers in headers.chunks(VALIDATIONS_PER_YIELD) {
        for header in headers {
            header.validate()?;
        }

        // Validation is computation heavy so we yield on every chunk
        yield_now().await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_range_to_fetch_test_header_limit() {
        let head_height = 1024;
        let ranges = [256..=512];

        let fetch_range = calculate_range_to_fetch(head_height, &ranges, None, 16);
        assert_eq!(fetch_range, 1009..=1024);

        let fetch_range = calculate_range_to_fetch(head_height, &ranges, None, 511);
        assert_eq!(fetch_range, 514..=1024);
        let fetch_range = calculate_range_to_fetch(head_height, &ranges, None, 512);
        assert_eq!(fetch_range, 513..=1024);
        let fetch_range = calculate_range_to_fetch(head_height, &ranges, None, 513);
        assert_eq!(fetch_range, 513..=1024);

        let fetch_range = calculate_range_to_fetch(head_height, &ranges, None, 1024);
        assert_eq!(fetch_range, 513..=1024);
        let fetch_range = calculate_range_to_fetch(head_height, &ranges, Some(900), 1024);
        assert_eq!(fetch_range, 901..=1024);
    }

    #[test]
    fn calculate_range_to_fetch_empty_store() {
        let fetch_range = calculate_range_to_fetch(1, &[], None, 100);
        assert_eq!(fetch_range, 1..=1);

        let fetch_range = calculate_range_to_fetch(100, &[], None, 10);
        assert_eq!(fetch_range, 91..=100);

        let fetch_range = calculate_range_to_fetch(100, &[], Some(75), 50);
        assert_eq!(fetch_range, 76..=100);
    }

    #[test]
    fn calculate_range_to_fetch_fully_synced() {
        let fetch_range = calculate_range_to_fetch(1, &[1..=1], None, 100);
        assert!(fetch_range.is_empty());

        let fetch_range = calculate_range_to_fetch(100, &[1..=100], None, 10);
        assert!(fetch_range.is_empty());

        let fetch_range = calculate_range_to_fetch(100, &[1..=100], Some(100), 10);
        assert!(fetch_range.is_empty());
    }

    #[test]
    fn calculate_range_to_fetch_caught_up() {
        let head_height = 4000;

        let fetch_range = calculate_range_to_fetch(head_height, &[3000..=4000], None, 500);
        assert_eq!(fetch_range, 2500..=2999);
        let fetch_range = calculate_range_to_fetch(head_height, &[3000..=4000], Some(2600), 500);
        assert_eq!(fetch_range, 2601..=2999);
        let fetch_range =
            calculate_range_to_fetch(head_height, &[500..=1000, 3000..=4000], None, 500);
        assert_eq!(fetch_range, 2500..=2999);
        let fetch_range =
            calculate_range_to_fetch(head_height, &[2500..=2800, 3000..=4000], None, 500);
        assert_eq!(fetch_range, 2801..=2999);
        let fetch_range =
            calculate_range_to_fetch(head_height, &[2500..=2800, 3000..=4000], Some(2000), 500);
        assert_eq!(fetch_range, 2801..=2999);
        let fetch_range = calculate_range_to_fetch(head_height, &[300..=4000], None, 500);
        assert_eq!(fetch_range, 1..=299);
        let fetch_range = calculate_range_to_fetch(head_height, &[300..=4000], Some(2000), 500);
        assert!(fetch_range.is_empty());
    }

    #[test]
    fn calculate_range_to_fetch_catching_up() {
        let head_height = 4000;

        let fetch_range = calculate_range_to_fetch(head_height, &[2000..=3000], None, 500);
        assert_eq!(fetch_range, 3501..=4000);
        let fetch_range = calculate_range_to_fetch(head_height, &[2000..=3000], Some(3600), 500);
        assert_eq!(fetch_range, 3601..=4000);
        let fetch_range =
            calculate_range_to_fetch(head_height, &[1..=2998, 3000..=3800], None, 500);
        assert_eq!(fetch_range, 3801..=4000);
        let fetch_range =
            calculate_range_to_fetch(head_height, &[1..=2998, 3000..=3800], Some(3900), 500);
        assert_eq!(fetch_range, 3901..=4000);
    }
}
