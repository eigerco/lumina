/**
* Spawn a worker running lumina node and get the `NodeClient` connected to it.
*/
export function spawnNode(): Promise<NodeClient>;
export * from "lumina-node-wasm";
export default function init(): Promise<void>;
