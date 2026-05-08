# Choose "release" or "debug" at build time:
#   docker build -f nifty-dashboard/Dockerfile --build-arg CARGO_PROFILE=debug   -t nifty-dashboard:debug   .
#   docker build -f nifty-dashboard/Dockerfile --build-arg CARGO_PROFILE=release -t nifty-dashboard:release .
ARG CARGO_PROFILE=release

# App naming (crate dir / package name / binary name)
ARG APP_CRATE=nifty-dashboard
ARG APP_BIN=nifty-dashboard

# ---- Frontend builder (SvelteKit) ---------------------------------
FROM node:22-slim AS frontend-builder

WORKDIR /frontend
RUN corepack enable

COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY frontend ./
RUN pnpm build

RUN echo "=== DEBUG: contents of /frontend after pnpm build ===" && \
    ls -R .

# ---- Rust builder (workspace) ------------------------------------
FROM rust:1.91-slim-trixie AS builder

ARG APP_CRATE
ARG APP_BIN
ARG CARGO_PROFILE

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates git pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

RUN cargo install --locked sqlx-cli --no-default-features --features sqlite

# ---------- Pre-fetch Rust dependencies for workspace -------------
COPY Cargo.toml Cargo.lock ./

# Copy workspace member manifests
COPY ${APP_CRATE}/Cargo.toml ${APP_CRATE}/Cargo.toml
COPY api-doc-macros/Cargo.toml api-doc-macros/Cargo.toml
COPY app-macros/Cargo.toml app-macros/Cargo.toml

# Use dummy sources so Cargo can load all workspace members
RUN mkdir -p ${APP_CRATE}/src api-doc-macros/src app-macros/src && \
    echo "fn main() {}" > ${APP_CRATE}/src/main.rs && \
    echo "pub fn dummy() {}" > api-doc-macros/src/lib.rs && \
    echo "pub fn dummy() {}" > app-macros/src/lib.rs && \
    cargo fetch && \
    rm -rf ${APP_CRATE}/src api-doc-macros/src app-macros/src

# ---------- Copy full Rust workspace + frontend build -------------
COPY . .

# Copy built frontend into the location Rust expects at compile time.
# For #[folder = "../frontend/build"], from /app/${APP_CRATE} this resolves to /app/frontend/build.
COPY --from=frontend-builder /frontend/build ./frontend/build

RUN echo "=== DEBUG: contents of /app after copying frontend build ===" && \
    ls -R /app && \
    echo "=== DEBUG: contents of /app/frontend/build ===" && \
    if [ -d /app/frontend/build ]; then \
        ls -R /app/frontend/build; \
    else \
        echo "ERROR: /app/frontend/build directory does not exist"; \
        exit 1; \
    fi && \
    if [ ! -f /app/frontend/build/index.html ]; then \
        echo "ERROR: /app/frontend/build/index.html is missing"; \
        exit 1; \
    fi

WORKDIR /app/${APP_CRATE}
ENV DATABASE_URL=sqlite:test.db

RUN sqlx database create && sqlx migrate run && \
    if [ "$CARGO_PROFILE" = "release" ]; then \
        echo "Building release profile"; \
        cargo build --release --locked -p "${APP_CRATE}"; \
    elif [ "$CARGO_PROFILE" = "debug" ]; then \
        echo "Building debug profile"; \
        cargo build --locked -p "${APP_CRATE}"; \
    else \
        echo "Unknown CARGO_PROFILE='$CARGO_PROFILE' (expected 'release' or 'debug')" >&2; \
        exit 1; \
    fi

# ---- Runtime -------------------------------------------------
FROM debian:trixie-slim AS runtime

ARG APP_BIN
ARG CARGO_PROFILE
ENV APP_BIN=${APP_BIN}

LABEL org.opencontainers.image.source="https://github.com/enigmacurry/${APP_BIN}"

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates coreutils sqlite3 && \
    rm -rf /var/lib/apt/lists/*

ARG USER=appuser UID=1001 GID=1001
RUN groupadd --gid "$GID" "$USER" \
 && useradd  --uid "$UID" \
             --gid "$GID" \
             --create-home \
             --shell /usr/sbin/nologin \
             "$USER"

COPY --from=builder /app/target/${CARGO_PROFILE}/${APP_BIN} /usr/local/bin/${APP_BIN}

RUN mkdir /data && chown ${USER} /data
VOLUME /data

ENV ROOT_DIR=/data NET_LISTEN_IP=0.0.0.0 NET_LISTEN_PORT=3000

USER $USER

# ENTRYPOINT JSON form doesn't expand vars, so use sh -c to exec the chosen binary.
ENTRYPOINT ["/bin/sh", "-c", "exec /usr/local/bin/$APP_BIN"]
