
<a name="readmemd"></a>

**lumina-node-wasm**

***

# Lumina node wasm

A compatibility layer for the [`Lumina`](https://github.com/eigerco/lumina) node to
work within a browser environment and be operable with javascript.

# Example
Starting lumina inside a dedicated worker

```javascript
import { spawnNode, NodeConfig, Network } from "lumina-node";

const node = await spawnNode();
const mainnetConfig = NodeConfig.default(Network.Mainnet);

await node.start(mainnetConfig);

await node.waitConnected();
await node.requestHeadHeader();
```

## Manual setup

Note that `spawnNode` implicitly calls wasm initialisation code. If you want to set things up manually, make sure to call the default export before using any of the wasm functionality.

```javascript
import init, { NodeConfig, Network } from "lumina-node";

await init();
const config = NodeConfig.default(Network.Mainnet);

// client and worker accept any object with MessagePort like interface e.g. Worker
const channel = new MessageChannel();
const worker = new NodeWorker(channel.port1);

// note that this runs lumina in the current context (and doesn't create a new web-worker). Promise created with `.run()` never completes.
const worker_promise = worker.run();

// client port can be used locally or transferred like any plain MessagePort
const client = await new NodeClient(channel.port2);
await client.waitConnected();
await client.requestHeadHeader();
```

## Rust API

For comprehensive and fully typed interface documentation, see [lumina-node](https://docs.rs/lumina-node/latest/lumina_node/) and [celestia-types](https://docs.rs/celestia-types/latest/celestia_types/) documentation on docs.rs. You can see there the exact structure of more complex types, such as [`ExtendedHeader`](https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html). JavaScript API's goal is to provide similar interface to Rust when possible, e.g. `NodeClient` mirrors [`Node`](https://docs.rs/lumina-node/latest/lumina_node/node/struct.Node.html).

# Classes


<a name="classesappversionmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AppVersion

## Class: AppVersion

Version of the App

### Properties

#### V1

> `readonly` `static` **V1**: [`AppVersion`](#classesappversionmd)

App v1

##### Defined in

lumina\_node\_wasm.d.ts:154

***

#### V2

> `readonly` `static` **V2**: [`AppVersion`](#classesappversionmd)

App v2

##### Defined in

lumina\_node\_wasm.d.ts:158

***

#### V3

> `readonly` `static` **V3**: [`AppVersion`](#classesappversionmd)

App v3

##### Defined in

lumina\_node\_wasm.d.ts:162

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:146

***

#### latest()

> `static` **latest**(): [`AppVersion`](#classesappversionmd)

Latest App version variant.

##### Returns

[`AppVersion`](#classesappversionmd)

##### Defined in

lumina\_node\_wasm.d.ts:150


<a name="classesblobmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Blob

## Class: Blob

Arbitrary data that can be stored in the network within certain [`Namespace`].

### Constructors

#### new Blob()

> **new Blob**(`namespace`, `data`, `app_version`): [`Blob`](#classesblobmd)

Create a new blob with the given data within the [`Namespace`].

##### Parameters

###### namespace

[`Namespace`](#classesnamespacemd)

###### data

`Uint8Array`\<`ArrayBuffer`\>

###### app\_version

[`AppVersion`](#classesappversionmd)

##### Returns

[`Blob`](#classesblobmd)

##### Defined in

lumina\_node\_wasm.d.ts:180

### Properties

#### commitment

> **commitment**: [`Commitment`](#classescommitmentmd)

A [`Commitment`] computed from the [`Blob`]s data.

##### Defined in

lumina\_node\_wasm.d.ts:202

***

#### data

> **data**: `Uint8Array`\<`ArrayBuffer`\>

Data stored within the [`Blob`].

##### Defined in

lumina\_node\_wasm.d.ts:192

***

#### namespace

> **namespace**: [`Namespace`](#classesnamespacemd)

A [`Namespace`] the [`Blob`] belongs to.

##### Defined in

lumina\_node\_wasm.d.ts:188

***

#### share\_version

> **share\_version**: `number`

Version indicating the format in which [`Share`]s should be created from this [`Blob`].

[`Share`]: crate::share::Share

##### Defined in

lumina\_node\_wasm.d.ts:198

### Accessors

#### index

##### Get Signature

> **get** **index**(): `bigint`

Index of the blob's first share in the EDS. Only set for blobs retrieved from chain.

###### Returns

`bigint`

##### Set Signature

> **set** **index**(`value`): `void`

Index of the blob's first share in the EDS. Only set for blobs retrieved from chain.

###### Parameters

####### value

`bigint`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:206

### Methods

#### clone()

> **clone**(): [`Blob`](#classesblobmd)

Clone a blob creating a new deep copy of it.

##### Returns

[`Blob`](#classesblobmd)

##### Defined in

lumina\_node\_wasm.d.ts:184

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:176

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:171

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:175


<a name="classesblockrangemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / BlockRange

## Class: BlockRange

A range of blocks between `start` and `end` height, inclusive

### Properties

#### end

> **end**: `bigint`

Last block height in range

##### Defined in

lumina\_node\_wasm.d.ts:233

***

#### start

> **start**: `bigint`

First block height in range

##### Defined in

lumina\_node\_wasm.d.ts:229

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:225

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:220

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:224


<a name="classescommitmentmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Commitment

## Class: Commitment

A merkle hash used to identify the [`Blob`]s data.

In Celestia network, the transaction which pays for the blob's inclusion
is separated from the data itself. The reason for that is to allow verifying
the blockchain's state without the need to pull the actual data which got stored.
To achieve that, the [`MsgPayForBlobs`] transaction only includes the [`Commitment`]s
of the blobs it is paying for, not the data itself.

The algorithm of computing the [`Commitment`] of the [`Blob`]'s [`Share`]s is
designed in a way to allow easy and cheap proving of the [`Share`]s inclusion in the
block. It is computed as a [`merkle hash`] of all the [`Nmt`] subtree roots created from
the blob shares included in the [`ExtendedDataSquare`] rows. Assuming the `s1` and `s2`
are the only shares of some blob posted to the celestia, they'll result in a single subtree
root as shown below:

```text
NMT:           row root
               /     \
             o   subtree root
            / \      / \
          _________________
EDS row: | s | s | s1 | s2 |
```

Using subtree roots as a base for [`Commitment`] computation allows for much smaller
inclusion proofs than when the [`Share`]s would be used directly, but it imposes some
constraints on how the [`Blob`]s can be placed in the [`ExtendedDataSquare`]. You can
read more about that in the [`share commitment rules`].

[`Blob`]: crate::Blob
[`Share`]: crate::share::Share
[`MsgPayForBlobs`]: celestia_proto::celestia::blob::v1::MsgPayForBlobs
[`merkle hash`]: tendermint::merkle::simple_hash_from_byte_vectors
[`Nmt`]: crate::nmt::Nmt
[`ExtendedDataSquare`]: crate::ExtendedDataSquare
[`share commitment rules`]: https://github.com/celestiaorg/celestia-app/blob/main/specs/src/specs/data_square_layout.md#blob-share-commitment-rules
### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:283

***

#### hash()

> **hash**(): `Uint8Array`\<`ArrayBuffer`\>

Hash of the commitment

##### Returns

`Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:287

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:278

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:282


<a name="classesconnectioncounterssnapshotmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ConnectionCountersSnapshot

## Class: ConnectionCountersSnapshot

### Properties

#### num\_connections

> **num\_connections**: `number`

The total number of connections, both pending and established.

##### Defined in

lumina\_node\_wasm.d.ts:303

***

#### num\_established

> **num\_established**: `number`

The number of outgoing connections being established.

##### Defined in

lumina\_node\_wasm.d.ts:319

***

#### num\_established\_incoming

> **num\_established\_incoming**: `number`

The number of established incoming connections.

##### Defined in

lumina\_node\_wasm.d.ts:323

***

#### num\_established\_outgoing

> **num\_established\_outgoing**: `number`

The number of established outgoing connections.

##### Defined in

lumina\_node\_wasm.d.ts:327

***

#### num\_pending

> **num\_pending**: `number`

The total number of pending connections, both incoming and outgoing.

##### Defined in

lumina\_node\_wasm.d.ts:307

***

#### num\_pending\_incoming

> **num\_pending\_incoming**: `number`

The total number of pending connections, both incoming and outgoing.

##### Defined in

lumina\_node\_wasm.d.ts:311

***

#### num\_pending\_outgoing

> **num\_pending\_outgoing**: `number`

The number of outgoing connections being established.

##### Defined in

lumina\_node\_wasm.d.ts:315

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:299

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:294

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:298


<a name="classesdataavailabilityheadermd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / DataAvailabilityHeader

## Class: DataAvailabilityHeader

Header with commitments of the data availability.

It consists of the root hashes of the merkle trees created from each
row and column of the [`ExtendedDataSquare`]. Those are used to prove
the inclusion of the data in a block.

The hash of this header is a hash of all rows and columns and thus a
data commitment of the block.

## Example

```no_run
## use celestia_types::{ExtendedHeader, Height, Share};
## use celestia_types::nmt::{Namespace, NamespaceProof};
## fn extended_header() -> ExtendedHeader {
##     unimplemented!();
## }
## fn shares_with_proof(_: Height, _: &Namespace) -> (Vec<Share>, NamespaceProof) {
##     unimplemented!();
## }
// fetch the block header and data for your namespace
let namespace = Namespace::new_v0(&[1, 2, 3, 4]).unwrap();
let eh = extended_header();
let (shares, proof) = shares_with_proof(eh.height(), &namespace);

// get the data commitment for a given row
let dah = eh.dah;
let root = dah.row_root(0).unwrap();

// verify a proof of the inclusion of the shares
assert!(proof.verify_complete_namespace(&root, &shares, *namespace).is_ok());
```

[`ExtendedDataSquare`]: crate::eds::ExtendedDataSquare

### Methods

#### columnRoot()

> **columnRoot**(`column`): `any`

Get the a root of the column with the given index.

##### Parameters

###### column

`number`

##### Returns

`any`

##### Defined in

lumina\_node\_wasm.d.ts:391

***

#### columnRoots()

> **columnRoots**(): `any`[]

Merkle roots of the [`ExtendedDataSquare`] columns.

##### Returns

`any`[]

##### Defined in

lumina\_node\_wasm.d.ts:383

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:375

***

#### hash()

> **hash**(): `any`

Compute the combined hash of all rows and columns.

This is the data commitment for the block.

##### Returns

`any`

##### Defined in

lumina\_node\_wasm.d.ts:397

***

#### rowRoot()

> **rowRoot**(`row`): `any`

Get a root of the row with the given index.

##### Parameters

###### row

`number`

##### Returns

`any`

##### Defined in

lumina\_node\_wasm.d.ts:387

***

#### rowRoots()

> **rowRoots**(): `any`[]

Merkle roots of the [`ExtendedDataSquare`] rows.

##### Returns

`any`[]

##### Defined in

lumina\_node\_wasm.d.ts:379

***

#### squareWidth()

> **squareWidth**(): `number`

Get the size of the [`ExtendedDataSquare`] for which this header was built.

##### Returns

`number`

##### Defined in

lumina\_node\_wasm.d.ts:401

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:370

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:374


<a name="classesextendedheadermd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ExtendedHeader

## Class: ExtendedHeader

Block header together with the relevant Data Availability metadata.

[`ExtendedHeader`]s are used to announce and describe the blocks
in the Celestia network.

Before being used, each header should be validated and verified with a header you trust.

## Example

```
## use celestia_types::ExtendedHeader;
## fn trusted_genesis_header() -> ExtendedHeader {
##     let s = include_str!("../test_data/chain1/extended_header_block_1.json");
##     serde_json::from_str(s).unwrap()
## }
## fn some_untrusted_header() -> ExtendedHeader {
##     let s = include_str!("../test_data/chain1/extended_header_block_27.json");
##     serde_json::from_str(s).unwrap()
## }
let genesis_header = trusted_genesis_header();

// fetch new header
let fetched_header = some_untrusted_header();

fetched_header.validate().expect("Invalid block header");
genesis_header.verify(&fetched_header).expect("Malicious header received");
```

### Properties

#### commit

> `readonly` **commit**: `any`

Commit metadata and signatures from validators committing the block.

##### Defined in

lumina\_node\_wasm.d.ts:532

***

#### dah

> **dah**: [`DataAvailabilityHeader`](#classesdataavailabilityheadermd)

Header of the block data availability.

##### Defined in

lumina\_node\_wasm.d.ts:524

***

#### header

> `readonly` **header**: `any`

Tendermint block header.

##### Defined in

lumina\_node\_wasm.d.ts:528

***

#### validatorSet

> `readonly` **validatorSet**: `any`

Information about the set of validators commiting the block.

##### Defined in

lumina\_node\_wasm.d.ts:536

### Methods

#### clone()

> **clone**(): [`ExtendedHeader`](#classesextendedheadermd)

Clone a header producing a deep copy of it.

##### Returns

[`ExtendedHeader`](#classesextendedheadermd)

##### Defined in

lumina\_node\_wasm.d.ts:446

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:442

***

#### hash()

> **hash**(): `string`

Get the block hash.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:458

***

#### height()

> **height**(): `bigint`

Get the block height.

##### Returns

`bigint`

##### Defined in

lumina\_node\_wasm.d.ts:450

***

#### previousHeaderHash()

> **previousHeaderHash**(): `string`

Get the hash of the previous header.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:462

***

#### time()

> **time**(): `number`

Get the block time.

##### Returns

`number`

##### Defined in

lumina\_node\_wasm.d.ts:454

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:437

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:441

***

#### validate()

> **validate**(): `void`

Decode protobuf encoded header and then validate it.

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:466

***

#### verify()

> **verify**(`untrusted`): `void`

Verify a chain of adjacent untrusted headers and make sure
they are adjacent to `self`.

## Errors

If verification fails, this function will return an error with a reason of failure.
This function will also return an error if untrusted headers and `self` don't form contiguous range

##### Parameters

###### untrusted

[`ExtendedHeader`](#classesextendedheadermd)

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:476

***

#### verifyAdjacentRange()

> **verifyAdjacentRange**(`untrusted`): `void`

Verify a chain of adjacent untrusted headers and make sure
they are adjacent to `self`.

## Note

Provided headers will be consumed by this method, meaning
they will no longer be accessible. If this behavior is not desired,
consider using `ExtendedHeader.clone()`.

```js
const genesis = hdr0;
const headers = [hrd1, hdr2, hdr3];
genesis.verifyAdjacentRange(headers.map(h => h.clone()));
```

## Errors

If verification fails, this function will return an error with a reason of failure.
This function will also return an error if untrusted headers and `self` don't form contiguous range

##### Parameters

###### untrusted

[`ExtendedHeader`](#classesextendedheadermd)[]

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:520

***

#### verifyRange()

> **verifyRange**(`untrusted`): `void`

Verify a chain of adjacent untrusted headers.

## Note

Provided headers will be consumed by this method, meaning
they will no longer be accessible. If this behavior is not desired,
consider using `ExtendedHeader.clone()`.

```js
const genesis = hdr0;
const headers = [hrd1, hdr2, hdr3];
genesis.verifyRange(headers.map(h => h.clone()));
```

## Errors

If verification fails, this function will return an error with a reason of failure.
This function will also return an error if untrusted headers are not adjacent
to each other.

##### Parameters

###### untrusted

[`ExtendedHeader`](#classesextendedheadermd)[]

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:498


<a name="classesintounderlyingbytesourcemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / IntoUnderlyingByteSource

## Class: IntoUnderlyingByteSource

### Properties

#### autoAllocateChunkSize

> `readonly` **autoAllocateChunkSize**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:545

***

#### type

> `readonly` **type**: `"bytes"`

##### Defined in

lumina\_node\_wasm.d.ts:544

### Methods

#### cancel()

> **cancel**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:543

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:540

***

#### pull()

> **pull**(`controller`): `Promise`\<`any`\>

##### Parameters

###### controller

`ReadableByteStreamController`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:542

***

#### start()

> **start**(`controller`): `void`

##### Parameters

###### controller

`ReadableByteStreamController`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:541


<a name="classesintounderlyingsinkmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / IntoUnderlyingSink

## Class: IntoUnderlyingSink

### Methods

#### abort()

> **abort**(`reason`): `Promise`\<`any`\>

##### Parameters

###### reason

`any`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:552

***

#### close()

> **close**(): `Promise`\<`any`\>

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:551

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:549

***

#### write()

> **write**(`chunk`): `Promise`\<`any`\>

##### Parameters

###### chunk

`any`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:550


<a name="classesintounderlyingsourcemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / IntoUnderlyingSource

## Class: IntoUnderlyingSource

### Methods

#### cancel()

> **cancel**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:558

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:556

***

#### pull()

> **pull**(`controller`): `Promise`\<`any`\>

##### Parameters

###### controller

`ReadableStreamDefaultController`\<`any`\>

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:557


<a name="classesnamespacemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Namespace

## Class: Namespace

Namespace of the data published to the celestia network.

The [`Namespace`] is a single byte defining the version
followed by 28 bytes specifying concrete ID of the namespace.

Currently there are two versions of namespaces:

 - version `0` - the one allowing for the custom namespace ids. It requires an id to start
   with 18 `0x00` bytes followed by a user specified suffix (except reserved ones, see below).
 - version `255` - for secondary reserved namespaces. It requires an id to start with 27
   `0xff` bytes followed by a single byte indicating the id.

Some namespaces are reserved for the block creation purposes and cannot be used
when submitting the blobs to celestia. Those fall into one of the two categories:

 - primary reserved namespaces - those use version `0` and have id lower or equal to `0xff`
   so they are always placed in blocks before user-submitted data.
 - secondary reserved namespaces - those use version `0xff` so they are always placed after
   user-submitted data.

### Properties

#### id

> `readonly` **id**: `Uint8Array`\<`ArrayBuffer`\>

Returns the trailing 28 bytes indicating the id of the [`Namespace`].

##### Defined in

lumina\_node\_wasm.d.ts:667

***

#### version

> `readonly` **version**: `number`

Returns the first byte indicating the version of the [`Namespace`].

##### Defined in

lumina\_node\_wasm.d.ts:663

***

#### MAX\_PRIMARY\_RESERVED

> `readonly` `static` **MAX\_PRIMARY\_RESERVED**: [`Namespace`](#classesnamespacemd)

Maximal primary reserved [`Namespace`].

Used to indicate the end of the primary reserved group.

##### Defined in

lumina\_node\_wasm.d.ts:638

***

#### MIN\_SECONDARY\_RESERVED

> `readonly` `static` **MIN\_SECONDARY\_RESERVED**: [`Namespace`](#classesnamespacemd)

Minimal secondary reserved [`Namespace`].

Used to indicate the beginning of the secondary reserved group.

##### Defined in

lumina\_node\_wasm.d.ts:644

***

#### NS\_SIZE

> `readonly` `static` **NS\_SIZE**: `number`

Namespace size in bytes.

##### Defined in

lumina\_node\_wasm.d.ts:617

***

#### PARITY\_SHARE

> `readonly` `static` **PARITY\_SHARE**: [`Namespace`](#classesnamespacemd)

The [`Namespace`] for `parity shares`.

It is the namespace with which all the `parity shares` from
`ExtendedDataSquare` are inserted to the `Nmt` when computing
merkle roots.

##### Defined in

lumina\_node\_wasm.d.ts:659

***

#### PAY\_FOR\_BLOB

> `readonly` `static` **PAY\_FOR\_BLOB**: [`Namespace`](#classesnamespacemd)

Primary reserved [`Namespace`] for the compact Shares with MsgPayForBlobs transactions.

##### Defined in

lumina\_node\_wasm.d.ts:625

***

#### PRIMARY\_RESERVED\_PADDING

> `readonly` `static` **PRIMARY\_RESERVED\_PADDING**: [`Namespace`](#classesnamespacemd)

Primary reserved [`Namespace`] for the `Share`s used for padding.

`Share`s with this namespace are inserted after other shares from primary reserved namespace
so that user-defined namespaces are correctly aligned in `ExtendedDataSquare`

##### Defined in

lumina\_node\_wasm.d.ts:632

***

#### TAIL\_PADDING

> `readonly` `static` **TAIL\_PADDING**: [`Namespace`](#classesnamespacemd)

Secondary reserved [`Namespace`] used for padding after the blobs.

It is used to fill up the `original data square` after all user-submitted
blobs before the parity data is generated for the `ExtendedDataSquare`.

##### Defined in

lumina\_node\_wasm.d.ts:651

***

#### TRANSACTION

> `readonly` `static` **TRANSACTION**: [`Namespace`](#classesnamespacemd)

Primary reserved [`Namespace`] for the compact `Share`s with `cosmos SDK` transactions.

##### Defined in

lumina\_node\_wasm.d.ts:621

### Methods

#### asBytes()

> **asBytes**(): `Uint8Array`\<`ArrayBuffer`\>

Converts the [`Namespace`] to a byte slice.

##### Returns

`Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:613

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:591

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:586

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:590

***

#### fromRaw()

> `static` **fromRaw**(`raw`): [`Namespace`](#classesnamespacemd)

Create a new [`Namespace`] from the raw bytes.

## Errors

This function will return an error if the slice length is different than
[`NS_SIZE`] or if the namespace is invalid. If you are constructing the
version `0` namespace, check [`newV0`].

##### Parameters

###### raw

`Uint8Array`\<`ArrayBuffer`\>

##### Returns

[`Namespace`](#classesnamespacemd)

##### Defined in

lumina\_node\_wasm.d.ts:609

***

#### newV0()

> `static` **newV0**(`id`): [`Namespace`](#classesnamespacemd)

Create a new [`Namespace`] version `0` with given id.

Check [`Namespace::new_v0`] for more details.

[`Namespace::new_v0`]: https://docs.rs/celestia-types/latest/celestia_types/nmt/struct.Namespace.html#method.new_v0
##### Parameters

###### id

`Uint8Array`\<`ArrayBuffer`\>

##### Returns

[`Namespace`](#classesnamespacemd)

##### Defined in

lumina\_node\_wasm.d.ts:599


<a name="classesnetworkinfosnapshotmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / NetworkInfoSnapshot

## Class: NetworkInfoSnapshot

Information about the connections

### Properties

#### connection\_counters

> **connection\_counters**: [`ConnectionCountersSnapshot`](#classesconnectioncounterssnapshotmd)

Gets counters for ongoing network connections.

##### Defined in

lumina\_node\_wasm.d.ts:690

***

#### num\_peers

> **num\_peers**: `number`

The number of connected peers, i.e. peers with whom at least one established connection exists.

##### Defined in

lumina\_node\_wasm.d.ts:686

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:682

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:677

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:681


<a name="classesnodeclientmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / NodeClient

## Class: NodeClient

`NodeClient` is responsible for steering [`NodeWorker`] by sending it commands and receiving
responses over the provided port.

[`NodeWorker`]: crate::worker::NodeWorker

### Constructors

#### new NodeClient()

> **new NodeClient**(`port`): [`NodeClient`](#classesnodeclientmd)

Create a new connection to a Lumina node running in [`NodeWorker`]. Provided `port` is
expected to have `MessagePort`-like interface for sending and receiving messages.

##### Parameters

###### port

`any`

##### Returns

[`NodeClient`](#classesnodeclientmd)

##### Defined in

lumina\_node\_wasm.d.ts:704

### Methods

#### addConnectionToWorker()

> **addConnectionToWorker**(`port`): `Promise`\<`void`\>

Establish a new connection to the existing worker over provided port

##### Parameters

###### port

`any`

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:708

***

#### connectedPeers()

> **connectedPeers**(): `Promise`\<`any`[]\>

Get all the peers that node is connected to.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:745

***

#### eventsChannel()

> **eventsChannel**(): `Promise`\<`BroadcastChannel`\>

Returns a [`BroadcastChannel`] for events generated by [`Node`].

##### Returns

`Promise`\<`BroadcastChannel`\>

##### Defined in

lumina\_node\_wasm.d.ts:812

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:699

***

#### getHeaderByHash()

> **getHeaderByHash**(`hash`): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Get a synced header for the block with a given hash.

##### Parameters

###### hash

`string`

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:788

***

#### getHeaderByHeight()

> **getHeaderByHeight**(`height`): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Get a synced header for the block with a given height.

##### Parameters

###### height

`bigint`

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:792

***

#### getHeaders()

> **getHeaders**(`start_height`?, `end_height`?): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)[]\>

Get synced headers from the given heights range.

If start of the range is undefined (None), the first returned header will be of height 1.
If end of the range is undefined (None), the last returned header will be the last header in the
store.

## Errors

If range contains a height of a header that is not found in the store.

##### Parameters

###### start\_height?

`bigint`

###### end\_height?

`bigint`

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:804

***

#### getLocalHeadHeader()

> **getLocalHeadHeader**(): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Get the latest locally synced header.

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:784

***

#### getNetworkHeadHeader()

> **getNetworkHeadHeader**(): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Get the latest header announced in the network.

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:780

***

#### getSamplingMetadata()

> **getSamplingMetadata**(`height`): `Promise`\<[`SamplingMetadata`](#classessamplingmetadatamd)\>

Get data sampling metadata of an already sampled height.

##### Parameters

###### height

`bigint`

##### Returns

`Promise`\<[`SamplingMetadata`](#classessamplingmetadatamd)\>

##### Defined in

lumina\_node\_wasm.d.ts:808

***

#### isRunning()

> **isRunning**(): `Promise`\<`boolean`\>

Check whether Lumina is currently running

##### Returns

`Promise`\<`boolean`\>

##### Defined in

lumina\_node\_wasm.d.ts:712

***

#### listeners()

> **listeners**(): `Promise`\<`any`[]\>

Get all the multiaddresses on which the node listens.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:741

***

#### localPeerId()

> **localPeerId**(): `Promise`\<`string`\>

Get node's local peer ID.

##### Returns

`Promise`\<`string`\>

##### Defined in

lumina\_node\_wasm.d.ts:721

***

#### networkInfo()

> **networkInfo**(): `Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

Get current network info.

##### Returns

`Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:737

***

#### peerTrackerInfo()

> **peerTrackerInfo**(): `Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

Get current [`PeerTracker`] info.

##### Returns

`Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:725

***

#### requestAllBlobs()

> **requestAllBlobs**(`header`, `namespace`, `timeout_secs`?): `Promise`\<[`Blob`](#classesblobmd)[]\>

Request all blobs with provided namespace in the block corresponding to this header
using bitswap protocol.

##### Parameters

###### header

[`ExtendedHeader`](#classesextendedheadermd)

###### namespace

[`Namespace`](#classesnamespacemd)

###### timeout\_secs?

`number`

##### Returns

`Promise`\<[`Blob`](#classesblobmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:772

***

#### requestHeaderByHash()

> **requestHeaderByHash**(`hash`): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Request a header for the block with a given hash from the network.

##### Parameters

###### hash

`string`

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:757

***

#### requestHeaderByHeight()

> **requestHeaderByHeight**(`height`): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Request a header for the block with a given height from the network.

##### Parameters

###### height

`bigint`

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:761

***

#### requestHeadHeader()

> **requestHeadHeader**(): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Request the head header from the network.

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:753

***

#### requestVerifiedHeaders()

> **requestVerifiedHeaders**(`from`, `amount`): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)[]\>

Request headers in range (from, from + amount] from the network.

The headers will be verified with the `from` header.

##### Parameters

###### from

[`ExtendedHeader`](#classesextendedheadermd)

###### amount

`bigint`

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:767

***

#### setPeerTrust()

> **setPeerTrust**(`peer_id`, `is_trusted`): `Promise`\<`void`\>

Trust or untrust the peer with a given ID.

##### Parameters

###### peer\_id

`string`

###### is\_trusted

`boolean`

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:749

***

#### start()

> **start**(`config`): `Promise`\<`void`\>

Start a node with the provided config, if it's not running

##### Parameters

###### config

[`NodeConfig`](#classesnodeconfigmd)

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:716

***

#### stop()

> **stop**(): `Promise`\<`void`\>

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:717

***

#### syncerInfo()

> **syncerInfo**(): `Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

Get current header syncing info.

##### Returns

`Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:776

***

#### waitConnected()

> **waitConnected**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:729

***

#### waitConnectedTrusted()

> **waitConnectedTrusted**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 trusted peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:733


<a name="classesnodeconfigmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / NodeConfig

## Class: NodeConfig

Config for the lumina wasm node.

### Properties

#### bootnodes

> **bootnodes**: `string`[]

A list of bootstrap peers to connect to.

##### Defined in

lumina\_node\_wasm.d.ts:839

***

#### network

> **network**: [`Network`](#enumerationsnetworkmd)

A network to connect to.

##### Defined in

lumina\_node\_wasm.d.ts:835

***

#### usePersistentMemory

> **usePersistentMemory**: `boolean`

Whether to store data in persistent memory or not.

**Default value:** true

##### Defined in

lumina\_node\_wasm.d.ts:845

### Accessors

#### customPruningDelaySecs

##### Get Signature

> **get** **customPruningDelaySecs**(): `number`

Pruning delay defines how much time the pruner should wait after sampling window in
order to prune the block.

If this is not set, then default value will apply:

* If `use_persistent_memory == true`, default value is 1 hour.
* If `use_persistent_memory == false`, default value is 60 seconds.

The minimum value that can be set is 60 seconds.

###### Returns

`number`

##### Set Signature

> **set** **customPruningDelaySecs**(`value`): `void`

Pruning delay defines how much time the pruner should wait after sampling window in
order to prune the block.

If this is not set, then default value will apply:

* If `use_persistent_memory == true`, default value is 1 hour.
* If `use_persistent_memory == false`, default value is 60 seconds.

The minimum value that can be set is 60 seconds.

###### Parameters

####### value

`number`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:879

***

#### customSamplingWindowSecs

##### Get Signature

> **get** **customSamplingWindowSecs**(): `number`

Sampling window defines maximum age of a block considered for syncing and sampling.

If this is not set, then default value will apply:

* If `use_persistent_memory == true`, default value is 30 days.
* If `use_persistent_memory == false`, default value is 60 seconds.

The minimum value that can be set is 60 seconds.

###### Returns

`number`

##### Set Signature

> **set** **customSamplingWindowSecs**(`value`): `void`

Sampling window defines maximum age of a block considered for syncing and sampling.

If this is not set, then default value will apply:

* If `use_persistent_memory == true`, default value is 30 days.
* If `use_persistent_memory == false`, default value is 60 seconds.

The minimum value that can be set is 60 seconds.

###### Parameters

####### value

`number`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:856

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:827

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:822

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:826

***

#### default()

> `static` **default**(`network`): [`NodeConfig`](#classesnodeconfigmd)

Get the configuration with default bootnodes for provided network

##### Parameters

###### network

[`Network`](#enumerationsnetworkmd)

##### Returns

[`NodeConfig`](#classesnodeconfigmd)

##### Defined in

lumina\_node\_wasm.d.ts:831


<a name="classesnodeworkermd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / NodeWorker

## Class: NodeWorker

`NodeWorker` is responsible for receiving commands from connected [`NodeClient`]s, executing
them and sending a response back, as well as accepting new `NodeClient` connections.

[`NodeClient`]: crate::client::NodeClient

### Constructors

#### new NodeWorker()

> **new NodeWorker**(`port_like_object`): [`NodeWorker`](#classesnodeworkermd)

##### Parameters

###### port\_like\_object

`any`

##### Returns

[`NodeWorker`](#classesnodeworkermd)

##### Defined in

lumina\_node\_wasm.d.ts:901

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:900

***

#### run()

> **run**(): `Promise`\<`void`\>

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:902


<a name="classespeertrackerinfosnapshotmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / PeerTrackerInfoSnapshot

## Class: PeerTrackerInfoSnapshot

Statistics of the connected peers

### Properties

#### num\_connected\_peers

> **num\_connected\_peers**: `bigint`

Number of the connected peers.

##### Defined in

lumina\_node\_wasm.d.ts:921

***

#### num\_connected\_trusted\_peers

> **num\_connected\_trusted\_peers**: `bigint`

Number of the connected trusted peers.

##### Defined in

lumina\_node\_wasm.d.ts:925

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:917

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:912

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:916


<a name="classessamplingmetadatamd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SamplingMetadata

## Class: SamplingMetadata

Sampling metadata for a block.

This struct persists DAS-ing information in a header store for future reference.

### Properties

#### cids

> `readonly` **cids**: `Uint8Array`\<`ArrayBuffer`\>[]

Return Array of cids

##### Defined in

lumina\_node\_wasm.d.ts:942

***

#### status

> **status**: [`SamplingStatus`](#enumerationssamplingstatusmd)

Indicates whether this node was able to successfuly sample the block

##### Defined in

lumina\_node\_wasm.d.ts:938

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:934


<a name="classessyncinginfosnapshotmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SyncingInfoSnapshot

## Class: SyncingInfoSnapshot

Status of the synchronization.

### Properties

#### stored\_headers

> **stored\_headers**: [`BlockRange`](#classesblockrangemd)[]

Ranges of headers that are already synchronised

##### Defined in

lumina\_node\_wasm.d.ts:961

***

#### subjective\_head

> **subjective\_head**: `bigint`

Syncing target. The latest height seen in the network that was successfully verified.

##### Defined in

lumina\_node\_wasm.d.ts:965

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:957

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:952

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:956


<a name="classestxclientmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxClient

## Class: TxClient

Celestia grpc transaction client.

### Constructors

#### new TxClient()

> **new TxClient**(`url`, `bech32_address`, `pubkey`, `signer_fn`): [`TxClient`](#classestxclientmd)

Create a new transaction client with the specified account.

Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).

## Example with noble/curves
```js
import { secp256k1 } from "@noble/curves/secp256k1";

const address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm";
const privKey = "fdc8ac75dfa1c142dbcba77938a14dd03078052ce0b49a529dcf72a9885a3abb";
const pubKey = secp256k1.getPublicKey(privKey);

const signer = (signDoc) => {
  const bytes = protoEncodeSignDoc(signDoc);
  const sig = secp256k1.sign(bytes, privKey, { prehash: true });
  return sig.toCompactRawBytes();
};

const txClient = await new TxClient("http://127.0.0.1:18080", address, pubKey, signer);
```

## Example with leap wallet
```js
await window.leap.enable("mocha-4")
const keys = await window.leap.getKey("mocha-4")

const signer = (signDoc) => {
  return window.leap.signDirect("mocha-4", keys.bech32Address, signDoc, { preferNoSetFee: true })
    .then(sig => Uint8Array.from(atob(sig.signature.signature), c => c.charCodeAt(0)))
}

const tx_client = await new TxClient("http://127.0.0.1:18080", keys.bech32Address, keys.pubKey, signer)
```

##### Parameters

###### url

`string`

###### bech32\_address

`string`

###### pubkey

`Uint8Array`\<`ArrayBuffer`\>

###### signer\_fn

[`SignerFn`](#type-aliasessignerfnmd)

##### Returns

[`TxClient`](#classestxclientmd)

##### Defined in

lumina\_node\_wasm.d.ts:1007

### Properties

#### appVersion

> `readonly` **appVersion**: [`AppVersion`](#classesappversionmd)

AppVersion of the client

##### Defined in

lumina\_node\_wasm.d.ts:1102

***

#### chainId

> `readonly` **chainId**: `string`

Chain id of the client

##### Defined in

lumina\_node\_wasm.d.ts:1098

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:971

***

#### getAccount()

> **getAccount**(`account`): `Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)\>

Get account

##### Parameters

###### account

`string`

##### Returns

`Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1074

***

#### getAccounts()

> **getAccounts**(): `Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)[]\>

Get accounts

##### Returns

`Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1078

***

#### getAllBalances()

> **getAllBalances**(`address`): `Promise`\<[`Coin`](#interfacescoinmd)[]\>

Get balance of all coins

##### Parameters

###### address

`string`

##### Returns

`Promise`\<[`Coin`](#interfacescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1086

***

#### getAuthParams()

> **getAuthParams**(): `Promise`\<[`AuthParams`](#interfacesauthparamsmd)\>

Get auth params

##### Returns

`Promise`\<[`AuthParams`](#interfacesauthparamsmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1070

***

#### getBalance()

> **getBalance**(`address`, `denom`): `Promise`\<[`Coin`](#interfacescoinmd)\>

Get balance of coins with given denom

##### Parameters

###### address

`string`

###### denom

`string`

##### Returns

`Promise`\<[`Coin`](#interfacescoinmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1082

***

#### getSpendableBalances()

> **getSpendableBalances**(`address`): `Promise`\<[`Coin`](#interfacescoinmd)[]\>

Get balance of all spendable coins

##### Parameters

###### address

`string`

##### Returns

`Promise`\<[`Coin`](#interfacescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1090

***

#### getTotalSupply()

> **getTotalSupply**(): `Promise`\<[`Coin`](#interfacescoinmd)[]\>

Get total supply

##### Returns

`Promise`\<[`Coin`](#interfacescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1094

***

#### lastSeenGasPrice()

> **lastSeenGasPrice**(): `number`

Last gas price fetched by the client

##### Returns

`number`

##### Defined in

lumina\_node\_wasm.d.ts:1011

***

#### submitBlobs()

> **submitBlobs**(`blobs`, `tx_config`?): `Promise`\<[`TxInfo`](#interfacestxinfomd)\>

Submit blobs to the celestia network.

When no `TxConfig` is provided, client will automatically calculate needed
gas and update the `gasPrice`, if network agreed on a new minimal value.
To enforce specific values use a `TxConfig`.

## Example
```js
const ns = Namespace.newV0(new Uint8Array([97, 98, 99]));
const data = new Uint8Array([100, 97, 116, 97]);
const blob = new Blob(ns, data, AppVersion.latest());

const txInfo = await txClient.submitBlobs([blob]);
await txClient.submitBlobs([blob], { gasLimit: 100000n, gasPrice: 0.02 });
```

## Note

Provided blobs will be consumed by this method, meaning
they will no longer be accessible. If this behavior is not desired,
consider using `Blob.clone()`.

```js
const blobs = [blob1, blob2, blob3];
await txClient.submitBlobs(blobs.map(b => b.clone()));
```

##### Parameters

###### blobs

[`Blob`](#classesblobmd)[]

###### tx\_config?

[`TxConfig`](#interfacestxconfigmd)

##### Returns

`Promise`\<[`TxInfo`](#interfacestxinfomd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1040

***

#### submitMessage()

> **submitMessage**(`message`, `tx_config`?): `Promise`\<[`TxInfo`](#interfacestxinfomd)\>

Submit message to the celestia network.

When no `TxConfig` is provided, client will automatically calculate needed
gas and update the `gasPrice`, if network agreed on a new minimal value.
To enforce specific values use a `TxConfig`.

## Example
```js
import { Registry } from "@cosmjs/proto-signing";

const registry = new Registry();
const sendMsg = {
  typeUrl: "/cosmos.bank.v1beta1.MsgSend",
  value: {
    fromAddress: "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm",
    toAddress: "celestia1t52q7uqgnjfzdh3wx5m5phvma3umrq8k6tq2p9",
    amount: [{ denom: "utia", amount: "10000" }],
  },
};
const sendMsgAny = registry.encodeAsAny(sendMsg);

const txInfo = await txClient.submitMessage(sendMsgAny);
```

##### Parameters

###### message

[`ProtoAny`](#interfacesprotoanymd)

###### tx\_config?

[`TxConfig`](#interfacestxconfigmd)

##### Returns

`Promise`\<[`TxInfo`](#interfacestxinfomd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1066

# Enumerations


<a name="enumerationsnetworkmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Network

## Enumeration: Network

Supported Celestia networks.

### Enumeration Members

#### Arabica

> **Arabica**: `1`

Arabica testnet.

##### Defined in

lumina\_node\_wasm.d.ts:22

***

#### Mainnet

> **Mainnet**: `0`

Celestia mainnet.

##### Defined in

lumina\_node\_wasm.d.ts:18

***

#### Mocha

> **Mocha**: `2`

Mocha testnet.

##### Defined in

lumina\_node\_wasm.d.ts:26

***

#### Private

> **Private**: `3`

Private local network.

##### Defined in

lumina\_node\_wasm.d.ts:30


<a name="enumerationssamplingstatusmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SamplingStatus

## Enumeration: SamplingStatus

Sampling status for a block.

### Enumeration Members

#### Accepted

> **Accepted**: `1`

Sampling is done and block is accepted.

##### Defined in

lumina\_node\_wasm.d.ts:43

***

#### Rejected

> **Rejected**: `2`

Sampling is done and block is rejected.

##### Defined in

lumina\_node\_wasm.d.ts:47

***

#### Unknown

> **Unknown**: `0`

Sampling is not done.

##### Defined in

lumina\_node\_wasm.d.ts:39

# Functions


<a name="functionsprotoencodesigndocmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / protoEncodeSignDoc

## Function: protoEncodeSignDoc()

> **protoEncodeSignDoc**(`sign_doc`): `Uint8Array`

A helper to encode the SignDoc with protobuf to get bytes to sign directly.

### Parameters

#### sign\_doc

[`SignDoc`](#interfacessigndocmd)

### Returns

`Uint8Array`

### Defined in

lumina\_node\_wasm.d.ts:10


<a name="functionssetup_loggingmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / setup\_logging

## Function: setup\_logging()

> **setup\_logging**(): `void`

Set up a logging layer that direct logs to the browser's console.

### Returns

`void`

### Defined in

lumina\_node\_wasm.d.ts:6


<a name="globalsmd"></a>

[**lumina-node-wasm**](#readmemd)

***

# lumina-node-wasm

## Enumerations

- [Network](#enumerationsnetworkmd)
- [SamplingStatus](#enumerationssamplingstatusmd)

## Classes

- [AppVersion](#classesappversionmd)
- [Blob](#classesblobmd)
- [BlockRange](#classesblockrangemd)
- [Commitment](#classescommitmentmd)
- [ConnectionCountersSnapshot](#classesconnectioncounterssnapshotmd)
- [DataAvailabilityHeader](#classesdataavailabilityheadermd)
- [ExtendedHeader](#classesextendedheadermd)
- [IntoUnderlyingByteSource](#classesintounderlyingbytesourcemd)
- [IntoUnderlyingSink](#classesintounderlyingsinkmd)
- [IntoUnderlyingSource](#classesintounderlyingsourcemd)
- [Namespace](#classesnamespacemd)
- [NetworkInfoSnapshot](#classesnetworkinfosnapshotmd)
- [NodeClient](#classesnodeclientmd)
- [NodeConfig](#classesnodeconfigmd)
- [NodeWorker](#classesnodeworkermd)
- [PeerTrackerInfoSnapshot](#classespeertrackerinfosnapshotmd)
- [SamplingMetadata](#classessamplingmetadatamd)
- [SyncingInfoSnapshot](#classessyncinginfosnapshotmd)
- [TxClient](#classestxclientmd)

## Interfaces

- [AuthParams](#interfacesauthparamsmd)
- [BaseAccount](#interfacesbaseaccountmd)
- [Coin](#interfacescoinmd)
- [ProtoAny](#interfacesprotoanymd)
- [PublicKey](#interfacespublickeymd)
- [SignDoc](#interfacessigndocmd)
- [TxConfig](#interfacestxconfigmd)
- [TxInfo](#interfacestxinfomd)

## Type Aliases

- [ReadableStreamType](#type-aliasesreadablestreamtypemd)
- [SignerFn](#type-aliasessignerfnmd)

## Functions

- [protoEncodeSignDoc](#functionsprotoencodesigndocmd)
- [setup\_logging](#functionssetup_loggingmd)

# Interfaces


<a name="interfacesauthparamsmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AuthParams

## Interface: AuthParams

Auth module parameters

### Properties

#### maxMemoCharacters

> **maxMemoCharacters**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:78

***

#### sigVerifyCostEd25519

> **sigVerifyCostEd25519**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:81

***

#### sigVerifyCostSecp256k1

> **sigVerifyCostSecp256k1**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:82

***

#### txSigLimit

> **txSigLimit**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:79

***

#### txSizeCostPerByte

> **txSizeCostPerByte**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:80


<a name="interfacesbaseaccountmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / BaseAccount

## Interface: BaseAccount

Common data of all account types

### Properties

#### accountNumber

> **accountNumber**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:70

***

#### address

> **address**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:68

***

#### pubkey?

> `optional` **pubkey**: [`PublicKey`](#interfacespublickeymd)

##### Defined in

lumina\_node\_wasm.d.ts:69

***

#### sequence

> **sequence**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:71


<a name="interfacescoinmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Coin

## Interface: Coin

Coin

### Properties

#### amount

> **amount**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:92

***

#### denom

> **denom**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:91


<a name="interfacesprotoanymd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ProtoAny

## Interface: ProtoAny

Protobuf Any type

### Properties

#### typeUrl

> **typeUrl**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:136

***

#### value

> **value**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:137


<a name="interfacespublickeymd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / PublicKey

## Interface: PublicKey

Public key

### Properties

#### type

> **type**: `"ed25519"` \| `"secp256k1"`

##### Defined in

lumina\_node\_wasm.d.ts:60

***

#### value

> **value**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:61


<a name="interfacessigndocmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SignDoc

## Interface: SignDoc

A payload to be signed

### Properties

#### accountNumber

> **accountNumber**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:122

***

#### authInfoBytes

> **authInfoBytes**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:120

***

#### bodyBytes

> **bodyBytes**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:119

***

#### chainId

> **chainId**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:121


<a name="interfacestxconfigmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxConfig

## Interface: TxConfig

Transaction config.

### Properties

#### gasLimit?

> `optional` **gasLimit**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:109

***

#### gasPrice?

> `optional` **gasPrice**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:110


<a name="interfacestxinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxInfo

## Interface: TxInfo

Transaction info

### Properties

#### hash

> **hash**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:101

***

#### height

> **height**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:102

# Type Aliases


<a name="type-aliasesreadablestreamtypemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ReadableStreamType

## Type Alias: ReadableStreamType

> **ReadableStreamType**: `"bytes"`

The `ReadableStreamType` enum.

*This API requires the following crate features to be activated: `ReadableStreamType`*

### Defined in

lumina\_node\_wasm.d.ts:54


<a name="type-aliasessignerfnmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SignerFn

## Type Alias: SignerFn

> **SignerFn**: (`arg`) => `Uint8Array` \| (`arg`) => `Promise`\<`Uint8Array`\>

A function that produces a signature of a payload

### Defined in

lumina\_node\_wasm.d.ts:128
