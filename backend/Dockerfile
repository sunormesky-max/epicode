FROM rust:1.85-bookworm AS builder

WORKDIR /build
COPY . .

RUN cargo build --release --bin cloud

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*

RUN groupadd -r epicode && useradd -r -g epicode -d /app -s /sbin/nologin epicode

WORKDIR /app
COPY --from=builder /build/target/release/cloud /app/cloud
COPY --from=builder /build/models/model.onnx /app/models/model.onnx
COPY --from=builder /build/models/tokenizer.json /app/models/tokenizer.json

RUN chmod 500 /app/cloud && chown -R epicode:epicode /app
USER epicode
RUN mkdir -p /app/data

ENV RUST_LOG=info
EXPOSE 9110
VOLUME ["/app/data"]

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -sf http://localhost:9110/health || exit 1

ENTRYPOINT ["/app/cloud"]
