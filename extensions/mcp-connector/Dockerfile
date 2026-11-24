FROM rust:1.98-bookworm AS build
WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release
FROM node:24-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates python3 python3-pip python3-venv && rm -rf /var/lib/apt/lists/* && mkdir -p /data /data/workspace && chown -R 10001:10001 /data
COPY --from=build /src/target/release/dominion-mcp-connector /usr/local/bin/dominion-mcp-connector
COPY --from=ghcr.io/astral-sh/uv:0.13.9 /uv /uvx /usr/local/bin/
ENV HOME=/data NPM_CONFIG_CACHE=/data/.npm UV_CACHE_DIR=/data/.cache/uv
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/dominion-mcp-connector"]
