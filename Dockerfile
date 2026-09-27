# syntax=docker/dockerfile:1.7
#
# Kanban board as one small image: the Svelte frontend is built first, then
# embedded into the Rust binary, which is the only thing in the final image.
# The SQLite database lives in /data, which is a volume.
#
#   docker compose up -d --build        (see compose.yaml)
#   docker build -t kanban-board .      (image only)

# ---------------------------------------------------------------- frontend
FROM node:24-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY web/ ./
RUN npm run build

# ---------------------------------------------------------------- server
FROM rust:1-bookworm AS server
WORKDIR /src/server
COPY server/ ./
# rust-embed reads ../web/dist relative to server/Cargo.toml.
COPY --from=web /src/web/dist /src/web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/server/target \
    cargo build --release --locked \
 && cp target/release/kanban-server /usr/local/bin/kanban-server

# ---------------------------------------------------------------- runtime
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home-dir /data kanban \
 && mkdir -p /data && chown kanban:kanban /data
COPY --from=server /usr/local/bin/kanban-server /usr/local/bin/kanban-server
USER kanban

# Inside the container the server must listen on all interfaces so Docker can
# forward to it. compose.yaml publishes it on the host's 127.0.0.1 only,
# because the board has no authentication.
ENV KANBAN_DB=/data/kanban.db \
    KANBAN_BIND=0.0.0.0 \
    KANBAN_PORT=8610
VOLUME /data
EXPOSE 8610
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 CMD ["kanban-server", "healthcheck"]
ENTRYPOINT ["kanban-server"]
