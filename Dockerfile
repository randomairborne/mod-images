FROM ghcr.io/randomairborne/asset-squisher:latest AS client-builder

WORKDIR /assets/

COPY assets uncompressed

RUN asset-squisher uncompressed compressed --no-compress-images

FROM rust:alpine AS server-builder

RUN apk add musl-dev

COPY . .

RUN cargo build --release

FROM alpine:latest

COPY --from=server-builder ./target/release/mod-images /usr/bin/mod-images
COPY --from=client-builder /assets/compressed/ /var/www/mod-images/assets/

ENV ASSET_DIR="/var/www/mod-images/assets/"
CMD ["/usr/bin/mod-images"]