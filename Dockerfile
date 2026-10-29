# The official Rust image tracks the latest stable compiler on Debian trixie.
FROM rust:trixie AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install topcoat-cli --version 0.11.0 --locked
WORKDIR /build
COPY . .
RUN cargo build --release --locked && topcoat asset bundle --release

FROM debian:trixie-slim AS runtime

# FFmpeg includes ffprobe; the agent also uses a shell and standard tools.
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash ca-certificates ffmpeg fontconfig fonts-dejavu-core imagemagick jq python3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 flicksy \
    && useradd --uid 10001 --gid flicksy --home-dir /var/lib/flicksy --create-home --shell /bin/bash flicksy \
    && mkdir -p /var/lib/flicksy/projects /etc/flicksy \
    && chown -R flicksy:flicksy /var/lib/flicksy
WORKDIR /app
COPY --from=build /build/target/release/flicksy /app/flicksy
COPY --from=build /build/target/release/assets /app/assets
COPY docker/config.toml /etc/flicksy/config.toml
USER flicksy
EXPOSE 3000
ENTRYPOINT ["/app/flicksy"]
CMD ["--config", "/etc/flicksy/config.toml"]
