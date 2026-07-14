# syntax=docker/dockerfile:1.7

FROM rust:1.94-bookworm AS api-builder
WORKDIR /src
RUN apt-get update \
    && apt-get install -y --no-install-recommends clang cmake pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN rustup target add wasm32-unknown-unknown \
    && ./scripts/verify-plugin-artifacts.sh
RUN cargo build --locked --release -p asset-apps --bins

FROM node:22-bookworm-slim AS web-builder
WORKDIR /src/asset-web-admin
COPY asset-web-admin/package.json asset-web-admin/package-lock.json ./
RUN npm ci
COPY asset-web-admin/ ./
RUN npm run build

FROM node:22-bookworm-slim AS markdown-web-builder
WORKDIR /src/plugins/azvs-markdown/web
COPY plugins/azvs-markdown/web/package.json plugins/azvs-markdown/web/package-lock.json ./
RUN npm ci
COPY plugins/azvs-markdown/web/ ./
COPY plugins/azvs-markdown/azvs-markdown.json /src/plugins/azvs-markdown/azvs-markdown.json
COPY --from=api-builder /src/target/release/asset-plugin /usr/local/bin/asset-plugin
RUN npm run build
WORKDIR /src
RUN asset-plugin verify-web plugins/azvs-markdown/azvs-markdown.json

FROM debian:bookworm-slim AS api
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app asset-hub \
    && mkdir -p /app/plugins/azvs-markdown/web /data/blob \
    && chown -R asset-hub:asset-hub /app /data
WORKDIR /app
COPY --from=api-builder /src/target/release/asset-http /usr/local/bin/asset-http
COPY --from=api-builder /src/plugins /app/plugins
COPY --from=markdown-web-builder /src/plugins/azvs-markdown/web/dist /app/plugins/azvs-markdown/web/dist
COPY docker/config.toml /app/config.toml
USER asset-hub
ENV ASSET_HUB_CONFIG=/app/config.toml \
    ASSET_HTTP_ADDR=0.0.0.0:8080 \
    ASSET_HTTP_ENABLE_SWAGGER=false \
    ASSET_HTTP_ENABLE_PURGE=false \
    ASSET_HTTP_REQUEST_TIMEOUT_SECS=30 \
    RUST_LOG=asset_http=info,tower_http=info
EXPOSE 8080
VOLUME ["/data"]
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/health
ENTRYPOINT ["asset-http"]

FROM nginx:1.27-alpine AS web
COPY --from=web-builder /src/asset-web-admin/dist /usr/share/nginx/html
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=5 \
    CMD wget -q --spider http://127.0.0.1/ || exit 1
