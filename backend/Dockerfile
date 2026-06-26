# NOTE: must use trixie (glibc 2.40), not bookworm (glibc 2.36). The `ort`
# crate ships a prebuilt ONNX Runtime that needs glibc >= 2.38 at link time
# (__isoc23_strtol / __isoc23_strtoll). The runtime image must match.
FROM rust:1.88-trixie AS builder

WORKDIR /build
COPY . .

RUN cargo build --release --locked --bin epicode-cloud

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*

RUN groupadd -r epicode && useradd -r -g epicode -d /app -s /sbin/nologin epicode

WORKDIR /app
COPY --from=builder /build/target/release/epicode-cloud /app/epicode-cloud
COPY --from=builder /build/models/model.onnx /app/models/model.onnx
COPY --from=builder /build/models/tokenizer.json /app/models/tokenizer.json

RUN chmod 500 /app/epicode-cloud && chown -R epicode:epicode /app
USER epicode
RUN mkdir -p /app/data

ENV RUST_LOG=info
ENV EPICODE_LISTEN_ADDR=0.0.0.0:9111
EXPOSE 9111
VOLUME ["/app/data"]

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -sf http://localhost:9111/health || exit 1

ENTRYPOINT ["/app/epicode-cloud"]
