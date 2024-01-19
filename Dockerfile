FROM alpine AS client-builder

RUN apk add zstd brotli pigz

WORKDIR /assets/

COPY assets .

RUN find /assets -type f ! -name "*.png" -exec pigz -k9 '{}' \; -exec pigz -zk9 '{}' \; -exec brotli -k9 '{}' \; -exec zstd -qk19 '{}' \;

FROM rust:alpine AS server-builder

RUN apk add musl-dev

COPY . .

RUN cargo build --release

FROM alpine

COPY --from=server-builder ./target/release/mod-images /usr/bin/mod-images
COPY --from=client-builder /assets/ /var/www/mod-images/assets/
COPY /templates/ /var/www/mod-images/templates/

ENV ASSET_DIR="/var/www/mod-images/assets/"
ENV TEMPLATE_DIR="/var/www/mod-images/templates/"
ENTRYPOINT "/usr/bin/mod-images"