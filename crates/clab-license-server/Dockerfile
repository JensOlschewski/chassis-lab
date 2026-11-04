# Build from the repository root:
#   git archive HEAD | docker build -t clab-license -f crates/clab-license-server/Dockerfile -
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p clab-license-server

FROM debian:bookworm-slim
COPY --from=build /src/target/release/clab-license-server /usr/local/bin/clab-license-server
USER 1000:1000
ENTRYPOINT ["clab-license-server", "--data", "/data"]
CMD ["serve", "--listen", "0.0.0.0:8080"]
