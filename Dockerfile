FROM rust:1.90-slim AS build-base
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

FROM build-base AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
COPY src/ ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM build-base AS builder
ARG BUILD_TARGET=""
RUN cargo install cargo-chef --locked
WORKDIR /app
COPY --from=chef /app/recipe.json /app/recipe.json
RUN if [ -n "$BUILD_TARGET" ]; then rustup target add "$BUILD_TARGET"; fi
# Build dependencies - this is the caching Docker layer
RUN TARGET_ARG="${BUILD_TARGET:+--target $BUILD_TARGET}" && \
    cargo chef cook --release --recipe-path /app/recipe.json $TARGET_ARG
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
COPY src/ ./src
# Build application
RUN TARGET_ARG="${BUILD_TARGET:+--target $BUILD_TARGET}" && \
    cargo build --release --locked $TARGET_ARG
RUN TARGET_SUBDIR="${BUILD_TARGET:+$BUILD_TARGET/}" && \
    mkdir -p /app/bin && \
    cp "target/${TARGET_SUBDIR}release/neon-beat-back" /app/bin/neon-beat-back

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/bin/neon-beat-back /usr/local/bin/neon-beat-back
ENV RUST_LOG=info
EXPOSE 8080
CMD ["neon-beat-back"]
