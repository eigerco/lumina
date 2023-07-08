use celestia_rpc::prelude::*;
use celestia_types::consts::appconsts::{
    CONTINUATION_SPARSE_SHARE_CONTENT_SIZE, FIRST_SPARSE_SHARE_CONTENT_SIZE, SEQUENCE_LEN_BYTES,
    SHARE_INFO_BYTES,
};
use celestia_types::nmt::Namespace;
use celestia_types::Blob;

pub mod utils;

use crate::utils::client::{blob_submit, new_test_client, AuthLevel};
use crate::utils::{random_bytes, random_ns, random_ns_range};

#[tokio::test]
async fn get_shares_by_namespace() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let dah = client
        .header_get_by_height(submitted_height)
        .await
        .unwrap()
        .dah;

    let ns_shares = client
        .share_get_shares_by_namespace(&dah, namespace)
        .await
        .unwrap();

    let seq_len =
        &ns_shares.rows[0].shares[0].data[SHARE_INFO_BYTES..SHARE_INFO_BYTES + SEQUENCE_LEN_BYTES];
    let seq_len = u32::from_be_bytes(seq_len.try_into().unwrap());
    assert_eq!(seq_len as usize, data.len());

    let reconstructed_data = ns_shares
        .rows
        .into_iter()
        .flat_map(|row| row.shares.into_iter())
        .fold(vec![], |mut acc, share| {
            let data = if acc.is_empty() {
                &share.data[share.data.len() - FIRST_SPARSE_SHARE_CONTENT_SIZE..]
            } else {
                &share.data[share.data.len() - CONTINUATION_SPARSE_SHARE_CONTENT_SIZE..]
            };
            acc.extend_from_slice(data);
            acc
        });

    assert_eq!(&reconstructed_data[..seq_len as usize], &data[..]);
}

#[tokio::test]
async fn get_shares_by_namespace_wrong_ns() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let dah = client
        .header_get_by_height(submitted_height)
        .await
        .unwrap()
        .dah;

    let root_hash = dah.row_root(0).unwrap();
    let min_ns = root_hash.min_namespace().into();
    let max_ns = root_hash.max_namespace().into();

    // When we try to get shares for the unknown namespace then
    // if there exists a row where row_root.min_namespace() < namespace < row_root.max_namespace()
    // then we will get an absence proof for that row
    // The rows where namespace falls outside of the ns range of the row_root
    // are not included in the response.
    // As the block has just a single blob, we can only care about the first row.

    // check the case where we receive absence proof
    let random_ns = random_ns_range(min_ns, max_ns);
    let ns_shares = client
        .share_get_shares_by_namespace(&dah, random_ns)
        .await
        .unwrap();
    assert_eq!(ns_shares.rows.len(), 1);
    assert!(ns_shares.rows[0].shares.is_empty());

    let proof = &ns_shares.rows[0].proof;
    assert!(proof.is_of_absence());

    let no_leaves: &[&[u8]] = &[];

    proof
        .verify_complete_namespace(&root_hash, no_leaves, random_ns.into())
        .unwrap();
}

#[tokio::test]
async fn get_shares_by_namespace_wrong_ns_out_of_range() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let dah = client
        .header_get_by_height(submitted_height)
        .await
        .unwrap()
        .dah;

    let root_hash = dah.row_root(0).unwrap();
    let min_ns = root_hash.min_namespace().into();

    // check the case where namespace is outside of the root hash range
    let zero = Namespace::const_v0([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let random_ns = random_ns_range(zero, min_ns);
    let ns_shares = client
        .share_get_shares_by_namespace(&dah, random_ns)
        .await
        .unwrap();

    assert!(ns_shares.rows.is_empty());
    assert!(!root_hash.contains(random_ns.into()));
}

#[tokio::test]
async fn get_shares_by_namespace_wrong_roots() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024);
    let blob = Blob::new(namespace, data.clone()).unwrap();

    blob_submit(&client, &[blob]).await.unwrap();

    let genesis_dah = client.header_get_by_height(1).await.unwrap().dah;

    let ns_shares = client
        .share_get_shares_by_namespace(&genesis_dah, namespace)
        .await
        .unwrap();

    assert!(ns_shares.rows.is_empty());
}

#[tokio::test]
async fn get_eds() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = vec![1, 2, 3, 4];
    let blob = Blob::new(namespace, data.clone()).unwrap();

    let submitted_height = blob_submit(&client, &[blob]).await.unwrap();

    let dah = client
        .header_get_by_height(submitted_height)
        .await
        .unwrap()
        .dah;

    let _eds = client.share_get_eds(&dah).await.unwrap();

    // TODO: validate
}

#[tokio::test]
async fn probability_of_availability() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    client.share_probability_of_availability().await.unwrap();
    // TODO: any way to validate it?
}
