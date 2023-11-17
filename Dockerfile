FROM rust:1.74 AS builder

WORKDIR /build

RUN apt-get update && \
  apt-get install -y protobuf-compiler && \
  apt-get clean

RUN cargo install wasm-pack@0.12.1 --locked

COPY . .

RUN wasm-pack build --release --target web node-wasm

RUN cargo build --release -p celestia


FROM debian:bookworm-slim

COPY --from=builder /build/target/release/celestia /usr/local/bin/celestia

ENTRYPOINT ["celestia"]
CMD ["node"]
