# Static musl `lade` on alpine; ca-certificates is required for TLS to vault backends.

FROM alpine:3 AS fetch
ARG VERSION
ARG TARGETARCH
RUN apk add --no-cache curl tar coreutils
RUN set -eu; \
  case "$TARGETARCH" in \
  amd64) TARGET=x86_64-unknown-linux-musl ;; \
  arm64) TARGET=aarch64-unknown-linux-musl ;; \
  *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
  esac; \
  base="https://github.com/zifeo/lade/releases/download/v${VERSION}"; \
  asset="lade-v${VERSION}-${TARGET}.tar.gz"; \
  curl -fsSL "${base}/${asset}" -o /tmp/lade.tar.gz; \
  curl -fsSL "${base}/${asset}.sha256" -o /tmp/lade.sha256; \
  echo "$(cut -d' ' -f1 /tmp/lade.sha256)  /tmp/lade.tar.gz" | sha256sum -c -; \
  tar -xzf /tmp/lade.tar.gz -C /usr/local/bin lade; \
  chmod +x /usr/local/bin/lade

FROM alpine:3
RUN apk add --no-cache ca-certificates
COPY --from=fetch /usr/local/bin/lade /usr/local/bin/lade
ENTRYPOINT ["/usr/local/bin/lade"]
