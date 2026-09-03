FROM scratch
LABEL org.opencontainers.image.source=https://github.com/Notgnoshi/scorarium
COPY target/x86_64-unknown-linux-musl/release/scorarium /scorarium
ENV SCORARIUM_DATA_DIR=/data
WORKDIR /data
EXPOSE 3000
USER 1000:1000
ENTRYPOINT ["/scorarium"]
