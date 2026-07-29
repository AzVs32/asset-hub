# syntax=docker/dockerfile:1.7

FROM rust:1.97.1-trixie AS api-builder
WORKDIR /src
RUN apt-get update \
    && apt-get install -y --no-install-recommends clang cmake pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN rustup target add wasm32-unknown-unknown \
    && ./scripts/verify-plugin-artifacts.sh
RUN cargo build --locked --release -p asset-http -p asset-cli --bins

FROM node:22-trixie-slim AS web-builder
WORKDIR /src/asset-web
COPY asset-web/package.json asset-web/package-lock.json ./
RUN npm ci
COPY asset-web/ ./
RUN npm run build

FROM node:22-trixie-slim AS markdown-web-builder
WORKDIR /src/plugins/azvs-markdown/web
COPY plugins/azvs-markdown/web/package.json plugins/azvs-markdown/web/package-lock.json ./
RUN npm ci
COPY plugins/azvs-markdown/web/ ./
COPY plugins/azvs-markdown/manifest.json plugins/azvs-markdown/plugin.wasm /src/plugins/azvs-markdown/
RUN npm run build
RUN mkdir -p /out/azvs.markdown \
    && cp /src/plugins/azvs-markdown/manifest.json /src/plugins/azvs-markdown/plugin.wasm \
      /out/azvs.markdown/ \
    && cp -R /src/plugins/azvs-markdown/dist/. /out/azvs.markdown/

FROM node:22-trixie-slim AS epub-web-builder
WORKDIR /src/plugins/azvs-epub/web
COPY plugins/azvs-epub/web/package.json plugins/azvs-epub/web/package-lock.json ./
RUN npm ci
COPY plugins/azvs-epub/web/ ./
COPY plugins/azvs-epub/manifest.json plugins/azvs-epub/plugin.wasm /src/plugins/azvs-epub/
RUN npm run build
RUN mkdir -p /out/azvs.epub \
    && cp /src/plugins/azvs-epub/manifest.json /src/plugins/azvs-epub/plugin.wasm \
      /out/azvs.epub/ \
    && cp -R /src/plugins/azvs-epub/dist/. /out/azvs.epub/

FROM debian:trixie-slim AS api
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app asset-hub \
    && mkdir -p /app /conf /data/.asset-hub/plugins \
    && chown -R asset-hub:asset-hub /app /conf /data
WORKDIR /app
COPY --from=api-builder /src/target/release/asset-http /usr/local/bin/asset-http
COPY --from=markdown-web-builder --chown=asset-hub:asset-hub /out/azvs.markdown /data/.asset-hub/plugins/azvs.markdown
COPY --from=epub-web-builder --chown=asset-hub:asset-hub /out/azvs.epub /data/.asset-hub/plugins/azvs.epub
COPY --chown=asset-hub:asset-hub docker/config.toml /conf/config.toml
USER asset-hub
ENV ASSET_HUB_CONFIG=/conf/config.toml \
    ASSET_HTTP_ADDR=0.0.0.0:8080 \
    ASSET_HTTP_ENABLE_SWAGGER=false \
    ASSET_HTTP_ENABLE_PURGE=false \
    ASSET_HTTP_REQUEST_TIMEOUT_SECS=30 \
    RUST_LOG=asset_http=info,tower_http=info
EXPOSE 8080
VOLUME ["/conf", "/data"]
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/health
ENTRYPOINT ["asset-http"]

FROM nginx:1.27-alpine AS web
COPY --from=web-builder /src/asset-web/dist /usr/share/nginx/html
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=5 \
    CMD wget -q --spider http://127.0.0.1/ || exit 1
