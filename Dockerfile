# syntax=docker/dockerfile:1
# Aegis headless stack: aegis_api + aegis_harness (+ optional static console).
# Native ./scripts/launch-desktop.sh remains the primary local workflow.
# Tauri desktop is intentionally not containerized.

ARG BUILD_CONSOLE=1

FROM rust:1-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY tools ./tools
# Release binaries for API + harness only.
RUN cargo build --release -p aegis_api -p aegis_harness \
    && strip target/release/aegis_api target/release/aegis_harness

FROM node:22-bookworm AS console-builder
ARG BUILD_CONSOLE=1
WORKDIR /console
COPY apps/console/package.json apps/console/package-lock.json ./
RUN if [ "$BUILD_CONSOLE" = "1" ]; then npm ci; else mkdir -p /console/dist; fi
COPY apps/console/ ./
RUN if [ "$BUILD_CONSOLE" = "1" ]; then \
      npm run build; \
    else \
      mkdir -p dist && printf '%s\n' \
        '<!doctype html><meta charset="utf-8"><title>Aegis</title>' \
        '<p>Console not baked into this image. Rebuild with <code>BUILD_CONSOLE=1</code>.</p>' \
        > dist/index.html; \
    fi

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --home-dir /home/aegis --shell /usr/sbin/nologin aegis

COPY --from=rust-builder /src/target/release/aegis_api /src/target/release/aegis_harness /usr/local/bin/
COPY docker/entrypoint-api.sh /usr/local/bin/entrypoint-api.sh
RUN chmod 755 /usr/local/bin/entrypoint-api.sh /usr/local/bin/aegis_api /usr/local/bin/aegis_harness

WORKDIR /app
COPY scenarios /app/scenarios
COPY --from=console-builder /console/dist /app/console

USER aegis
ENV PORT=8080
EXPOSE 8080
WORKDIR /app
ENTRYPOINT ["/usr/local/bin/entrypoint-api.sh"]
