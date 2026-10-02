FROM scratch
LABEL org.opencontainers.image.source=https://github.com/Notgnoshi/scorarium
COPY --from=alpine:3 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY target/x86_64-unknown-linux-musl/release/scorarium /scorarium
ENV SCORARIUM_DATA_DIR=/data
USER 1000:1000
WORKDIR /data
EXPOSE 3000
ENTRYPOINT ["/scorarium"]
