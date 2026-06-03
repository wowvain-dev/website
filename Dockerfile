FROM rust:1-bookworm AS builder

ARG TRUNK_VERSION=0.21.14
ENV CARGO_BUILD_JOBS=2
WORKDIR /app

RUN rustup target add wasm32-unknown-unknown \
    && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
        | tar -xz -C /usr/local/bin trunk

COPY . .

RUN cd frontend && trunk build --release
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/wowvain_portfolio /usr/local/bin/wowvain_portfolio
COPY --from=builder /app/frontend/dist /app/frontend/dist

ENV BIND_ADDR=0.0.0.0:3001
EXPOSE 3001
CMD ["wowvain_portfolio"]
