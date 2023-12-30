FROM alpine AS client-builder

RUN apk add zstd brotli gzip

COPY . .

RUN find ./client/ -type f ! -name "*.png" -exec gzip -k9 '{}' \; -exec brotli -k9 '{}' \; -exec zstd -qk19 '{}' \;

FROM rust:alpine AS server-builder

RUN apk add musl-dev

COPY . .

RUN cargo build --release

FROM alpine

COPY --from=server-builder ./target/release/mod-images /usr/bin/mod-images
COPY --from=client-builder ./client/ /var/www/mod-images/

ENV CLIENT_DIR=/var/www/mod-images
ENTRYPOINT "/usr/bin/mod-images"