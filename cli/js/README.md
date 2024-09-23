```bash
wasm-pack build --release --target web node-wasm
wasm-pack pack

cd node-wasm/js
npm pack

cd cli/js
npm i
npm run build

cargo run --features browser-node -- browser
```
