
<a name="readmemd"></a>

**lumina-node-wasm**

***

# Lumina node wasm

A compatibility layer for the [`Lumina`](https://github.com/eigerco/lumina) node to
work within a browser environment and be operable with javascript.

# Changelog

You can find about the latest changes [here](https://github.com/eigerco/lumina/blob/main/node-wasm/CHANGELOG.md).

# Example
Starting lumina inside a dedicated worker

```javascript
import { spawnNode, Network, NodeConfig } from "lumina-node";

const node = await spawnNode();
const mainnetConfig = NodeConfig.default(Network.Mainnet);

await node.start(mainnetConfig);

await node.waitConnected();
await node.requestHeadHeader();
```

## Manual setup

`spawnNode` sets up a `DedicatedWorker` instance and runs `NodeWorker` there. If you want to set things up manually
you need to connect client and worker using objects that have `MessagePort` interface.

```javascript
import { Network, NodeClient, NodeConfig, NodeWorker } from "lumina-node";

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

For comprehensive and fully typed interface documentation, see [lumina-node](https://docs.rs/lumina-node/latest/lumina_node/)
and [celestia-types](https://docs.rs/celestia-types/latest/celestia_types/) documentation on docs.rs.
You can see there the exact structure of more complex types, such as [`ExtendedHeader`](https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html).
JavaScript API's goal is to provide similar interface to Rust when possible, e.g. `NodeClient` mirrors [`Node`](https://docs.rs/lumina-node/latest/lumina_node/node/struct.Node.html).

# Classes


<a name="classesabcimessagelogmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AbciMessageLog

## Class: AbciMessageLog

ABCIMessageLog defines a structure containing an indexed tx ABCI message log.

### Properties

#### events

> **events**: [`StringEvent`](#classesstringeventmd)[]

Events contains a slice of Event objects that were emitted during some
execution.

##### Defined in

lumina\_node\_wasm.d.ts:531

***

#### log

> **log**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:526

***

#### msg\_index

> **msg\_index**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:525

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:524


<a name="classesabciqueryresponsemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AbciQueryResponse

## Class: AbciQueryResponse

Response to a tx query

### Properties

#### code

> **code**: [`ErrorCode`](#enumerationserrorcodemd)

Response code.

##### Defined in

lumina\_node\_wasm.d.ts:542

***

#### codespace

> **codespace**: `string`

Namespace for the Code.

##### Defined in

lumina\_node\_wasm.d.ts:546

***

#### height

> `readonly` **height**: `bigint`

The block height from which data was derived.

Note that this is the height of the block containing the application's Merkle root hash,
which represents the state as it was after committing the block at height - 1.

##### Defined in

lumina\_node\_wasm.d.ts:588

***

#### index

> **index**: `bigint`

The index of the key in the tree.

##### Defined in

lumina\_node\_wasm.d.ts:550

***

#### info

> **info**: `string`

Additional information. May be non-deterministic.

##### Defined in

lumina\_node\_wasm.d.ts:581

***

#### key

> **key**: `Uint8Array`\<`ArrayBuffer`\>

The key of the matching data.

##### Defined in

lumina\_node\_wasm.d.ts:554

***

#### log

> **log**: `string`

The output of the application's logger (raw string). May be
non-deterministic.

##### Defined in

lumina\_node\_wasm.d.ts:577

***

#### value

> **value**: `Uint8Array`\<`ArrayBuffer`\>

The value of the matching data.

##### Defined in

lumina\_node\_wasm.d.ts:558

### Accessors

#### proof\_ops

##### Get Signature

> **get** **proof\_ops**(): [`ProofOps`](#classesproofopsmd)

Serialized proof for the value data, if requested,
to be verified against the [`AppHash`] for the given [`Height`].

[`AppHash`]: tendermint::hash::AppHash

###### Returns

[`ProofOps`](#classesproofopsmd)

##### Set Signature

> **set** **proof\_ops**(`value`): `void`

Serialized proof for the value data, if requested,
to be verified against the [`AppHash`] for the given [`Height`].

[`AppHash`]: tendermint::hash::AppHash

###### Parameters

####### value

[`ProofOps`](#classesproofopsmd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:565

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:538


<a name="classesaccaddressmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AccAddress

## Class: AccAddress

Address of an account.

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:603

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:598

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:602


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

lumina\_node\_wasm.d.ts:618

***

#### V2

> `readonly` `static` **V2**: [`AppVersion`](#classesappversionmd)

App v2

##### Defined in

lumina\_node\_wasm.d.ts:622

***

#### V3

> `readonly` `static` **V3**: [`AppVersion`](#classesappversionmd)

App v3

##### Defined in

lumina\_node\_wasm.d.ts:626

***

#### V4

> `readonly` `static` **V4**: [`AppVersion`](#classesappversionmd)

App v4

##### Defined in

lumina\_node\_wasm.d.ts:630

***

#### V5

> `readonly` `static` **V5**: [`AppVersion`](#classesappversionmd)

App v5

##### Defined in

lumina\_node\_wasm.d.ts:634

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:610

***

#### latest()

> `static` **latest**(): [`AppVersion`](#classesappversionmd)

Latest App version variant.

##### Returns

[`AppVersion`](#classesappversionmd)

##### Defined in

lumina\_node\_wasm.d.ts:614


<a name="classesattributemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Attribute

## Class: Attribute

Attribute defines an attribute wrapper where the key and value are
strings instead of raw bytes.

### Properties

#### key

> **key**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:643

***

#### value

> **value**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:644

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:642


<a name="classesauthinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AuthInfo

## Class: AuthInfo

[`AuthInfo`] describes the fee and signer modes that are used to sign a transaction.

### Properties

#### fee

> **fee**: [`Fee`](#classesfeemd)

[`Fee`] and gas limit for the transaction.

The first signer is the primary signer and the one which pays the fee.
The fee can be calculated based on the cost of evaluating the body and doing signature
verification of the signers. This can be estimated via simulation.

##### Defined in

lumina\_node\_wasm.d.ts:667

***

#### signer\_infos

> **signer\_infos**: [`SignerInfo`](#classessignerinfomd)[]

Defines the signing modes for the required signers.

The number and order of elements must match the required signers from transaction
[`TxBody`]’s messages. The first element is the primary signer and the one
which pays the [`Fee`].

##### Defined in

lumina\_node\_wasm.d.ts:659

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:651


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

lumina\_node\_wasm.d.ts:685

### Properties

#### commitment

> **commitment**: [`Commitment`](#classescommitmentmd)

A [`Commitment`] computed from the [`Blob`]s data.

##### Defined in

lumina\_node\_wasm.d.ts:705

***

#### data

> **data**: `Uint8Array`\<`ArrayBuffer`\>

Data stored within the [`Blob`].

##### Defined in

lumina\_node\_wasm.d.ts:697

***

#### namespace

> **namespace**: [`Namespace`](#classesnamespacemd)

A [`Namespace`] the [`Blob`] belongs to.

##### Defined in

lumina\_node\_wasm.d.ts:693

***

#### share\_version

> **share\_version**: `number`

Version indicating the format in which [`Share`]s should be created from this [`Blob`].

##### Defined in

lumina\_node\_wasm.d.ts:701

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

lumina\_node\_wasm.d.ts:709

***

#### signer

##### Get Signature

> **get** **signer**(): [`AccAddress`](#classesaccaddressmd)

A signer of the blob, i.e. address of the account which submitted the blob.

Must be present in `share_version 1` and absent otherwise.

###### Returns

[`AccAddress`](#classesaccaddressmd)

##### Set Signature

> **set** **signer**(`value`): `void`

A signer of the blob, i.e. address of the account which submitted the blob.

Must be present in `share_version 1` and absent otherwise.

###### Parameters

####### value

[`AccAddress`](#classesaccaddressmd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:719

### Methods

#### clone()

> **clone**(): [`Blob`](#classesblobmd)

Clone a blob creating a new deep copy of it.

##### Returns

[`Blob`](#classesblobmd)

##### Defined in

lumina\_node\_wasm.d.ts:689

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:681

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:676

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:680


<a name="classesblobparamsmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / BlobParams

## Class: BlobParams

Params defines the parameters for the blob module.

### Properties

#### gas\_per\_blob\_byte

> **gas\_per\_blob\_byte**: `number`

Gas cost per blob byte

##### Defined in

lumina\_node\_wasm.d.ts:744

***

#### gov\_max\_square\_size

> **gov\_max\_square\_size**: `bigint`

Max square size

##### Defined in

lumina\_node\_wasm.d.ts:748

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:740

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:735

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:739


<a name="classesblockmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Block

## Class: Block

Blocks consist of a header, transactions, votes (the commit), and a list of
evidence of malfeasance (i.e. signing conflicting votes).

This is a modified version of [`tendermint::block::Block`] which contains
[modifications](data-mod) that Celestia introduced.

[data-mod]: https://github.com/celestiaorg/celestia-core/blob/a1268f7ae3e688144a613c8a439dd31818aae07d/proto/tendermint/types/types.proto#L84-L104
### Properties

#### data

> **data**: [`Data`](#classesdatamd)

Transaction data

##### Defined in

lumina\_node\_wasm.d.ts:765

***

#### evidence

> `readonly` **evidence**: [`Evidence`](#classesevidencemd)[]

Evidence of malfeasance

##### Defined in

lumina\_node\_wasm.d.ts:773

***

#### header

> `readonly` **header**: [`Header`](#classesheadermd)

Block header

##### Defined in

lumina\_node\_wasm.d.ts:769

***

#### lastCommit

> `readonly` **lastCommit**: [`Commit`](#classescommitmd)

Last commit, should be `None` for the initial block.

##### Defined in

lumina\_node\_wasm.d.ts:777

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:761


<a name="classesblockidmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / BlockId

## Class: BlockId

Block identifiers which contain two distinct Merkle roots of the block, as well as the number of parts in the block.

### Properties

#### hash

> **hash**: `string`

The block’s main hash is the Merkle root of all the fields in the block header.

##### Defined in

lumina\_node\_wasm.d.ts:788

***

#### part\_set\_header

> **part\_set\_header**: [`PartsHeader`](#classespartsheadermd)

Parts header (if available) is used for secure gossipping of the block during
consensus. It is the Merkle root of the complete serialized block cut into parts.

PartSet is used to split a byteslice of data into parts (pieces) for transmission.
By splitting data into smaller parts and computing a Merkle root hash on the list,
you can verify that a part is legitimately part of the complete data, and the part
can be forwarded to other peers before all the parts are known. In short, it’s
a fast way to propagate a large file over a gossip network.

<https://github.com/tendermint/tendermint/wiki/Block-Structure#partset>

PartSetHeader in protobuf is defined as never nil using the gogoproto annotations.
This does not translate to Rust, but we can indicate this in the domain type.

##### Defined in

lumina\_node\_wasm.d.ts:804

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:784


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

lumina\_node\_wasm.d.ts:827

***

#### start

> **start**: `bigint`

First block height in range

##### Defined in

lumina\_node\_wasm.d.ts:823

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:819

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:814

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:818


<a name="classesbroadcastmodemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / BroadcastMode

## Class: BroadcastMode

BroadcastMode specifies the broadcast mode for the TxService.Broadcast RPC method.

### Properties

#### Async

> `readonly` `static` **Async**: [`BroadcastMode`](#classesbroadcastmodemd)

`BroadcastMode` `Async` defines a tx broadcasting mode where the client returns
immediately.

##### Defined in

lumina\_node\_wasm.d.ts:853

***

#### Block

> `readonly` `static` **Block**: [`BroadcastMode`](#classesbroadcastmodemd)

`BroadcastMode` `Block` defines a tx broadcasting mode where the client waits for
the tx to be committed in a block.

##### Defined in

lumina\_node\_wasm.d.ts:843

***

#### Sync

> `readonly` `static` **Sync**: [`BroadcastMode`](#classesbroadcastmodemd)

`BroadcastMode` `Sync` defines a tx broadcasting mode where the client waits for
a CheckTx execution response only.

##### Defined in

lumina\_node\_wasm.d.ts:848

***

#### Unspecified

> `readonly` `static` **Unspecified**: [`BroadcastMode`](#classesbroadcastmodemd)

zero-value for mode ordering

##### Defined in

lumina\_node\_wasm.d.ts:838

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:834


<a name="classescoinmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Coin

## Class: Coin

Coin defines a token with a denomination and an amount.

### Properties

#### amount

> **amount**: `bigint`

Coin amount

##### Defined in

lumina\_node\_wasm.d.ts:417

***

#### denom

> **denom**: `string`

Coin denomination

##### Defined in

lumina\_node\_wasm.d.ts:416

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:860


<a name="classescommitmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Commit

## Class: Commit

Commit contains the justification (ie. a set of signatures) that a block was
committed by a set of validators.

### Properties

#### block\_id

> **block\_id**: [`BlockId`](#classesblockidmd)

Block ID

##### Defined in

lumina\_node\_wasm.d.ts:888

***

#### height

> **height**: `bigint`

Block height

##### Defined in

lumina\_node\_wasm.d.ts:880

***

#### round

> **round**: `number`

Round

##### Defined in

lumina\_node\_wasm.d.ts:884

***

#### signatures

> **signatures**: [`CommitSig`](#classescommitsigmd)[]

Signatures

##### Defined in

lumina\_node\_wasm.d.ts:892

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:876


<a name="classescommitsigmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / CommitSig

## Class: CommitSig

CommitSig represents a signature of a validator. It’s a part of the Commit and can
be used to reconstruct the vote set given the validator set.

### Properties

#### vote\_type

> **vote\_type**: [`CommitVoteType`](#enumerationscommitvotetypemd)

vote type of a validator

##### Defined in

lumina\_node\_wasm.d.ts:904

### Accessors

#### vote

##### Get Signature

> **get** **vote**(): [`CommitVote`](#classescommitvotemd)

vote, if received

###### Returns

[`CommitVote`](#classescommitvotemd)

##### Set Signature

> **set** **vote**(`value`): `void`

vote, if received

###### Parameters

####### value

[`CommitVote`](#classescommitvotemd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:908

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:900


<a name="classescommitvotemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / CommitVote

## Class: CommitVote

Value of the validator vote

### Properties

#### timestamp

> **timestamp**: `string`

Timestamp

##### Defined in

lumina\_node\_wasm.d.ts:927

***

#### validator\_address

> **validator\_address**: `string`

Address of the voting validator

##### Defined in

lumina\_node\_wasm.d.ts:923

### Accessors

#### signature

##### Get Signature

> **get** **signature**(): [`Signature`](#classessignaturemd)

Signature

###### Returns

[`Signature`](#classessignaturemd)

##### Set Signature

> **set** **signature**(`value`): `void`

Signature

###### Parameters

####### value

[`Signature`](#classessignaturemd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:931

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:919


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

lumina\_node\_wasm.d.ts:985

***

#### hash()

> **hash**(): `Uint8Array`\<`ArrayBuffer`\>

Hash of the commitment

##### Returns

`Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:989

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:980

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:984


<a name="classesconflictingblockmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ConflictingBlock

## Class: ConflictingBlock

Conflicting block detected in light client attack

### Properties

#### signed\_header

> **signed\_header**: [`SignedHeader`](#classessignedheadermd)

Signed header

##### Defined in

lumina\_node\_wasm.d.ts:1000

***

#### validator\_set

> **validator\_set**: [`ValidatorSet`](#classesvalidatorsetmd)

Validator set

##### Defined in

lumina\_node\_wasm.d.ts:1004

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:996


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

lumina\_node\_wasm.d.ts:1020

***

#### num\_established

> **num\_established**: `number`

The number of outgoing connections being established.

##### Defined in

lumina\_node\_wasm.d.ts:1036

***

#### num\_established\_incoming

> **num\_established\_incoming**: `number`

The number of established incoming connections.

##### Defined in

lumina\_node\_wasm.d.ts:1040

***

#### num\_established\_outgoing

> **num\_established\_outgoing**: `number`

The number of established outgoing connections.

##### Defined in

lumina\_node\_wasm.d.ts:1044

***

#### num\_pending

> **num\_pending**: `number`

The total number of pending connections, both incoming and outgoing.

##### Defined in

lumina\_node\_wasm.d.ts:1024

***

#### num\_pending\_incoming

> **num\_pending\_incoming**: `number`

The total number of pending connections, both incoming and outgoing.

##### Defined in

lumina\_node\_wasm.d.ts:1028

***

#### num\_pending\_outgoing

> **num\_pending\_outgoing**: `number`

The number of outgoing connections being established.

##### Defined in

lumina\_node\_wasm.d.ts:1032

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1016

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1011

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1015


<a name="classesconsaddressmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ConsAddress

## Class: ConsAddress

Address of a consensus node.

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1059

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1054

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1058


<a name="classesdatamd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Data

## Class: Data

Data contained in a [`Block`].

[`Block`]: crate::block::Block

### Properties

#### hash

> **hash**: `Uint8Array`\<`ArrayBuffer`\>

Hash is the root of a binary Merkle tree where the leaves of the tree are
the row and column roots of an extended data square. Hash is often referred
to as the "data root".

##### Defined in

lumina\_node\_wasm.d.ts:1078

***

#### square\_size

> **square\_size**: `bigint`

Square width of original data square.

##### Defined in

lumina\_node\_wasm.d.ts:1072

***

#### transactions

> `readonly` **transactions**: `Uint8Array`\<`ArrayBuffer`\>[]

Transactions

##### Defined in

lumina\_node\_wasm.d.ts:1082

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1068


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

lumina\_node\_wasm.d.ts:1146

***

#### columnRoots()

> **columnRoots**(): `any`[]

Merkle roots of the [`ExtendedDataSquare`] columns.

##### Returns

`any`[]

##### Defined in

lumina\_node\_wasm.d.ts:1138

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1130

***

#### hash()

> **hash**(): `any`

Compute the combined hash of all rows and columns.

This is the data commitment for the block.

##### Returns

`any`

##### Defined in

lumina\_node\_wasm.d.ts:1152

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

lumina\_node\_wasm.d.ts:1142

***

#### rowRoots()

> **rowRoots**(): `any`[]

Merkle roots of the [`ExtendedDataSquare`] rows.

##### Returns

`any`[]

##### Defined in

lumina\_node\_wasm.d.ts:1134

***

#### squareWidth()

> **squareWidth**(): `number`

Get the size of the [`ExtendedDataSquare`] for which this header was built.

##### Returns

`number`

##### Defined in

lumina\_node\_wasm.d.ts:1156

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1125

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1129


<a name="classesduplicatevoteevidencemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / DuplicateVoteEvidence

## Class: DuplicateVoteEvidence

Duplicate vote evidence

### Properties

#### timestamp

> **timestamp**: `string`

Timestamp

##### Defined in

lumina\_node\_wasm.d.ts:1183

***

#### total\_voting\_power

> **total\_voting\_power**: `bigint`

Total voting power

##### Defined in

lumina\_node\_wasm.d.ts:1175

***

#### validator\_power

> **validator\_power**: `bigint`

Validator power

##### Defined in

lumina\_node\_wasm.d.ts:1179

***

#### vote\_a

> **vote\_a**: [`Vote`](#classesvotemd)

Vote A

##### Defined in

lumina\_node\_wasm.d.ts:1167

***

#### vote\_b

> **vote\_b**: [`Vote`](#classesvotemd)

Vote B

##### Defined in

lumina\_node\_wasm.d.ts:1171

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1163


<a name="classesevidencemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Evidence

## Class: Evidence

Evidence of malfeasance by validators (i.e. signing conflicting votes or light client attack).

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1190


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

lumina\_node\_wasm.d.ts:1321

***

#### dah

> **dah**: [`DataAvailabilityHeader`](#classesdataavailabilityheadermd)

Header of the block data availability.

##### Defined in

lumina\_node\_wasm.d.ts:1313

***

#### header

> `readonly` **header**: `any`

Tendermint block header.

##### Defined in

lumina\_node\_wasm.d.ts:1317

***

#### validatorSet

> `readonly` **validatorSet**: `any`

Information about the set of validators commiting the block.

##### Defined in

lumina\_node\_wasm.d.ts:1325

### Methods

#### clone()

> **clone**(): [`ExtendedHeader`](#classesextendedheadermd)

Clone a header producing a deep copy of it.

##### Returns

[`ExtendedHeader`](#classesextendedheadermd)

##### Defined in

lumina\_node\_wasm.d.ts:1235

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1231

***

#### hash()

> **hash**(): `string`

Get the block hash.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1247

***

#### height()

> **height**(): `bigint`

Get the block height.

##### Returns

`bigint`

##### Defined in

lumina\_node\_wasm.d.ts:1239

***

#### previousHeaderHash()

> **previousHeaderHash**(): `string`

Get the hash of the previous header.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1251

***

#### time()

> **time**(): `number`

Get the block time.

##### Returns

`number`

##### Defined in

lumina\_node\_wasm.d.ts:1243

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1226

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1230

***

#### validate()

> **validate**(): `void`

Decode protobuf encoded header and then validate it.

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1255

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

lumina\_node\_wasm.d.ts:1265

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

lumina\_node\_wasm.d.ts:1309

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

lumina\_node\_wasm.d.ts:1287


<a name="classesfeemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Fee

## Class: Fee

Fee includes the amount of coins paid in fees and the maximum
gas to be used by the transaction. The ratio yields an effective "gasprice",
which must be above some miminum to be accepted into the mempool.

### Properties

#### amount

> **amount**: [`Coin`](#classescoinmd)[]

amount is the amount of coins to be paid as a fee

##### Defined in

lumina\_node\_wasm.d.ts:1338

***

#### gas\_limit

> **gas\_limit**: `bigint`

gas_limit is the maximum gas that can be used in transaction processing
before an out of gas error occurs

##### Defined in

lumina\_node\_wasm.d.ts:1343

***

#### granter

> `readonly` **granter**: `string`

if set, the fee payer (either the first signer or the value of the payer field) requests that a fee grant be used
to pay fees instead of the fee payer's own balance. If an appropriate fee grant does not exist or the chain does
not support fee grants, this will fail

##### Defined in

lumina\_node\_wasm.d.ts:1355

***

#### payer

> `readonly` **payer**: `string`

if unset, the first signer is responsible for paying the fees. If set, the specified account must pay the fees.
the payer must be a tx signer (and thus have signed this field in AuthInfo).
setting this field does *not* change the ordering of required signers for the transaction.

##### Defined in

lumina\_node\_wasm.d.ts:1349

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1334


<a name="classesgasestimatemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / GasEstimate

## Class: GasEstimate

Result of gas price and usage estimation

### Properties

#### price

> **price**: `number`

Gas price estimated based on last 5 blocks

##### Defined in

lumina\_node\_wasm.d.ts:1366

***

#### usage

> **usage**: `bigint`

Simulated transaction gas usage

##### Defined in

lumina\_node\_wasm.d.ts:1370

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1362


<a name="classesgasinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / GasInfo

## Class: GasInfo

GasInfo defines tx execution gas context.

### Properties

#### gas\_used

> **gas\_used**: `bigint`

GasUsed is the amount of gas actually consumed.

##### Defined in

lumina\_node\_wasm.d.ts:1385

***

#### gas\_wanted

> **gas\_wanted**: `bigint`

GasWanted is the maximum units of work we allow this tx to perform.

##### Defined in

lumina\_node\_wasm.d.ts:1381

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1377


<a name="classesgettxresponsemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / GetTxResponse

## Class: GetTxResponse

Response to GetTx

### Properties

#### tx

> **tx**: [`Tx`](#classestxmd)

Response Transaction

##### Defined in

lumina\_node\_wasm.d.ts:1396

***

#### tx\_response

> **tx\_response**: [`TxResponse`](#classestxresponsemd)

TxResponse to a Query

##### Defined in

lumina\_node\_wasm.d.ts:1400

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1392


<a name="classesgrpcclientmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / GrpcClient

## Class: GrpcClient

Celestia GRPC client

### Methods

#### abci\_query()

> **abci\_query**(`data`, `path`, `height`, `prove`): `Promise`\<[`AbciQueryResponse`](#classesabciqueryresponsemd)\>

Issue a direct ABCI query to the application

##### Parameters

###### data

`Uint8Array`\<`ArrayBuffer`\>

###### path

`string`

###### height

`bigint`

###### prove

`boolean`

##### Returns

`Promise`\<[`AbciQueryResponse`](#classesabciqueryresponsemd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1464

***

#### broadcast\_tx()

> **broadcast\_tx**(`tx_bytes`, `mode`): `Promise`\<[`TxResponse`](#classestxresponsemd)\>

Broadcast prepared and serialised transaction

##### Parameters

###### tx\_bytes

`Uint8Array`\<`ArrayBuffer`\>

###### mode

[`BroadcastMode`](#classesbroadcastmodemd)

##### Returns

`Promise`\<[`TxResponse`](#classestxresponsemd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1468

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1407

***

#### get\_account()

> **get\_account**(`account`): `Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)\>

Get account

##### Parameters

###### account

`string`

##### Returns

`Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1419

***

#### get\_accounts()

> **get\_accounts**(): `Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)[]\>

Get accounts

##### Returns

`Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1423

***

#### get\_all\_balances()

> **get\_all\_balances**(`address`): `Promise`\<[`Coin`](#classescoinmd)[]\>

Get balance of all coins

##### Parameters

###### address

`string`

##### Returns

`Promise`\<[`Coin`](#classescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1440

***

#### get\_auth\_params()

> **get\_auth\_params**(): `Promise`\<[`AuthParams`](#interfacesauthparamsmd)\>

Get auth params

##### Returns

`Promise`\<[`AuthParams`](#interfacesauthparamsmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1415

***

#### get\_balance()

> **get\_balance**(`address`, `denom`): `Promise`\<[`Coin`](#classescoinmd)\>

Get balance of coins with given denom

##### Parameters

###### address

`string`

###### denom

`string`

##### Returns

`Promise`\<[`Coin`](#classescoinmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1436

***

#### get\_blob\_params()

> **get\_blob\_params**(): `Promise`\<[`BlobParams`](#classesblobparamsmd)\>

Get blob params

##### Returns

`Promise`\<[`BlobParams`](#classesblobparamsmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1480

***

#### get\_block\_by\_height()

> **get\_block\_by\_height**(`height`): `Promise`\<[`Block`](#classesblockmd)\>

Get block by height

##### Parameters

###### height

`bigint`

##### Returns

`Promise`\<[`Block`](#classesblockmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1460

***

#### get\_latest\_block()

> **get\_latest\_block**(): `Promise`\<[`Block`](#classesblockmd)\>

Get latest block

##### Returns

`Promise`\<[`Block`](#classesblockmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1456

***

#### get\_min\_gas\_price()

> **get\_min\_gas\_price**(): `Promise`\<`number`\>

Get Minimum Gas price

##### Returns

`Promise`\<`number`\>

##### Defined in

lumina\_node\_wasm.d.ts:1452

***

#### get\_spendable\_balances()

> **get\_spendable\_balances**(`address`): `Promise`\<[`Coin`](#classescoinmd)[]\>

Get balance of all spendable coins

##### Parameters

###### address

`string`

##### Returns

`Promise`\<[`Coin`](#classescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1444

***

#### get\_total\_supply()

> **get\_total\_supply**(): `Promise`\<[`Coin`](#classescoinmd)[]\>

Get total supply

##### Returns

`Promise`\<[`Coin`](#classescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1448

***

#### get\_tx()

> **get\_tx**(`hash`): `Promise`\<[`GetTxResponse`](#classesgettxresponsemd)\>

Get Tx

##### Parameters

###### hash

`string`

##### Returns

`Promise`\<[`GetTxResponse`](#classesgettxresponsemd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1472

***

#### get\_verified\_balance()

> **get\_verified\_balance**(`address`, `header`): `Promise`\<[`Coin`](#classescoinmd)\>

Get balance of coins with bond denom for the given address, together with a proof,
and verify the returned balance against the corresponding block's app hash.

NOTE: the balance returned is the balance reported by the parent block of
the provided header. This is due to the fact that for block N, the block's
app hash is the result of applying the previous block's transaction list.

##### Parameters

###### address

`string`

###### header

[`ExtendedHeader`](#classesextendedheadermd)

##### Returns

`Promise`\<[`Coin`](#classescoinmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1432

***

#### simulate()

> **simulate**(`tx_bytes`): `Promise`\<[`GasInfo`](#classesgasinfomd)\>

Simulate prepared and serialised transaction, returning simulated gas usage

##### Parameters

###### tx\_bytes

`Uint8Array`\<`ArrayBuffer`\>

##### Returns

`Promise`\<[`GasInfo`](#classesgasinfomd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1476

***

#### tx\_status()

> **tx\_status**(`hash`): `Promise`\<[`TxStatusResponse`](#classestxstatusresponsemd)\>

Get status of the transaction

##### Parameters

###### hash

`string`

##### Returns

`Promise`\<[`TxStatusResponse`](#classestxstatusresponsemd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1484

***

#### new()

> `static` **new**(`url`): `Promise`\<[`GrpcClient`](#classesgrpcclientmd)\>

Create a new client connected with the given `url`

##### Parameters

###### url

`string`

##### Returns

`Promise`\<[`GrpcClient`](#classesgrpcclientmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1411


<a name="classesheadermd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Header

## Class: Header

Block Header values contain metadata about the block and about the consensus,
as well as commitments to the data in the current block, the previous block,
and the results returned by the application.

### Properties

#### app\_hash

> **app\_hash**: `string`

State after txs from the previous block

##### Defined in

lumina\_node\_wasm.d.ts:1549

***

#### chain\_id

> **chain\_id**: `string`

Chain ID

##### Defined in

lumina\_node\_wasm.d.ts:1501

***

#### consensus\_hash

> **consensus\_hash**: `string`

Consensus params for the current block

##### Defined in

lumina\_node\_wasm.d.ts:1545

***

#### height

> **height**: `bigint`

Current block height

##### Defined in

lumina\_node\_wasm.d.ts:1505

***

#### next\_validators\_hash

> **next\_validators\_hash**: `string`

Validators for the next block

##### Defined in

lumina\_node\_wasm.d.ts:1541

***

#### proposer\_address

> **proposer\_address**: `string`

Original proposer of the block

##### Defined in

lumina\_node\_wasm.d.ts:1569

***

#### time

> **time**: `string`

Current timestamp encoded as rfc3339

##### Defined in

lumina\_node\_wasm.d.ts:1509

***

#### validators\_hash

> **validators\_hash**: `string`

Validators for the current block

##### Defined in

lumina\_node\_wasm.d.ts:1537

***

#### version

> **version**: [`ProtocolVersion`](#classesprotocolversionmd)

Header version

##### Defined in

lumina\_node\_wasm.d.ts:1497

### Accessors

#### data\_hash

##### Get Signature

> **get** **data\_hash**(): `string`

Merkle root of transaction hashes

###### Returns

`string`

##### Set Signature

> **set** **data\_hash**(`value`): `void`

Merkle root of transaction hashes

###### Parameters

####### value

`string`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1529

***

#### evidence\_hash

##### Get Signature

> **get** **evidence\_hash**(): `string`

Hash of evidence included in the block

###### Returns

`string`

##### Set Signature

> **set** **evidence\_hash**(`value`): `void`

Hash of evidence included in the block

###### Parameters

####### value

`string`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1561

***

#### last\_block\_id

##### Get Signature

> **get** **last\_block\_id**(): [`BlockId`](#classesblockidmd)

Previous block info

###### Returns

[`BlockId`](#classesblockidmd)

##### Set Signature

> **set** **last\_block\_id**(`value`): `void`

Previous block info

###### Parameters

####### value

[`BlockId`](#classesblockidmd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1513

***

#### last\_commit\_hash

##### Get Signature

> **get** **last\_commit\_hash**(): `string`

Commit from validators from the last block

###### Returns

`string`

##### Set Signature

> **set** **last\_commit\_hash**(`value`): `void`

Commit from validators from the last block

###### Parameters

####### value

`string`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1521

***

#### last\_results\_hash

##### Get Signature

> **get** **last\_results\_hash**(): `string`

Root hash of all results from the txs from the previous block

###### Returns

`string`

##### Set Signature

> **set** **last\_results\_hash**(`value`): `void`

Root hash of all results from the txs from the previous block

###### Parameters

####### value

`string`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1553

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1493


<a name="classesintounderlyingbytesourcemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / IntoUnderlyingByteSource

## Class: IntoUnderlyingByteSource

### Properties

#### autoAllocateChunkSize

> `readonly` **autoAllocateChunkSize**: `number`

##### Defined in

lumina\_node\_wasm.d.ts:1578

***

#### type

> `readonly` **type**: `"bytes"`

##### Defined in

lumina\_node\_wasm.d.ts:1577

### Methods

#### cancel()

> **cancel**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1576

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1573

***

#### pull()

> **pull**(`controller`): `Promise`\<`any`\>

##### Parameters

###### controller

`ReadableByteStreamController`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:1575

***

#### start()

> **start**(`controller`): `void`

##### Parameters

###### controller

`ReadableByteStreamController`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1574


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

lumina\_node\_wasm.d.ts:1585

***

#### close()

> **close**(): `Promise`\<`any`\>

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:1584

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1582

***

#### write()

> **write**(`chunk`): `Promise`\<`any`\>

##### Parameters

###### chunk

`any`

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:1583


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

lumina\_node\_wasm.d.ts:1591

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1589

***

#### pull()

> **pull**(`controller`): `Promise`\<`any`\>

##### Parameters

###### controller

`ReadableStreamDefaultController`\<`any`\>

##### Returns

`Promise`\<`any`\>

##### Defined in

lumina\_node\_wasm.d.ts:1590


<a name="classesjsbitvectormd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / JsBitVector

## Class: JsBitVector

Array of bits

### Properties

#### 0

> **0**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:1599

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1598


<a name="classesjseventmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / JsEvent

## Class: JsEvent

Event allows application developers to attach additional information to
ResponseBeginBlock, ResponseEndBlock, ResponseCheckTx and ResponseDeliverTx.
Later, transactions may be queried using these events.

### Properties

#### attributes

> **attributes**: [`JsEventAttribute`](#classesjseventattributemd)[]

##### Defined in

lumina\_node\_wasm.d.ts:1610

***

#### type

> **type**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:1609

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1608


<a name="classesjseventattributemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / JsEventAttribute

## Class: JsEventAttribute

### Properties

#### index

> **index**: `boolean`

##### Defined in

lumina\_node\_wasm.d.ts:1617

***

#### key

> **key**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:1615

***

#### value

> **value**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:1616

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1614


<a name="classesjsvalidatorinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / JsValidatorInfo

## Class: JsValidatorInfo

Validator information

### Properties

#### address

> **address**: `string`

Validator account address

##### Defined in

lumina\_node\_wasm.d.ts:1628

***

#### power

> **power**: `bigint`

Validator voting power

##### Defined in

lumina\_node\_wasm.d.ts:1636

***

#### proposer\_priority

> **proposer\_priority**: `bigint`

Validator proposer priority

##### Defined in

lumina\_node\_wasm.d.ts:1648

***

#### pub\_key

> **pub\_key**: [`PublicKey`](#interfacespublickeymd)

Validator public key

##### Defined in

lumina\_node\_wasm.d.ts:1632

### Accessors

#### name

##### Get Signature

> **get** **name**(): `string`

Validator name

###### Returns

`string`

##### Set Signature

> **set** **name**(`value`): `void`

Validator name

###### Parameters

####### value

`string`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1640

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1624


<a name="classeslightclientattackevidencemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / LightClientAttackEvidence

## Class: LightClientAttackEvidence

LightClient attack evidence

### Properties

#### byzantine\_validators

> **byzantine\_validators**: [`JsValidatorInfo`](#classesjsvalidatorinfomd)[]

Byzantine validators

##### Defined in

lumina\_node\_wasm.d.ts:1667

***

#### common\_height

> **common\_height**: `bigint`

Common height

##### Defined in

lumina\_node\_wasm.d.ts:1663

***

#### conflicting\_block

> **conflicting\_block**: [`ConflictingBlock`](#classesconflictingblockmd)

Conflicting block

##### Defined in

lumina\_node\_wasm.d.ts:1659

***

#### timestamp

> **timestamp**: `string`

Timestamp

##### Defined in

lumina\_node\_wasm.d.ts:1675

***

#### total\_voting\_power

> **total\_voting\_power**: `bigint`

Total voting power

##### Defined in

lumina\_node\_wasm.d.ts:1671

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1655


<a name="classesmodeinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ModeInfo

## Class: ModeInfo

ModeInfo describes the signing mode of a single or nested multisig signer.

### Properties

#### bitarray

> `readonly` **bitarray**: [`JsBitVector`](#classesjsbitvectormd)

Multi is the mode info for a multisig public key
bitarray specifies which keys within the multisig are signing

##### Defined in

lumina\_node\_wasm.d.ts:1697

***

#### mode

> `readonly` **mode**: `number`

Single is the mode info for a single signer. It is structured as a message
to allow for additional fields such as locale for SIGN_MODE_TEXTUAL in the
future

##### Defined in

lumina\_node\_wasm.d.ts:1692

***

#### mode\_infos

> `readonly` **mode\_infos**: [`ModeInfo`](#classesmodeinfomd)[]

Multi is the mode info for a multisig public key
mode_infos is the corresponding modes of the signers of the multisig
which could include nested multisig public keys

##### Defined in

lumina\_node\_wasm.d.ts:1703

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1682

***

#### signature\_mode()

> **signature\_mode**(): [`SignatureMode`](#enumerationssignaturemodemd)

Return signature mode for the stored signature(s)

##### Returns

[`SignatureMode`](#enumerationssignaturemodemd)

##### Defined in

lumina\_node\_wasm.d.ts:1686


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

lumina\_node\_wasm.d.ts:1812

***

#### version

> `readonly` **version**: `number`

Returns the first byte indicating the version of the [`Namespace`].

##### Defined in

lumina\_node\_wasm.d.ts:1808

***

#### MAX\_PRIMARY\_RESERVED

> `readonly` `static` **MAX\_PRIMARY\_RESERVED**: [`Namespace`](#classesnamespacemd)

Maximal primary reserved [`Namespace`].

Used to indicate the end of the primary reserved group.

##### Defined in

lumina\_node\_wasm.d.ts:1783

***

#### MIN\_SECONDARY\_RESERVED

> `readonly` `static` **MIN\_SECONDARY\_RESERVED**: [`Namespace`](#classesnamespacemd)

Minimal secondary reserved [`Namespace`].

Used to indicate the beginning of the secondary reserved group.

##### Defined in

lumina\_node\_wasm.d.ts:1789

***

#### NS\_SIZE

> `readonly` `static` **NS\_SIZE**: `number`

Namespace size in bytes.

##### Defined in

lumina\_node\_wasm.d.ts:1762

***

#### PARITY\_SHARE

> `readonly` `static` **PARITY\_SHARE**: [`Namespace`](#classesnamespacemd)

The [`Namespace`] for `parity shares`.

It is the namespace with which all the `parity shares` from
`ExtendedDataSquare` are inserted to the `Nmt` when computing
merkle roots.

##### Defined in

lumina\_node\_wasm.d.ts:1804

***

#### PAY\_FOR\_BLOB

> `readonly` `static` **PAY\_FOR\_BLOB**: [`Namespace`](#classesnamespacemd)

Primary reserved [`Namespace`] for the compact Shares with MsgPayForBlobs transactions.

##### Defined in

lumina\_node\_wasm.d.ts:1770

***

#### PRIMARY\_RESERVED\_PADDING

> `readonly` `static` **PRIMARY\_RESERVED\_PADDING**: [`Namespace`](#classesnamespacemd)

Primary reserved [`Namespace`] for the `Share`s used for padding.

`Share`s with this namespace are inserted after other shares from primary reserved namespace
so that user-defined namespaces are correctly aligned in `ExtendedDataSquare`

##### Defined in

lumina\_node\_wasm.d.ts:1777

***

#### TAIL\_PADDING

> `readonly` `static` **TAIL\_PADDING**: [`Namespace`](#classesnamespacemd)

Secondary reserved [`Namespace`] used for padding after the blobs.

It is used to fill up the `original data square` after all user-submitted
blobs before the parity data is generated for the `ExtendedDataSquare`.

##### Defined in

lumina\_node\_wasm.d.ts:1796

***

#### TRANSACTION

> `readonly` `static` **TRANSACTION**: [`Namespace`](#classesnamespacemd)

Primary reserved [`Namespace`] for the compact `Share`s with `cosmos SDK` transactions.

##### Defined in

lumina\_node\_wasm.d.ts:1766

### Methods

#### asBytes()

> **asBytes**(): `Uint8Array`\<`ArrayBuffer`\>

Converts the [`Namespace`] to a byte slice.

##### Returns

`Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:1758

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1736

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1731

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1735

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

lumina\_node\_wasm.d.ts:1754

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

lumina\_node\_wasm.d.ts:1744


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

lumina\_node\_wasm.d.ts:1835

***

#### num\_peers

> **num\_peers**: `number`

The number of connected peers, i.e. peers with whom at least one established connection exists.

##### Defined in

lumina\_node\_wasm.d.ts:1831

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1827

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1822

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1826


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

lumina\_node\_wasm.d.ts:1849

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

lumina\_node\_wasm.d.ts:1853

***

#### connectedPeers()

> **connectedPeers**(): `Promise`\<`any`[]\>

Get all the peers that node is connected to.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1890

***

#### eventsChannel()

> **eventsChannel**(): `Promise`\<`BroadcastChannel`\>

Returns a [`BroadcastChannel`] for events generated by [`Node`].

##### Returns

`Promise`\<`BroadcastChannel`\>

##### Defined in

lumina\_node\_wasm.d.ts:1957

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1844

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

lumina\_node\_wasm.d.ts:1933

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

lumina\_node\_wasm.d.ts:1937

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

lumina\_node\_wasm.d.ts:1949

***

#### getLocalHeadHeader()

> **getLocalHeadHeader**(): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Get the latest locally synced header.

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1929

***

#### getNetworkHeadHeader()

> **getNetworkHeadHeader**(): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Get the latest header announced in the network.

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1925

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

lumina\_node\_wasm.d.ts:1953

***

#### isRunning()

> **isRunning**(): `Promise`\<`boolean`\>

Check whether Lumina is currently running

##### Returns

`Promise`\<`boolean`\>

##### Defined in

lumina\_node\_wasm.d.ts:1857

***

#### listeners()

> **listeners**(): `Promise`\<`any`[]\>

Get all the multiaddresses on which the node listens.

##### Returns

`Promise`\<`any`[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1886

***

#### localPeerId()

> **localPeerId**(): `Promise`\<`string`\>

Get node's local peer ID.

##### Returns

`Promise`\<`string`\>

##### Defined in

lumina\_node\_wasm.d.ts:1866

***

#### networkInfo()

> **networkInfo**(): `Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

Get current network info.

##### Returns

`Promise`\<[`NetworkInfoSnapshot`](#classesnetworkinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1882

***

#### peerTrackerInfo()

> **peerTrackerInfo**(): `Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

Get current [`PeerTracker`] info.

##### Returns

`Promise`\<[`PeerTrackerInfoSnapshot`](#classespeertrackerinfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1870

***

#### requestAllBlobs()

> **requestAllBlobs**(`namespace`, `block_height`, `timeout_secs`?): `Promise`\<[`Blob`](#classesblobmd)[]\>

Request all blobs with provided namespace in the block corresponding to this header
using bitswap protocol.

##### Parameters

###### namespace

[`Namespace`](#classesnamespacemd)

###### block\_height

`bigint`

###### timeout\_secs?

`number`

##### Returns

`Promise`\<[`Blob`](#classesblobmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:1917

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

lumina\_node\_wasm.d.ts:1902

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

lumina\_node\_wasm.d.ts:1906

***

#### requestHeadHeader()

> **requestHeadHeader**(): `Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

Request the head header from the network.

##### Returns

`Promise`\<[`ExtendedHeader`](#classesextendedheadermd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1898

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

lumina\_node\_wasm.d.ts:1912

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

lumina\_node\_wasm.d.ts:1894

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

lumina\_node\_wasm.d.ts:1861

***

#### stop()

> **stop**(): `Promise`\<`void`\>

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:1862

***

#### syncerInfo()

> **syncerInfo**(): `Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

Get current header syncing info.

##### Returns

`Promise`\<[`SyncingInfoSnapshot`](#classessyncinginfosnapshotmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:1921

***

#### waitConnected()

> **waitConnected**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:1874

***

#### waitConnectedTrusted()

> **waitConnectedTrusted**(): `Promise`\<`void`\>

Wait until the node is connected to at least 1 trusted peer.

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:1878


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

lumina\_node\_wasm.d.ts:1984

***

#### network

> **network**: [`Network`](#enumerationsnetworkmd)

A network to connect to.

##### Defined in

lumina\_node\_wasm.d.ts:1980

***

#### usePersistentMemory

> **usePersistentMemory**: `boolean`

Whether to store data in persistent memory or not.

**Default value:** true

##### Defined in

lumina\_node\_wasm.d.ts:1990

### Accessors

#### customPruningWindowSecs

##### Get Signature

> **get** **customPruningWindowSecs**(): `number`

Pruning window defines maximum age of a block for it to be retained in store.

If pruning window is smaller than sampling window, then blocks will be pruned
right after they are sampled. This is useful when you want to keep low
memory footprint but still validate the blockchain.

If this is not set, then default value will apply:

* If `use_persistent_memory == true`, default value is 7 days plus 1 hour.
* If `use_persistent_memory == false`, default value is 0 seconds.

###### Returns

`number`

##### Set Signature

> **set** **customPruningWindowSecs**(`value`): `void`

Pruning window defines maximum age of a block for it to be retained in store.

If pruning window is smaller than sampling window, then blocks will be pruned
right after they are sampled. This is useful when you want to keep low
memory footprint but still validate the blockchain.

If this is not set, then default value will apply:

* If `use_persistent_memory == true`, default value is 7 days plus 1 hour.
* If `use_persistent_memory == false`, default value is 0 seconds.

###### Parameters

####### value

`number`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2003

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:1972

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:1967

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:1971

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

lumina\_node\_wasm.d.ts:1976


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

lumina\_node\_wasm.d.ts:2026

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2025

***

#### run()

> **run**(): `Promise`\<`void`\>

##### Returns

`Promise`\<`void`\>

##### Defined in

lumina\_node\_wasm.d.ts:2027


<a name="classespartsheadermd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / PartsHeader

## Class: PartsHeader

Block parts header

### Properties

#### hash

> **hash**: `string`

Hash of the parts set header

##### Defined in

lumina\_node\_wasm.d.ts:2042

***

#### total

> **total**: `number`

Number of parts in this block

##### Defined in

lumina\_node\_wasm.d.ts:2038

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2034


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

lumina\_node\_wasm.d.ts:2061

***

#### num\_connected\_trusted\_peers

> **num\_connected\_trusted\_peers**: `bigint`

Number of the connected trusted peers.

##### Defined in

lumina\_node\_wasm.d.ts:2065

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2057

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:2052

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:2056


<a name="classesproofopmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ProofOp

## Class: ProofOp

ProofOp defines an operation used for calculating Merkle root. The data could
be arbitrary format, providing nessecary data for example neighbouring node
hash.

Note: This type is a duplicate of the ProofOp proto type defined in
Tendermint.

### Properties

#### data

> **data**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:2080

***

#### key

> **key**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:2079

***

#### type

> **type**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:2078

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2077


<a name="classesproofopsmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ProofOps

## Class: ProofOps

ProofOps is Merkle proof defined by the list of ProofOps.

Note: This type is a duplicate of the ProofOps proto type defined in
Tendermint.

### Properties

#### ops

> **ops**: [`ProofOp`](#classesproofopmd)[]

##### Defined in

lumina\_node\_wasm.d.ts:2091

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2090


<a name="classesprotocolversionmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ProtocolVersion

## Class: ProtocolVersion

Version contains the protocol version for the blockchain and the application.

### Properties

#### app

> **app**: `bigint`

app version

##### Defined in

lumina\_node\_wasm.d.ts:2106

***

#### block

> **block**: `bigint`

blockchain version

##### Defined in

lumina\_node\_wasm.d.ts:2102

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2098


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

lumina\_node\_wasm.d.ts:2119

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2115


<a name="classessignaturemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Signature

## Class: Signature

Signature

### Properties

#### 0

> **0**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:2127

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2126


<a name="classessignedheadermd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SignedHeader

## Class: SignedHeader

Signed block headers

### Properties

#### commit

> **commit**: [`Commit`](#classescommitmd)

Commit containing signatures for the header

##### Defined in

lumina\_node\_wasm.d.ts:2142

***

#### header

> **header**: [`Header`](#classesheadermd)

Signed block headers

##### Defined in

lumina\_node\_wasm.d.ts:2138

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2134


<a name="classessignerinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SignerInfo

## Class: SignerInfo

SignerInfo describes the public key and signing mode of a single top-level
signer.

### Properties

#### mode\_info

> **mode\_info**: [`ModeInfo`](#classesmodeinfomd)

mode_info describes the signing mode of the signer and is a nested
structure to support nested multisig pubkey's

##### Defined in

lumina\_node\_wasm.d.ts:2161

***

#### sequence

> **sequence**: `bigint`

sequence is the sequence of the account, which describes the
number of committed transactions signed by a given address. It is used to
prevent replay attacks.

##### Defined in

lumina\_node\_wasm.d.ts:2167

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2150

***

#### public\_key()

> **public\_key**(): [`ProtoAny`](#interfacesprotoanymd)

public_key is the public key of the signer. It is optional for accounts
that already exist in state. If unset, the verifier can use the required \
signer address for this position and lookup the public key.

##### Returns

[`ProtoAny`](#interfacesprotoanymd)

##### Defined in

lumina\_node\_wasm.d.ts:2156


<a name="classesstringeventmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / StringEvent

## Class: StringEvent

StringEvent defines en Event object wrapper where all the attributes
contain key/value pairs that are strings instead of raw bytes.

### Properties

#### attributes

> **attributes**: [`Attribute`](#classesattributemd)[]

##### Defined in

lumina\_node\_wasm.d.ts:2177

***

#### type

> **type**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:2176

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2175


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

lumina\_node\_wasm.d.ts:2196

***

#### subjective\_head

> **subjective\_head**: `bigint`

Syncing target. The latest height seen in the network that was successfully verified.

##### Defined in

lumina\_node\_wasm.d.ts:2200

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2192

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:2187

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:2191


<a name="classestxmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Tx

## Class: Tx

[`Tx`] is the standard type used for broadcasting transactions.

### Properties

#### auth\_info

> **auth\_info**: [`AuthInfo`](#classesauthinfomd)

Authorization related content of the transaction, specifically signers, signer modes
and [`Fee`].

##### Defined in

lumina\_node\_wasm.d.ts:2216

***

#### body

> **body**: [`TxBody`](#classestxbodymd)

Processable content of the transaction

##### Defined in

lumina\_node\_wasm.d.ts:2211

***

#### signatures

> `readonly` **signatures**: [`Signature`](#classessignaturemd)[]

List of signatures that matches the length and order of [`AuthInfo`]’s `signer_info`s to
allow connecting signature meta information like public key and signing mode by position.

##### Defined in

lumina\_node\_wasm.d.ts:2221

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2207


<a name="classestxbodymd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxBody

## Class: TxBody

[`TxBody`] of a transaction that all signers sign over.

### Properties

#### memo

> **memo**: `string`

`memo` is any arbitrary memo to be added to the transaction.

##### Defined in

lumina\_node\_wasm.d.ts:2255

***

#### timeout\_height

> `readonly` **timeout\_height**: `bigint`

`timeout` is the block height after which this transaction will not
be processed by the chain

##### Defined in

lumina\_node\_wasm.d.ts:2260

### Methods

#### extension\_options()

> **extension\_options**(): [`ProtoAny`](#interfacesprotoanymd)[]

`extension_options` are arbitrary options that can be added by chains
when the default options are not sufficient. If any of these are present
and can't be handled, the transaction will be rejected

##### Returns

[`ProtoAny`](#interfacesprotoanymd)[]

##### Defined in

lumina\_node\_wasm.d.ts:2245

***

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2228

***

#### messages()

> **messages**(): [`ProtoAny`](#interfacesprotoanymd)[]

`messages` is a list of messages to be executed. The required signers of
those messages define the number and order of elements in `AuthInfo`'s
signer_infos and Tx's signatures. Each required signer address is added to
the list only the first time it occurs.

By convention, the first required signer (usually from the first message)
is referred to as the primary signer and pays the fee for the whole
transaction.

##### Returns

[`ProtoAny`](#interfacesprotoanymd)[]

##### Defined in

lumina\_node\_wasm.d.ts:2239

***

#### non\_critical\_extension\_options()

> **non\_critical\_extension\_options**(): [`ProtoAny`](#interfacesprotoanymd)[]

`extension_options` are arbitrary options that can be added by chains
when the default options are not sufficient. If any of these are present
and can't be handled, they will be ignored

##### Returns

[`ProtoAny`](#interfacesprotoanymd)[]

##### Defined in

lumina\_node\_wasm.d.ts:2251


<a name="classestxclientmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxClient

## Class: TxClient

Celestia grpc transaction client.

### Constructors

#### new TxClient()

> **new TxClient**(`url`, `pubkey`, `signer_fn`): [`TxClient`](#classestxclientmd)

Create a new transaction client with the specified account.

Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).

## Example with noble/curves
```js
import { secp256k1 } from "@noble/curves/secp256k1";

const privKey = "fdc8ac75dfa1c142dbcba77938a14dd03078052ce0b49a529dcf72a9885a3abb";
const pubKey = secp256k1.getPublicKey(privKey);

const signer = (signDoc) => {
  const bytes = protoEncodeSignDoc(signDoc);
  const sig = secp256k1.sign(bytes, privKey, { prehash: true });
  return sig.toCompactRawBytes();
};

const txClient = await new TxClient("http://127.0.0.1:18080", pubKey, signer);
```

## Example with leap wallet
```js
await window.leap.enable("mocha-4")
const keys = await window.leap.getKey("mocha-4")

const signer = (signDoc) => {
  return window.leap.signDirect("mocha-4", keys.bech32Address, signDoc, { preferNoSetFee: true })
    .then(sig => Uint8Array.from(atob(sig.signature.signature), c => c.charCodeAt(0)))
}

const tx_client = await new TxClient("http://127.0.0.1:18080", keys.pubKey, signer)
```

##### Parameters

###### url

`string`

###### pubkey

`Uint8Array`\<`ArrayBuffer`\>

###### signer\_fn

[`SignerFn`](#type-aliasessignerfnmd)

##### Returns

[`TxClient`](#classestxclientmd)

##### Defined in

lumina\_node\_wasm.d.ts:2301

### Properties

#### appVersion

> `readonly` **appVersion**: [`AppVersion`](#classesappversionmd)

AppVersion of the client

##### Defined in

lumina\_node\_wasm.d.ts:2404

***

#### chainId

> `readonly` **chainId**: `string`

Chain id of the client

##### Defined in

lumina\_node\_wasm.d.ts:2400

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2266

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

lumina\_node\_wasm.d.ts:2376

***

#### getAccounts()

> **getAccounts**(): `Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)[]\>

Get accounts

##### Returns

`Promise`\<[`BaseAccount`](#interfacesbaseaccountmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:2380

***

#### getAllBalances()

> **getAllBalances**(`address`): `Promise`\<[`Coin`](#classescoinmd)[]\>

Get balance of all coins

##### Parameters

###### address

`string`

##### Returns

`Promise`\<[`Coin`](#classescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:2388

***

#### getAuthParams()

> **getAuthParams**(): `Promise`\<[`AuthParams`](#interfacesauthparamsmd)\>

Get auth params

##### Returns

`Promise`\<[`AuthParams`](#interfacesauthparamsmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:2372

***

#### getBalance()

> **getBalance**(`address`, `denom`): `Promise`\<[`Coin`](#classescoinmd)\>

Get balance of coins with given denom

##### Parameters

###### address

`string`

###### denom

`string`

##### Returns

`Promise`\<[`Coin`](#classescoinmd)\>

##### Defined in

lumina\_node\_wasm.d.ts:2384

***

#### getEstimateGasPrice()

> **getEstimateGasPrice**(`priority`): `Promise`\<`number`\>

estimate_gas_price takes a transaction priority and estimates the gas price based
on the gas prices of the transactions in the last five blocks.

If no transaction is found in the last five blocks, return the network
min gas price.

##### Parameters

###### priority

[`TxPriority`](#enumerationstxprioritymd)

##### Returns

`Promise`\<`number`\>

##### Defined in

lumina\_node\_wasm.d.ts:2313

***

#### getSpendableBalances()

> **getSpendableBalances**(`address`): `Promise`\<[`Coin`](#classescoinmd)[]\>

Get balance of all spendable coins

##### Parameters

###### address

`string`

##### Returns

`Promise`\<[`Coin`](#classescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:2392

***

#### getTotalSupply()

> **getTotalSupply**(): `Promise`\<[`Coin`](#classescoinmd)[]\>

Get total supply

##### Returns

`Promise`\<[`Coin`](#classescoinmd)[]\>

##### Defined in

lumina\_node\_wasm.d.ts:2396

***

#### minGasPrice()

> **minGasPrice**(): `Promise`\<`number`\>

Query for the current minimum gas price

##### Returns

`Promise`\<`number`\>

##### Defined in

lumina\_node\_wasm.d.ts:2305

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
await txClient.submitBlobs([blob], { gasLimit: 100000n, gasPrice: 0.02, memo: "foo" });
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

lumina\_node\_wasm.d.ts:2342

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

lumina\_node\_wasm.d.ts:2368


<a name="classestxresponsemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxResponse

## Class: TxResponse

Response to a tx query

### Properties

#### code

> **code**: [`ErrorCode`](#enumerationserrorcodemd)

Response code.

##### Defined in

lumina\_node\_wasm.d.ts:2419

***

#### codespace

> **codespace**: `string`

Namespace for the Code

##### Defined in

lumina\_node\_wasm.d.ts:2415

***

#### data

> **data**: `string`

Result bytes, if any.

##### Defined in

lumina\_node\_wasm.d.ts:2423

***

#### events

> `readonly` **events**: [`JsEvent`](#classesjseventmd)[]

Events defines all the events emitted by processing a transaction. Note,
these events include those emitted by processing all the messages and those
emitted from the ante. Whereas Logs contains the events, with
additional metadata, emitted only by processing the messages.

##### Defined in

lumina\_node\_wasm.d.ts:2461

***

#### gas\_used

> **gas\_used**: `bigint`

Amount of gas consumed by transaction.

##### Defined in

lumina\_node\_wasm.d.ts:2444

***

#### gas\_wanted

> **gas\_wanted**: `bigint`

Amount of gas requested for transaction.

##### Defined in

lumina\_node\_wasm.d.ts:2440

***

#### height

> `readonly` **height**: `bigint`

The block height

##### Defined in

lumina\_node\_wasm.d.ts:2454

***

#### info

> **info**: `string`

Additional information. May be non-deterministic.

##### Defined in

lumina\_node\_wasm.d.ts:2436

***

#### logs

> **logs**: [`AbciMessageLog`](#classesabcimessagelogmd)[]

The output of the application's logger (typed). May be non-deterministic.

##### Defined in

lumina\_node\_wasm.d.ts:2432

***

#### raw\_log

> **raw\_log**: `string`

The output of the application's logger (raw string). May be
non-deterministic.

##### Defined in

lumina\_node\_wasm.d.ts:2428

***

#### timestamp

> **timestamp**: `string`

Time of the previous block. For heights > 1, it's the weighted median of
the timestamps of the valid votes in the block.LastCommit. For height == 1,
it's genesis time.

##### Defined in

lumina\_node\_wasm.d.ts:2450

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2411


<a name="classestxstatusresponsemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxStatusResponse

## Class: TxStatusResponse

Response to a tx status query

### Properties

#### error

> **error**: `string`

Error log, if transaction failed.

##### Defined in

lumina\_node\_wasm.d.ts:2482

***

#### execution\_code

> **execution\_code**: [`ErrorCode`](#enumerationserrorcodemd)

Execution_code is returned when the transaction has been committed
and returns whether it was successful or errored. A non zero
execution code indicates an error.

##### Defined in

lumina\_node\_wasm.d.ts:2478

***

#### height

> `readonly` **height**: `bigint`

Height of the block in which the transaction was committed.

##### Defined in

lumina\_node\_wasm.d.ts:2490

***

#### index

> **index**: `number`

Index of the transaction in block.

##### Defined in

lumina\_node\_wasm.d.ts:2472

***

#### status

> **status**: [`TxStatus`](#enumerationstxstatusmd)

Status of the transaction.

##### Defined in

lumina\_node\_wasm.d.ts:2486

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2468


<a name="classesvaladdressmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ValAddress

## Class: ValAddress

Address of a validator.

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2505

***

#### toJSON()

> **toJSON**(): `Object`

* Return copy of self without private attributes.

##### Returns

`Object`

##### Defined in

lumina\_node\_wasm.d.ts:2500

***

#### toString()

> **toString**(): `string`

Return stringified version of self.

##### Returns

`string`

##### Defined in

lumina\_node\_wasm.d.ts:2504


<a name="classesvalidatorsetmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ValidatorSet

## Class: ValidatorSet

Validator set contains a vector of validators

### Properties

#### total\_voting\_power

> **total\_voting\_power**: `bigint`

Total voting power

##### Defined in

lumina\_node\_wasm.d.ts:2528

***

#### validators

> **validators**: [`JsValidatorInfo`](#classesjsvalidatorinfomd)[]

Validators in the set

##### Defined in

lumina\_node\_wasm.d.ts:2516

### Accessors

#### proposer

##### Get Signature

> **get** **proposer**(): [`JsValidatorInfo`](#classesjsvalidatorinfomd)

Proposer

###### Returns

[`JsValidatorInfo`](#classesjsvalidatorinfomd)

##### Set Signature

> **set** **proposer**(`value`): `void`

Proposer

###### Parameters

####### value

[`JsValidatorInfo`](#classesjsvalidatorinfomd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2520

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2512


<a name="classesvotemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / Vote

## Class: Vote

Votes are signed messages from validators for a particular block which include
information about the validator signing it.

### Properties

#### extension

> **extension**: `Uint8Array`\<`ArrayBuffer`\>

Vote extension provided by the application. Only valid for precommit messages.

##### Defined in

lumina\_node\_wasm.d.ts:2584

***

#### height

> **height**: `bigint`

Block height

##### Defined in

lumina\_node\_wasm.d.ts:2544

***

#### round

> **round**: `number`

Round

##### Defined in

lumina\_node\_wasm.d.ts:2548

***

#### validator\_address

> **validator\_address**: `string`

Validator address

##### Defined in

lumina\_node\_wasm.d.ts:2568

***

#### validator\_index

> **validator\_index**: `number`

Validator index

##### Defined in

lumina\_node\_wasm.d.ts:2572

***

#### vote\_type

> **vote\_type**: [`VoteType`](#enumerationsvotetypemd)

Type of vote (prevote or precommit)

##### Defined in

lumina\_node\_wasm.d.ts:2540

### Accessors

#### block\_id

##### Get Signature

> **get** **block\_id**(): [`BlockId`](#classesblockidmd)

Block ID

###### Returns

[`BlockId`](#classesblockidmd)

##### Set Signature

> **set** **block\_id**(`value`): `void`

Block ID

###### Parameters

####### value

[`BlockId`](#classesblockidmd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2552

***

#### extension\_signature

##### Get Signature

> **get** **extension\_signature**(): [`Signature`](#classessignaturemd)

Vote extension signature by the validator Only valid for precommit messages.

###### Returns

[`Signature`](#classessignaturemd)

##### Set Signature

> **set** **extension\_signature**(`value`): `void`

Vote extension signature by the validator Only valid for precommit messages.

###### Parameters

####### value

[`Signature`](#classessignaturemd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2588

***

#### signature

##### Get Signature

> **get** **signature**(): [`Signature`](#classessignaturemd)

Signature

###### Returns

[`Signature`](#classessignaturemd)

##### Set Signature

> **set** **signature**(`value`): `void`

Signature

###### Parameters

####### value

[`Signature`](#classessignaturemd)

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2576

***

#### timestamp

##### Get Signature

> **get** **timestamp**(): `string`

Timestamp

###### Returns

`string`

##### Set Signature

> **set** **timestamp**(`value`): `void`

Timestamp

###### Parameters

####### value

`string`

###### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2560

### Methods

#### free()

> **free**(): `void`

##### Returns

`void`

##### Defined in

lumina\_node\_wasm.d.ts:2536

# Enumerations


<a name="enumerationsattacktypemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / AttackType

## Enumeration: AttackType

Attack type for the associated evidence

### Enumeration Members

#### DuplicateVote

> **DuplicateVote**: `0`

Duplicate vote

##### Defined in

lumina\_node\_wasm.d.ts:18

***

#### LightClient

> **LightClient**: `1`

LightClient attack

##### Defined in

lumina\_node\_wasm.d.ts:22


<a name="enumerationscommitvotetypemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / CommitVoteType

## Enumeration: CommitVoteType

### Enumeration Members

#### BlockIdFlagAbsent

> **BlockIdFlagAbsent**: `0`

no vote was received from a validator.

##### Defined in

lumina\_node\_wasm.d.ts:28

***

#### BlockIdFlagCommit

> **BlockIdFlagCommit**: `1`

voted for the Commit.BlockID.

##### Defined in

lumina\_node\_wasm.d.ts:32

***

#### BlockIdFlagNil

> **BlockIdFlagNil**: `2`

voted for nil

##### Defined in

lumina\_node\_wasm.d.ts:36


<a name="enumerationserrorcodemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / ErrorCode

## Enumeration: ErrorCode

Error codes associated with transaction responses.

### Enumeration Members

#### AppConfig

> **AppConfig**: `40`

Min-gas-prices field in BaseConfig is empty

##### Defined in

lumina\_node\_wasm.d.ts:201

***

#### BlobSizeMismatch

> **BlobSizeMismatch**: `11113`

actual blob size differs from that specified in the MsgPayForBlob

##### Defined in

lumina\_node\_wasm.d.ts:225

***

#### BlobsTooLarge

> **BlobsTooLarge**: `11139`

blob(s) too large

##### Defined in

lumina\_node\_wasm.d.ts:323

***

#### CalculateCommitment

> **CalculateCommitment**: `11115`

unexpected error calculating commitment for share

##### Defined in

lumina\_node\_wasm.d.ts:233

***

#### CommittedSquareSizeNotPowOf2

> **CommittedSquareSizeNotPowOf2**: `11114`

committed to invalid square size: must be power of two

##### Defined in

lumina\_node\_wasm.d.ts:229

***

#### Conflict

> **Conflict**: `36`

Conflict error, e.g. when two goroutines try to access the same resource and one of them fails

##### Defined in

lumina\_node\_wasm.d.ts:185

***

#### InsufficientFee

> **InsufficientFee**: `13`

Fee is insufficient

##### Defined in

lumina\_node\_wasm.d.ts:93

***

#### InsufficientFunds

> **InsufficientFunds**: `5`

Account cannot pay requested amount

##### Defined in

lumina\_node\_wasm.d.ts:61

***

#### InvalidAddress

> **InvalidAddress**: `7`

Address is invalid

##### Defined in

lumina\_node\_wasm.d.ts:69

***

#### InvalidBlobSigner

> **InvalidBlobSigner**: `11140`

invalid blob signer

##### Defined in

lumina\_node\_wasm.d.ts:327

***

#### InvalidChainID

> **InvalidChainID**: `28`

Chain-id is invalid

##### Defined in

lumina\_node\_wasm.d.ts:153

***

#### InvalidCoins

> **InvalidCoins**: `10`

Coin is invalid

##### Defined in

lumina\_node\_wasm.d.ts:81

***

#### InvalidDataSize

> **InvalidDataSize**: `11112`

data must be multiple of shareSize

##### Defined in

lumina\_node\_wasm.d.ts:221

***

#### InvalidGasAdjustment

> **InvalidGasAdjustment**: `25`

Invalid gas adjustment

##### Defined in

lumina\_node\_wasm.d.ts:141

***

#### InvalidGasLimit

> **InvalidGasLimit**: `41`

Invalid GasWanted value is supplied

##### Defined in

lumina\_node\_wasm.d.ts:205

***

#### InvalidHeight

> **InvalidHeight**: `26`

Invalid height

##### Defined in

lumina\_node\_wasm.d.ts:145

***

#### InvalidNamespace

> **InvalidNamespace**: `11136`

invalid namespace

##### Defined in

lumina\_node\_wasm.d.ts:309

***

#### InvalidNamespaceLen

> **InvalidNamespaceLen**: `11111`

invalid namespace length

##### Defined in

lumina\_node\_wasm.d.ts:217

***

#### InvalidNamespaceVersion

> **InvalidNamespaceVersion**: `11137`

invalid namespace version

##### Defined in

lumina\_node\_wasm.d.ts:313

***

#### InvalidPubKey

> **InvalidPubKey**: `8`

Pubkey is invalid

##### Defined in

lumina\_node\_wasm.d.ts:73

***

#### InvalidRequest

> **InvalidRequest**: `18`

Request contains invalid data

##### Defined in

lumina\_node\_wasm.d.ts:113

***

#### InvalidSequence

> **InvalidSequence**: `3`

Sequence number (nonce) is incorrect for the signature

##### Defined in

lumina\_node\_wasm.d.ts:53

***

#### InvalidShareCommitment

> **InvalidShareCommitment**: `11116`

invalid commitment for share

##### Defined in

lumina\_node\_wasm.d.ts:237

***

#### InvalidShareCommitments

> **InvalidShareCommitments**: `11122`

invalid share commitments: all relevant square sizes must be committed to

##### Defined in

lumina\_node\_wasm.d.ts:253

***

#### InvalidSigner

> **InvalidSigner**: `24`

Tx intended signer does not match the given signer

##### Defined in

lumina\_node\_wasm.d.ts:137

***

#### InvalidType

> **InvalidType**: `29`

Invalid type

##### Defined in

lumina\_node\_wasm.d.ts:157

***

#### InvalidVersion

> **InvalidVersion**: `27`

Invalid version

##### Defined in

lumina\_node\_wasm.d.ts:149

***

#### IO

> **IO**: `39`

Internal errors caused by external operation

##### Defined in

lumina\_node\_wasm.d.ts:197

***

#### JSONMarshal

> **JSONMarshal**: `16`

Error converting to json

##### Defined in

lumina\_node\_wasm.d.ts:105

***

#### JSONUnmarshal

> **JSONUnmarshal**: `17`

Error converting from json

##### Defined in

lumina\_node\_wasm.d.ts:109

***

#### KeyNotFound

> **KeyNotFound**: `22`

Key doesn't exist

##### Defined in

lumina\_node\_wasm.d.ts:129

***

#### Logic

> **Logic**: `35`

Internal logic error, e.g. an invariant or assertion that is violated

##### Defined in

lumina\_node\_wasm.d.ts:181

***

#### MemoTooLarge

> **MemoTooLarge**: `12`

Memo too large

##### Defined in

lumina\_node\_wasm.d.ts:89

***

#### MempoolIsFull

> **MempoolIsFull**: `20`

Mempool is full

##### Defined in

lumina\_node\_wasm.d.ts:121

***

#### MismatchedNumberOfPFBComponent

> **MismatchedNumberOfPFBComponent**: `11130`

number of each component in a MsgPayForBlobs must be identical

##### Defined in

lumina\_node\_wasm.d.ts:285

***

#### MismatchedNumberOfPFBorBlob

> **MismatchedNumberOfPFBorBlob**: `11125`

mismatched number of blobs per MsgPayForBlob

##### Defined in

lumina\_node\_wasm.d.ts:265

***

#### MultipleMsgsInBlobTx

> **MultipleMsgsInBlobTx**: `11129`

not yet supported: multiple sdk.Msgs found in BlobTx

##### Defined in

lumina\_node\_wasm.d.ts:281

***

#### NamespaceMismatch

> **NamespaceMismatch**: `11127`

namespace of blob and its respective MsgPayForBlobs differ

##### Defined in

lumina\_node\_wasm.d.ts:273

***

#### NoBlobs

> **NoBlobs**: `11131`

no blobs provided

##### Defined in

lumina\_node\_wasm.d.ts:289

***

#### NoBlobSizes

> **NoBlobSizes**: `11134`

no blob sizes provided

##### Defined in

lumina\_node\_wasm.d.ts:301

***

#### NoNamespaces

> **NoNamespaces**: `11132`

no namespaces provided

##### Defined in

lumina\_node\_wasm.d.ts:293

***

#### NoPFB

> **NoPFB**: `11126`

no MsgPayForBlobs found in blob transaction

##### Defined in

lumina\_node\_wasm.d.ts:269

***

#### NoShareCommitments

> **NoShareCommitments**: `11135`

no share commitments provided

##### Defined in

lumina\_node\_wasm.d.ts:305

***

#### NoShareVersions

> **NoShareVersions**: `11133`

no share versions provided

##### Defined in

lumina\_node\_wasm.d.ts:297

***

#### NoSignatures

> **NoSignatures**: `15`

No signatures in transaction

##### Defined in

lumina\_node\_wasm.d.ts:101

***

#### NotFound

> **NotFound**: `38`

Requested entity doesn't exist in the state

##### Defined in

lumina\_node\_wasm.d.ts:193

***

#### NotSupported

> **NotSupported**: `37`

Called a branch of a code which is currently not supported

##### Defined in

lumina\_node\_wasm.d.ts:189

***

#### OutOfGas

> **OutOfGas**: `11`

Gas exceeded

##### Defined in

lumina\_node\_wasm.d.ts:85

***

#### PackAny

> **PackAny**: `33`

Packing a protobuf message to Any failed

##### Defined in

lumina\_node\_wasm.d.ts:173

***

#### Panic

> **Panic**: `111222`

Node recovered from panic

##### Defined in

lumina\_node\_wasm.d.ts:209

***

#### ParitySharesNamespace

> **ParitySharesNamespace**: `11117`

cannot use parity shares namespace ID

##### Defined in

lumina\_node\_wasm.d.ts:241

***

#### ProtoParsing

> **ProtoParsing**: `11128`

failure to parse a transaction from its protobuf representation

##### Defined in

lumina\_node\_wasm.d.ts:277

***

#### ReservedNamespace

> **ReservedNamespace**: `11110`

cannot use reserved namespace IDs

##### Defined in

lumina\_node\_wasm.d.ts:213

***

#### Success

> **Success**: `0`

No error

##### Defined in

lumina\_node\_wasm.d.ts:45

***

#### TailPaddingNamespace

> **TailPaddingNamespace**: `11118`

cannot use tail padding namespace ID

##### Defined in

lumina\_node\_wasm.d.ts:245

***

#### TooManySignatures

> **TooManySignatures**: `14`

Too many signatures

##### Defined in

lumina\_node\_wasm.d.ts:97

***

#### TotalBlobSizeTooLarge

> **TotalBlobSizeTooLarge**: `11138`

total blob size too large

TotalBlobSize is deprecated, use BlobsTooLarge instead.

##### Defined in

lumina\_node\_wasm.d.ts:319

***

#### TxDecode

> **TxDecode**: `2`

Cannot parse a transaction

##### Defined in

lumina\_node\_wasm.d.ts:49

***

#### TxInMempoolCache

> **TxInMempoolCache**: `19`

Tx already exists in the mempool

##### Defined in

lumina\_node\_wasm.d.ts:117

***

#### TxNamespace

> **TxNamespace**: `11119`

cannot use transaction namespace ID

##### Defined in

lumina\_node\_wasm.d.ts:249

***

#### TxTimeoutHeight

> **TxTimeoutHeight**: `30`

Tx rejected due to an explicitly set timeout height

##### Defined in

lumina\_node\_wasm.d.ts:161

***

#### TxTooLarge

> **TxTooLarge**: `21`

Tx is too large

##### Defined in

lumina\_node\_wasm.d.ts:125

***

#### Unauthorized

> **Unauthorized**: `4`

Request without sufficient authorization is handled

##### Defined in

lumina\_node\_wasm.d.ts:57

***

#### UnknownAddress

> **UnknownAddress**: `9`

Address is unknown

##### Defined in

lumina\_node\_wasm.d.ts:77

***

#### UnknownExtensionOptions

> **UnknownExtensionOptions**: `31`

Unknown extension options.

##### Defined in

lumina\_node\_wasm.d.ts:165

***

#### UnknownRequest

> **UnknownRequest**: `6`

Request is unknown

##### Defined in

lumina\_node\_wasm.d.ts:65

***

#### UnpackAny

> **UnpackAny**: `34`

Unpacking a protobuf message from Any failed

##### Defined in

lumina\_node\_wasm.d.ts:177

***

#### UnsupportedShareVersion

> **UnsupportedShareVersion**: `11123`

unsupported share version

##### Defined in

lumina\_node\_wasm.d.ts:257

***

#### WrongPassword

> **WrongPassword**: `23`

Key password is invalid

##### Defined in

lumina\_node\_wasm.d.ts:133

***

#### WrongSequence

> **WrongSequence**: `32`

Account sequence defined in the signer info doesn't match the account's actual sequence

##### Defined in

lumina\_node\_wasm.d.ts:169

***

#### ZeroBlobSize

> **ZeroBlobSize**: `11124`

cannot use zero blob size

##### Defined in

lumina\_node\_wasm.d.ts:261


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

lumina\_node\_wasm.d.ts:340

***

#### Mainnet

> **Mainnet**: `0`

Celestia mainnet.

##### Defined in

lumina\_node\_wasm.d.ts:336

***

#### Mocha

> **Mocha**: `2`

Mocha testnet.

##### Defined in

lumina\_node\_wasm.d.ts:344

***

#### Private

> **Private**: `3`

Private local network.

##### Defined in

lumina\_node\_wasm.d.ts:348


<a name="enumerationssignaturemodemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SignatureMode

## Enumeration: SignatureMode

### Enumeration Members

#### Multi

> **Multi**: `1`

##### Defined in

lumina\_node\_wasm.d.ts:352

***

#### Single

> **Single**: `0`

##### Defined in

lumina\_node\_wasm.d.ts:351


<a name="enumerationstxprioritymd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxPriority

## Enumeration: TxPriority

TxPriority is the priority level of the requested gas price.

### Enumeration Members

#### High

> **High**: `3`

Estimated gas price is the price at the start of the top 10% of transactions’ gas prices from the last 5 blocks.

##### Defined in

lumina\_node\_wasm.d.ts:369

***

#### Low

> **Low**: `1`

Estimated gas price is the value at the end of the lowest 10% of gas prices from the last 5 blocks.

##### Defined in

lumina\_node\_wasm.d.ts:361

***

#### Medium

> **Medium**: `2`

Estimated gas price is the mean of all gas prices from the last 5 blocks.

##### Defined in

lumina\_node\_wasm.d.ts:365


<a name="enumerationstxstatusmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxStatus

## Enumeration: TxStatus

Represents state of the transaction in the mempool

### Enumeration Members

#### Committed

> **Committed**: `3`

The transaction was committed into the block.

##### Defined in

lumina\_node\_wasm.d.ts:390

***

#### Evicted

> **Evicted**: `2`

The transaction was evicted from the mempool.

##### Defined in

lumina\_node\_wasm.d.ts:386

***

#### Pending

> **Pending**: `1`

The transaction is still pending.

##### Defined in

lumina\_node\_wasm.d.ts:382

***

#### Unknown

> **Unknown**: `0`

The transaction is not known to the node, it could be never sent.

##### Defined in

lumina\_node\_wasm.d.ts:378


<a name="enumerationsvotetypemd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / VoteType

## Enumeration: VoteType

Types of votes

### Enumeration Members

#### Precommit

> **Precommit**: `1`

Precommit

##### Defined in

lumina\_node\_wasm.d.ts:403

***

#### Prevote

> **Prevote**: `0`

Prevote

##### Defined in

lumina\_node\_wasm.d.ts:399

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

- [AttackType](#enumerationsattacktypemd)
- [CommitVoteType](#enumerationscommitvotetypemd)
- [ErrorCode](#enumerationserrorcodemd)
- [Network](#enumerationsnetworkmd)
- [SignatureMode](#enumerationssignaturemodemd)
- [TxPriority](#enumerationstxprioritymd)
- [TxStatus](#enumerationstxstatusmd)
- [VoteType](#enumerationsvotetypemd)

## Classes

- [AbciMessageLog](#classesabcimessagelogmd)
- [AbciQueryResponse](#classesabciqueryresponsemd)
- [AccAddress](#classesaccaddressmd)
- [AppVersion](#classesappversionmd)
- [Attribute](#classesattributemd)
- [AuthInfo](#classesauthinfomd)
- [Blob](#classesblobmd)
- [BlobParams](#classesblobparamsmd)
- [Block](#classesblockmd)
- [BlockId](#classesblockidmd)
- [BlockRange](#classesblockrangemd)
- [BroadcastMode](#classesbroadcastmodemd)
- [Coin](#classescoinmd)
- [Commit](#classescommitmd)
- [Commitment](#classescommitmentmd)
- [CommitSig](#classescommitsigmd)
- [CommitVote](#classescommitvotemd)
- [ConflictingBlock](#classesconflictingblockmd)
- [ConnectionCountersSnapshot](#classesconnectioncounterssnapshotmd)
- [ConsAddress](#classesconsaddressmd)
- [Data](#classesdatamd)
- [DataAvailabilityHeader](#classesdataavailabilityheadermd)
- [DuplicateVoteEvidence](#classesduplicatevoteevidencemd)
- [Evidence](#classesevidencemd)
- [ExtendedHeader](#classesextendedheadermd)
- [Fee](#classesfeemd)
- [GasEstimate](#classesgasestimatemd)
- [GasInfo](#classesgasinfomd)
- [GetTxResponse](#classesgettxresponsemd)
- [GrpcClient](#classesgrpcclientmd)
- [Header](#classesheadermd)
- [IntoUnderlyingByteSource](#classesintounderlyingbytesourcemd)
- [IntoUnderlyingSink](#classesintounderlyingsinkmd)
- [IntoUnderlyingSource](#classesintounderlyingsourcemd)
- [JsBitVector](#classesjsbitvectormd)
- [JsEvent](#classesjseventmd)
- [JsEventAttribute](#classesjseventattributemd)
- [JsValidatorInfo](#classesjsvalidatorinfomd)
- [LightClientAttackEvidence](#classeslightclientattackevidencemd)
- [ModeInfo](#classesmodeinfomd)
- [Namespace](#classesnamespacemd)
- [NetworkInfoSnapshot](#classesnetworkinfosnapshotmd)
- [NodeClient](#classesnodeclientmd)
- [NodeConfig](#classesnodeconfigmd)
- [NodeWorker](#classesnodeworkermd)
- [PartsHeader](#classespartsheadermd)
- [PeerTrackerInfoSnapshot](#classespeertrackerinfosnapshotmd)
- [ProofOp](#classesproofopmd)
- [ProofOps](#classesproofopsmd)
- [ProtocolVersion](#classesprotocolversionmd)
- [SamplingMetadata](#classessamplingmetadatamd)
- [Signature](#classessignaturemd)
- [SignedHeader](#classessignedheadermd)
- [SignerInfo](#classessignerinfomd)
- [StringEvent](#classesstringeventmd)
- [SyncingInfoSnapshot](#classessyncinginfosnapshotmd)
- [Tx](#classestxmd)
- [TxBody](#classestxbodymd)
- [TxClient](#classestxclientmd)
- [TxResponse](#classestxresponsemd)
- [TxStatusResponse](#classestxstatusresponsemd)
- [ValAddress](#classesvaladdressmd)
- [ValidatorSet](#classesvalidatorsetmd)
- [Vote](#classesvotemd)

## Interfaces

- [AuthParams](#interfacesauthparamsmd)
- [BaseAccount](#interfacesbaseaccountmd)
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

lumina\_node\_wasm.d.ts:454

***

#### sigVerifyCostEd25519

> **sigVerifyCostEd25519**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:457

***

#### sigVerifyCostSecp256k1

> **sigVerifyCostSecp256k1**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:458

***

#### txSigLimit

> **txSigLimit**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:455

***

#### txSizeCostPerByte

> **txSizeCostPerByte**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:456


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

lumina\_node\_wasm.d.ts:446

***

#### address

> **address**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:444

***

#### pubkey?

> `optional` **pubkey**: [`PublicKey`](#interfacespublickeymd)

##### Defined in

lumina\_node\_wasm.d.ts:445

***

#### sequence

> **sequence**: `bigint`

##### Defined in

lumina\_node\_wasm.d.ts:447


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

lumina\_node\_wasm.d.ts:426

***

#### value

> **value**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:427


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

lumina\_node\_wasm.d.ts:436

***

#### value

> **value**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:437


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

lumina\_node\_wasm.d.ts:470

***

#### authInfoBytes

> **authInfoBytes**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:468

***

#### bodyBytes

> **bodyBytes**: `Uint8Array`\<`ArrayBuffer`\>

##### Defined in

lumina\_node\_wasm.d.ts:467

***

#### chainId

> **chainId**: `string`

##### Defined in

lumina\_node\_wasm.d.ts:469


<a name="interfacestxconfigmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxConfig

## Interface: TxConfig

Transaction config.

### Properties

#### gasLimit?

> `optional` **gasLimit**: `bigint`

Custom gas limit for the transaction (in `utia`). By default, client will
query gas estimation service to get estimate gas limit.

##### Defined in

lumina\_node\_wasm.d.ts:502

***

#### gasPrice?

> `optional` **gasPrice**: `number`

Custom gas price for fee calculation. By default, client will query gas
estimation service to get gas price estimate.

##### Defined in

lumina\_node\_wasm.d.ts:507

***

#### memo?

> `optional` **memo**: `string`

Memo for the transaction

##### Defined in

lumina\_node\_wasm.d.ts:511

***

#### priority?

> `optional` **priority**: [`TxPriority`](#enumerationstxprioritymd)

Priority of the transaction, used with gas estimation service

##### Defined in

lumina\_node\_wasm.d.ts:515


<a name="interfacestxinfomd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / TxInfo

## Interface: TxInfo

Transaction info

### Properties

#### hash

> **hash**: `string`

Hash of the transaction.

##### Defined in

lumina\_node\_wasm.d.ts:487

***

#### height

> **height**: `bigint`

Height at which transaction was submitted.

##### Defined in

lumina\_node\_wasm.d.ts:491

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

lumina\_node\_wasm.d.ts:410


<a name="type-aliasessignerfnmd"></a>

[**lumina-node-wasm**](#readmemd)

***

[lumina-node-wasm](#globalsmd) / SignerFn

## Type Alias: SignerFn

> **SignerFn**: (`arg`) => `Uint8Array` \| (`arg`) => `Promise`\<`Uint8Array`\>

A function that produces a signature of a payload

### Defined in

lumina\_node\_wasm.d.ts:476
