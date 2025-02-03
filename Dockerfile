FROM rust:latest AS server-builder

WORKDIR /build

COPY . .

RUN cargo build --release

FROM ghcr.io/randomairborne/asset-squisher:latest AS client-builder

WORKDIR /assets/

COPY assets uncompressed

RUN asset-squisher uncompressed compressed --no-compress-images

FROM alpine:latest

COPY --from=server-builder /build/target/release/mod-images /usr/bin/mod-images
COPY --from=client-builder /assets/compressed/ /var/www/mod-images/assets/

ENV ASSET_DIR="/var/www/mod-images/assets/"
ENTRYPOINT ["/usr/bin/mod-images"]