ARG LLVMTARGETARCH
FROM --platform=${BUILDPLATFORM} ghcr.io/randomairborne/cross-cargo:${LLVMTARGETARCH} AS server-builder
ARG LLVMTARGETARCH

WORKDIR /build

COPY . .

RUN cargo build --release --target ${LLVMTARGETARCH}-unknown-linux-musl

FROM ghcr.io/randomairborne/asset-squisher:latest AS client-builder

WORKDIR /assets/

COPY assets uncompressed

RUN asset-squisher uncompressed compressed --no-compress-images

FROM alpine:latest
ARG LLVMTARGETARCH

COPY --from=server-builder /build/target/${LLVMTARGETARCH}-unknown-linux-musl/release/mod-images /usr/bin/mod-images
COPY --from=client-builder /assets/compressed/ /var/www/mod-images/assets/

ENV ASSET_DIR="/var/www/mod-images/assets/"
ENTRYPOINT ["/usr/bin/mod-images"]